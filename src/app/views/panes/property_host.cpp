#include "views/panes/property_host.h"

#include <windows.h>

#include <propsys.h>
#include <propvarutil.h>
#include <shobjidl.h>
#include <wrl/client.h>

#include <set>
#include <string>
#include <vector>

#include "views/bridge/core.h"
#include "views/bridge/json.h"

#pragma comment(lib, "propsys.lib")

namespace bb::views {

using Microsoft::WRL::ComPtr;

namespace {

void ensure_com() {
    static thread_local bool done = false;
    if (!done) {
        CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
        done = true;
    }
}

// The text handed back to Rust lives here until this thread's next call.
thread_local std::wstring g_out;

BbStr give(std::string utf8_json) {
    g_out = core::wide(utf8_json);
    return core::s(g_out);
}

std::string quote(const std::wstring& w) {
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

ComPtr<IPropertyStore> open_store(const std::wstring& path, GETPROPERTYSTOREFLAGS flags) {
    ComPtr<IPropertyStore> store;
    if (FAILED(SHGetPropertyStoreFromParsingName(path.c_str(), nullptr, flags, IID_PPV_ARGS(&store)))) return nullptr;
    return store;
}

std::wstring display_of(const PROPERTYKEY& key, const PROPVARIANT& pv) {
    PWSTR text = nullptr;
    std::wstring out;
    if (SUCCEEDED(PSFormatForDisplayAlloc(key, pv, PDFF_DEFAULT, &text)) && text) {
        out = text;
        CoTaskMemFree(text);
    }
    while (!out.empty() && iswspace(out.back())) out.pop_back();
    return out;
}

// A list of strings joins with semicolons, the way the pane types them.
std::wstring text_of(const PROPERTYKEY& key, const PROPVARIANT& pv) {
    if (pv.vt == VT_LPWSTR && pv.pwszVal) return pv.pwszVal;
    if (pv.vt == (VT_VECTOR | VT_LPWSTR)) {
        std::wstring out;
        for (ULONG i = 0; i < pv.calpwstr.cElems; ++i) {
            if (i) out += L"; ";
            out += pv.calpwstr.pElems[i];
        }
        return out;
    }
    return display_of(key, pv);
}

bool number_of(const PROPVARIANT& pv, double* out) {
    switch (pv.vt) {
        case VT_I1: case VT_I2: case VT_I4: case VT_I8: case VT_UI1: case VT_UI2: case VT_UI4: case VT_UI8:
        case VT_R4: case VT_R8: case VT_INT: case VT_UINT:
            return SUCCEEDED(PropVariantToDouble(pv, out));
        default: return false;
    }
}

std::wstring hresult_text(HRESULT hr) {
    wchar_t buf[512] = {};
    const DWORD n = FormatMessageW(FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS, nullptr, static_cast<DWORD>(hr), 0, buf, 512, nullptr);
    std::wstring s(buf, n);
    while (!s.empty() && (iswspace(s.back()) || s.back() == L'.')) s.pop_back();
    return s.empty() ? L"Windows refused" : s;
}

BbStatus details(void*, BbStr path_view, BbStr* out) {
    try {
        ensure_com();
        auto store = open_store(core::w(path_view), static_cast<GETPROPERTYSTOREFLAGS>(GPS_BESTEFFORT | GPS_OPENSLOWITEM));
        if (!store) {
            *out = give("[]");
            return BB_OK;
        }
        DWORD count = 0;
        store->GetCount(&count);
        std::set<std::wstring> seen;
        std::string json = "[";
        bool first = true;
        for (DWORD i = 0; i < count; ++i) {
            PROPERTYKEY key{};
            if (FAILED(store->GetAt(i, &key))) continue;
            ComPtr<IPropertyDescription> desc;
            if (FAILED(PSGetPropertyDescription(key, IID_PPV_ARGS(&desc))) || !desc) continue;
            PWSTR canonical = nullptr, display = nullptr;
            if (FAILED(desc->GetCanonicalName(&canonical)) || !canonical) continue;
            desc->GetDisplayName(&display);
            const std::wstring name = display ? display : L"";
            const std::wstring canon = canonical;
            CoTaskMemFree(canonical);
            if (display) CoTaskMemFree(display);
            if (name.empty() || !seen.insert(name).second) continue;
            PROPVARIANT pv;
            PropVariantInit(&pv);
            if (SUCCEEDED(store->GetValue(key, &pv)) && pv.vt != VT_EMPTY && pv.vt != VT_NULL) {
                const std::wstring value = display_of(key, pv);
                if (!value.empty()) {
                    json += std::string(first ? "" : ",") + "{\"canonical\":" + quote(canon) + ",\"name\":" + quote(name) + ",\"value\":" + quote(value) + "}";
                    first = false;
                }
            }
            PropVariantClear(&pv);
        }
        *out = give(json + "]");
        return BB_OK;
    } catch (...) {
        *out = give("[]");
        return BB_OK;
    }
}

BbStatus read_fields(void*, BbStr path_view, BbStr names_view, bool ask_writable, BbStr* out) {
    try {
        ensure_com();
        const std::wstring path = core::w(path_view);
        const json::Value names = json::parse(core::utf8(core::w(names_view)));
        std::vector<std::wstring> canonicals;
        for (size_t i = 0; i < names.size(); ++i) canonicals.push_back(core::wide(names[i].str()));

        // What this type's handler writes. Asked of a store opened for writing, because IsPropertyWritable
        // lies on a read-only one (a .txt, which has no handler, answers yes to everything); and that open is
        // exclusive, which is why the core asks once per extension.
        std::set<std::wstring> writable;
        bool asked = false;
        if (ask_writable) {
            auto rw = open_store(path, GPS_READWRITE);
            if (!rw) {
                *out = give("null");
                return BB_OK;
            }
            ComPtr<IPropertyStoreCapabilities> caps;
            if (SUCCEEDED(rw.As(&caps))) {
                asked = true;
                for (const auto& c : canonicals) {
                    PROPERTYKEY key{};
                    if (SUCCEEDED(PSGetPropertyKeyFromName(c.c_str(), &key)) && caps->IsPropertyWritable(key) == S_OK) writable.insert(c);
                }
            }
        }

        // The values come from an ordinary shared read, like a column's.
        auto store = open_store(path, static_cast<GETPROPERTYSTOREFLAGS>(GPS_BESTEFFORT | GPS_OPENSLOWITEM));
        if (!store) {
            *out = give("null");
            return BB_OK;
        }
        std::string json = "{\"writable\":";
        if (!ask_writable) {
            json += "null";
        } else {
            json += "[";
            bool first = true;
            for (const auto& c : writable) {
                json += std::string(first ? "" : ",") + quote(c);
                first = false;
            }
            json += "]";
        }
        json += ",\"values\":[";
        bool first = true;
        for (const auto& c : canonicals) {
            if (asked && !writable.count(c)) continue;
            PROPERTYKEY key{};
            if (FAILED(PSGetPropertyKeyFromName(c.c_str(), &key))) continue;
            PROPVARIANT pv;
            PropVariantInit(&pv);
            if (SUCCEEDED(store->GetValue(key, &pv))) {
                std::string row = "{\"canonical\":" + quote(c);
                if (pv.vt != VT_EMPTY && pv.vt != VT_NULL) {
                    row += ",\"text\":" + quote(text_of(key, pv));
                    double n = 0;
                    if (number_of(pv, &n)) {
                        char buf[64];
                        snprintf(buf, sizeof(buf), "%.17g", n);
                        row += std::string(",\"number\":") + buf;
                    }
                    if (pv.vt == VT_FILETIME) {
                        const long long ticks = static_cast<long long>(pv.filetime.dwHighDateTime) << 32 | pv.filetime.dwLowDateTime;
                        row += ",\"utc\":" + std::to_string(ticks);
                    }
                }
                json += std::string(first ? "" : ",") + row + "}";
                first = false;
            }
            PropVariantClear(&pv);
        }
        *out = give(json + "]}");
        return BB_OK;
    } catch (...) {
        *out = give("null");
        return BB_OK;
    }
}

BbStatus write_fields(void*, BbStr path_view, BbStr values_view, BbStr* error) {
    try {
        ensure_com();
        const std::wstring path = core::w(path_view);
        const json::Value values = json::parse(core::utf8(core::w(values_view)));
        ComPtr<IPropertyStore> store;
        HRESULT hr = SHGetPropertyStoreFromParsingName(path.c_str(), nullptr, GPS_READWRITE, IID_PPV_ARGS(&store));
        if (FAILED(hr) || !store) {
            g_out = hresult_text(hr);
            *error = core::s(g_out);
            return BB_ERR;
        }
        for (size_t i = 0; i < values.size(); ++i) {
            const json::Value& v = values[i];
            const std::wstring canonical = core::wide(v["canonical"].str());
            PROPERTYKEY key{};
            if (FAILED(PSGetPropertyKeyFromName(canonical.c_str(), &key))) {
                g_out = L"Windows does not know the property " + canonical;
                *error = core::s(g_out);
                return BB_ERR;
            }
            PROPVARIANT pv;
            PropVariantInit(&pv);
            if (!v["text"].is_null()) {
                InitPropVariantFromString(core::wide(v["text"].str()).c_str(), &pv);
            } else if (!v["number"].is_null()) {
                InitPropVariantFromUInt32(static_cast<ULONG>(v["number"].num()), &pv);
            } else if (!v["utc"].is_null()) {
                const long long ticks = static_cast<long long>(v["utc"].num());
                FILETIME ft{static_cast<DWORD>(ticks & 0xFFFFFFFF), static_cast<DWORD>(ticks >> 32)};
                InitPropVariantFromFileTime(&ft, &pv);
            }
            // Into the type the property is declared with: one string becomes the list System.Keywords is.
            if (pv.vt != VT_EMPTY) {
                hr = PSCoerceToCanonicalValue(key, &pv);
                if (FAILED(hr)) {
                    PropVariantClear(&pv);
                    g_out = hresult_text(hr);
                    *error = core::s(g_out);
                    return BB_ERR;
                }
            }
            hr = store->SetValue(key, pv);
            PropVariantClear(&pv);
            if (FAILED(hr)) {
                g_out = hresult_text(hr);
                *error = core::s(g_out);
                return BB_ERR;
            }
        }
        hr = store->Commit();
        if (FAILED(hr)) {
            g_out = hresult_text(hr);
            *error = core::s(g_out);
            return BB_ERR;
        }
        return BB_OK;
    } catch (...) {
        g_out = L"Windows refused";
        *error = core::s(g_out);
        return BB_ERR;
    }
}

}  // namespace

BbPropertyHost make_property_host() {
    BbPropertyHost host{};
    host.user = nullptr;
    host.details = &details;
    host.read_fields = &read_fields;
    host.write_fields = &write_fields;
    return host;
}

}  // namespace bb::views
