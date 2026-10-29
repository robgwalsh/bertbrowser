#include "views/shell_props.h"

#include <windows.h>

#include <propsys.h>
#include <propvarutil.h>
#include <shobjidl.h>

#include <map>

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

std::string machine_property_list() {
    IPropertyDescriptionList* list = nullptr;
    if (FAILED(PSEnumeratePropertyDescriptions(PDEF_VIEWABLE, IID_PPV_ARGS(&list))) || !list) return "[]";
    UINT count = 0;
    list->GetCount(&count);
    std::string out = "[";
    bool first = true;
    for (UINT i = 0; i < count; ++i) {
        IPropertyDescription* d = nullptr;
        if (FAILED(list->GetAt(i, IID_PPV_ARGS(&d))) || !d) continue;
        PWSTR name = nullptr, display = nullptr;
        if (SUCCEEDED(d->GetCanonicalName(&name)) && name) {
            d->GetDisplayName(&display);
            if (!first) out += ",";
            first = false;
            out += "[" + jq(name) + "," + jq(display ? display : name) + "]";
        }
        if (name) CoTaskMemFree(name);
        if (display) CoTaskMemFree(display);
        d->Release();
    }
    list->Release();
    return out + "]";
}

}  // namespace bb::views
