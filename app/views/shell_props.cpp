#include "views/shell_props.h"

#include <windows.h>

#include <propsys.h>
#include <propvarutil.h>
#include <shobjidl.h>

#include <map>
#include <mutex>
#include <optional>
#include <thread>
#include <utility>

#include "views/core.h"

namespace bb::views {

namespace {

std::string jq(const std::wstring& w) {
    const std::string s = core::utf8(w);
    std::string out = "\"";
    for (const unsigned char c : s) {
        switch (c) {
            case '\\': out += "\\\\"; break;
            case '"': out += "\\\""; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    snprintf(buf, sizeof(buf), "\\u%04x", c);
                    out += buf;
                } else {
                    out += static_cast<char>(c);
                }
        }
    }
    return out + "\"";
}

}  // namespace

std::string read_property_cells(const json::Value& requests) {
    std::map<std::wstring, IPropertyStore*> stores;
    std::string out = "[";
    bool first = true;
    for (size_t i = 0; i < requests.size(); ++i) {
        const std::wstring path = core::wide(requests[i][static_cast<size_t>(0)].str());
        const std::wstring canonical = core::wide(requests[i][static_cast<size_t>(1)].str());
        std::string value;
        PROPERTYKEY key{};
        if (SUCCEEDED(PSGetPropertyKeyFromName(canonical.c_str(), &key))) {
            IPropertyStore*& store = stores[path];
            if (!store) {
                if (FAILED(SHGetPropertyStoreFromParsingName(path.c_str(), nullptr, GPS_DEFAULT, IID_PPV_ARGS(&store)))) store = nullptr;
            }
            PROPVARIANT pv;
            PropVariantInit(&pv);
            if (store && SUCCEEDED(store->GetValue(key, &pv)) && pv.vt != VT_EMPTY && pv.vt != VT_NULL) {
                PWSTR text = nullptr;
                std::wstring display;
                if (SUCCEEDED(PSFormatForDisplayAlloc(key, pv, PDFF_DEFAULT, &text)) && text) {
                    display = text;
                    CoTaskMemFree(text);
                }
                if (!display.empty()) {
                    value = "{\"display\":" + jq(display);
                    double number = 0;
                    if (pv.vt == VT_FILETIME) {
                        const long long ticks = static_cast<long long>(pv.filetime.dwHighDateTime) << 32 | pv.filetime.dwLowDateTime;
                        value += ",\"date\":" + std::to_string(ticks);
                    } else if (SUCCEEDED(PropVariantToDouble(pv, &number))) {
                        char buf[64];
                        snprintf(buf, sizeof(buf), "%.17g", number);
                        value += std::string(",\"number\":") + buf;
                    }
                    value += "}";
                }
            }
            PropVariantClear(&pv);
        }
        if (!first) out += ",";
        first = false;
        out += "{\"path\":" + jq(path) + ",\"canonical\":" + jq(canonical);
        if (!value.empty()) out += ",\"value\":" + value;
        out += "}";
    }
    for (auto& kv : stores) {
        if (kv.second) kv.second->Release();
    }
    return out + "]";
}

std::vector<std::pair<std::wstring, std::wstring>> read_all_properties(const std::wstring& path) {
    std::vector<std::pair<std::wstring, std::wstring>> out;
    IPropertyStore* store = nullptr;
    if (FAILED(SHGetPropertyStoreFromParsingName(path.c_str(), nullptr, GPS_BESTEFFORT, IID_PPV_ARGS(&store))) || !store) return out;
    std::map<std::wstring, std::wstring, std::less<>> by_name;
    DWORD count = 0;
    store->GetCount(&count);
    for (DWORD i = 0; i < count; ++i) {
        PROPERTYKEY key{};
        if (FAILED(store->GetAt(i, &key))) continue;
        // A key with no registered description is internal plumbing; Explorer hides those too.
        IPropertyDescription* d = nullptr;
        if (FAILED(PSGetPropertyDescription(key, IID_PPV_ARGS(&d))) || !d) continue;
        PWSTR display = nullptr;
        d->GetDisplayName(&display);
        d->Release();
        std::wstring name = display ? display : L"";
        if (display) CoTaskMemFree(display);
        if (name.empty() || by_name.count(name)) continue;
        PROPVARIANT pv;
        PropVariantInit(&pv);
        if (SUCCEEDED(store->GetValue(key, &pv)) && pv.vt != VT_EMPTY && pv.vt != VT_NULL) {
            PWSTR text = nullptr;
            if (SUCCEEDED(PSFormatForDisplayAlloc(key, pv, PDFF_DEFAULT, &text)) && text) {
                std::wstring value = text;
                CoTaskMemFree(text);
                while (!value.empty() && iswspace(value.back())) value.pop_back();
                if (!value.empty()) by_name[name] = value;
            }
        }
        PropVariantClear(&pv);
    }
    store->Release();
    for (auto& kv : by_name) out.emplace_back(kv.first, kv.second);
    return out;
}

// Every description the system has, minus the plumbing: a property with no display name is not one a person
// could pick. Spec: ShellProperties.EnumerateDescriptions (PDEF_ALL).
std::string machine_property_list() {
    IPropertyDescriptionList* list = nullptr;
    if (FAILED(PSEnumeratePropertyDescriptions(PDEF_ALL, IID_PPV_ARGS(&list))) || !list) return "[]";
    UINT count = 0;
    list->GetCount(&count);
    std::string out = "[";
    bool first = true;
    for (UINT i = 0; i < count; ++i) {
        IPropertyDescription* d = nullptr;
        if (FAILED(list->GetAt(i, IID_PPV_ARGS(&d))) || !d) continue;
        PWSTR name = nullptr, display = nullptr;
        if (SUCCEEDED(d->GetCanonicalName(&name)) && name && *name && SUCCEEDED(d->GetDisplayName(&display)) && display && *display) {
            if (!first) out += ",";
            first = false;
            out += "[" + jq(name) + "," + jq(display) + "]";
        }
        if (name) CoTaskMemFree(name);
        if (display) CoTaskMemFree(display);
        d->Release();
    }
    list->Release();
    return out + "]";
}

namespace {

// The one background read of the property system. Leaked on purpose: a worker still enumerating when the
// process exits must not find its mutex destroyed under it.
struct MachineRead {
    std::mutex lock;
    bool started = false, done = false;
    std::optional<std::string> list;
};

MachineRead& machine_read() {
    static MachineRead* state = new MachineRead();
    return *state;
}

}  // namespace

void start_machine_property_read() {
    MachineRead& s = machine_read();
    {
        std::lock_guard<std::mutex> g(s.lock);
        if (s.started) return;
        s.started = true;
    }
    // The property system is free-threaded: the worker joins the multithreaded apartment, enumerates, and
    // leaves the answer here for the UI thread to take. Spec: ColumnAddPanel.Preload (Task.Run).
    std::thread([&s] {
        const HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
        std::string list = machine_property_list();
        if (SUCCEEDED(hr)) CoUninitialize();
        std::lock_guard<std::mutex> g(s.lock);
        s.list = std::move(list);
        s.done = true;
    }).detach();
}

bool machine_property_read_done() {
    MachineRead& s = machine_read();
    std::lock_guard<std::mutex> g(s.lock);
    return s.done;
}

std::optional<std::string> take_machine_property_list() {
    MachineRead& s = machine_read();
    std::lock_guard<std::mutex> g(s.lock);
    return s.list;  // kept: a second main view (a harness restart) asks again
}

}  // namespace bb::views
