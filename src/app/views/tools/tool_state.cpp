#include "views/tools/tool_state.h"

#include "views/main/main_view.h"
#include "views/bridge/model.h"

#include <map>

namespace bb::views {

ToolsUi& tools_ui(MainView& main) {
    // Plain data, keyed by the one window that asks; nothing here holds a view.
    static std::map<const MainView*, ToolsUi> table;
    return table[&main];
}

std::string json_quote(const std::wstring& text) {
    const std::string utf8 = core::utf8(text);
    std::string out = "\"";
    for (const unsigned char c : utf8) {
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
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

std::string json_array(const std::vector<std::wstring>& items) {
    std::string out = "[";
    for (size_t i = 0; i < items.size(); ++i) out += (i ? "," : "") + json_quote(items[i]);
    return out + "]";
}

json::Value tool_call(MainView& main, const char* verb, const std::string& args) {
    const std::wstring v = core::wide(verb);
    const std::wstring a = core::wide(args);
    return model([&](BbOwnedStr* o) { return bb_tools(main.core().app(), core::s(v), core::s(a), o); }, verb);
}

json::Value try_tool_call(MainView& main, const char* verb, const std::string& args, std::wstring* error) {
    try {
        return tool_call(main, verb, args);
    } catch (const std::exception& e) {
        if (error) *error = core::wide(e.what());
        return {};
    }
}

bool tool_host_command(MainView& main, const std::wstring& id) {
    if (id == L"file.checksum") {
        main.open_dialog(L"checksum");
    } else if (id == L"file.verify-checksums") {
        main.open_dialog(L"checksum-verify");
    } else if (id == L"file.compare-files") {
        main.open_dialog(L"compare-files");
    } else if (id == L"tools.disk-usage") {
        main.open_dialog(L"disk-usage");
    } else if (id == L"tools.changes") {
        main.open_dialog(L"changes");
    } else if (id == L"tools.compare-sync") {
        main.open_dialog(L"sync-preview");
    } else if (id == L"tools.compare-rescan") {
        try_tool_call(main, "compare.rescan");
    } else if (id == L"tools.compare-differences") {
        const json::Value m = try_tool_call(main, "compare.model");
        try_tool_call(main, "compare.filter", m["differencesOnly"].boolean() ? "{\"on\":false}" : "{\"on\":true}");
    } else if (id == L"file.settle-by-content") {
        std::vector<std::wstring> paths;
        for (const SelectedRow& r : main.selected_rows()) {
            if (!r.dir) paths.push_back(r.path);
        }
        std::vector<BbStr> views;
        for (const auto& p : paths) views.push_back(core::s(p));
        // The bytes are read on a worker; the rows re-paint when the verdicts are in.
        if (!views.empty()) bb_compare_settle_start(main.core().app(), views.data(), static_cast<uint32_t>(views.size()));
    } else {
        return false;
    }
    return true;
}

}  // namespace bb::views
