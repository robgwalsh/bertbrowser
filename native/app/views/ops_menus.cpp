// The right-click menus, composed: the app's own entries, the user's commands and each shell extension's
// contribution laid out in the order the Context menu page saved (one token list for the file list's menu
// and the folder tree's), then tidied so a hidden group never leaves two separators touching. And the
// Undo / Redo dropdown lists. Spec: Views/ContextMenuComposer, BuiltInMenu, ShellMenu, MainWindow.UndoHistory.
#include "views/ops_menus.h"

#include "views/command_ui.h"
#include "views/main_view.h"

#include <algorithm>
#include <set>

namespace bb::views::ops {

using namespace bb::ui;

namespace {

std::wstring w(const json::Value& v) { return core::wide(v.str()); }

std::string up(std::string s) {
    std::transform(s.begin(), s.end(), s.begin(), [](unsigned char c) { return static_cast<char>(std::toupper(c)); });
    return s;
}

BarRow command_row(core::Core& core, const std::string& id) {
    BarRow r;
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_command_state(core.app(), core::s(core::wide(id)), o); });
    if (st.is_null()) return r;
    r.kind = "command";
    r.id = st["id"].str();
    r.icon = st["icon"].str();
    r.name = core::wide(st["name"].str());
    r.caption = escape_caption(core::wide(st["caption"].is_null() ? st["name"].str() : st["caption"].str()));
    r.gesture = core::wide(st["gesture"].str());
    r.enabled = st["whyNot"].is_null();
    r.reason = core::wide(st["whyNot"].str());
    r.checked = st["checked"].boolean();
    return r;
}

// What a built-in entry is made of, and whether the menu always shows it or only while it can run.
struct Spec {
    const char* id;
    std::vector<const char*> commands;  // one item each; the group's caption is used when `group` is set
    const wchar_t* group;               // a submenu of the commands, or null for one item per command
    bool files_always, background, background_always, tree, tree_always;
};

const std::vector<Spec>& specs() {
    // id, commands, group, files_always, background, background_always, tree, tree_always
    static const std::vector<Spec> table = {
        {"new", {"file.new-folder", "file.new-file"}, L"N&ew", true, true, true, true, true},
        {"open", {"file.open"}, nullptr, true, false, false, false, false},
        {"run-as-admin", {"file.run-as-admin"}, nullptr, false, false, false, false, false},
        {"open-new-tab", {"file.open-new-tab"}, nullptr, false, false, false, true, true},
        {"open-new-pane", {"file.open-pane-right", "file.open-pane-below"}, L"Open in new &pane", false, false, false, true, true},
        {"open-terminal", {"file.open-terminal"}, nullptr, false, true, false, true, true},
        {"open-vscode", {"file.open-vscode"}, nullptr, false, true, false, true, true},
        {"disk-usage", {"tools.disk-usage"}, nullptr, false, true, false, true, true},
        {"duplicates", {"tools.duplicates"}, nullptr, false, true, false, true, true},
        {"changes", {"tools.changes"}, nullptr, false, true, false, true, true},
        {"compare-panes", {"tools.compare"}, nullptr, false, true, false, false, false},
        {"compress", {"file.compress"}, nullptr, true, false, false, false, false},
        {"extract", {"file.extract-here", "file.extract-to"}, nullptr, false, false, false, false, false},
        {"copy-path", {"edit.copy-path"}, nullptr, true, true, true, true, true},
        {"copy-name", {"edit.copy-name"}, nullptr, false, false, false, false, false},
        {"compare-files", {"file.compare-files"}, nullptr, false, false, false, false, false},
        {"settle-by-content", {"file.settle-by-content"}, nullptr, false, false, false, false, false},
        {"verify-checksums", {"file.verify-checksums"}, nullptr, false, false, false, false, false},
        {"checksum", {"file.checksum"}, nullptr, false, false, false, false, false},
        {"cut", {"edit.cut"}, nullptr, true, false, false, false, false},
        {"copy", {"edit.copy"}, nullptr, true, false, false, false, false},
        {"paste", {"edit.paste"}, nullptr, true, true, true, false, false},
        {"rename", {"file.rename"}, nullptr, true, false, false, false, false},
        {"delete", {"file.delete"}, nullptr, true, false, false, true, true},
        {"delete-permanently", {"file.delete-permanently"}, nullptr, false, false, false, false, false},
        {"bookmark", {"file.bookmark"}, nullptr, true, true, true, true, true},
        {"properties", {"file.properties"}, nullptr, true, true, true, true, true},
    };
    return table;
}

const Spec* spec_of(const std::string& id) {
    for (const Spec& s : specs()) {
        if (id == s.id) return &s;
    }
    return nullptr;
}

MenuItems built_in_items(MainView& main, const Spec& s, MenuKind kind) {
    const bool files = kind == MenuKind::Files, background = kind == MenuKind::Background, tree = kind == MenuKind::Tree;
    if ((background && !s.background) || (tree && !s.tree)) return {};
    const bool always = files ? s.files_always : background ? s.background_always : s.tree_always;
    std::vector<BarRow> rows;
    for (const char* c : s.commands) {
        // The folder's own path is what Copy as path means over empty space.
        std::string id = c;
        if (background && id == "edit.copy-path") id = "edit.copy-folder-path";
        BarRow r = command_row(main.core(), id);
        if (r.id.empty()) continue;
        if (!always && !r.enabled) continue;
        rows.push_back(std::move(r));
    }
    if (rows.empty()) return {};
    if (s.group) {
        BarRow m;
        m.kind = "menu";
        m.caption = s.group;
        m.children = std::move(rows);
        rows.clear();
        rows.push_back(std::move(m));
    }
    return build_menu_items(main.menu_env(), rows);
}

MenuItem from_entry(const ShellEntry& e, const std::shared_ptr<ShellMenuSession>& session, MainView& main) {
    if (e.separator) return MenuItem::separator();
    MenuItem m = MenuItem::make(e.header, {}, e.gesture);
    m.enabled = e.enabled;
    if (!e.children.empty()) {
        for (const ShellEntry& c : e.children) m.sub.push_back(from_entry(c, session, main));
    } else {
        const int command = e.command;
        MainView* mv = &main;
        m.on_invoke = [session, command, mv] {
            const std::wstring said = session->invoke(command, mv->window().hwnd());
            (void)said;
        };
    }
    return m;
}

}  // namespace

ComposedMenu compose_context_menu(MainView& main, MenuKind kind) {
    core::Core& core = main.core();
    ComposedMenu out;
    const std::wstring folder = core.tab_text(main.active_tab(), BB_TEXT_PATH);

    // Who the menu is about.
    std::vector<ShellTarget> targets;
    if (kind == MenuKind::Files) {
        for (const auto& r : main.selected_rows()) targets.push_back({r.path, r.dir});
    }
    if (targets.empty()) targets.push_back({folder, true});
    const ShellContext context = kind == MenuKind::Background ? ShellContext::Background : ShellContext::Items;

    // The app's own entries this menu declares, minus the ones unticked on the Context menu page.
    const json::Value page = query(core, "context-menu");
    std::set<std::string> hidden;
    for (size_t i = 0; i < page["hiddenBuiltIns"].size(); ++i) {
        std::string h = page["hiddenBuiltIns"][i].str();
        std::transform(h.begin(), h.end(), h.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
        hidden.insert(h);
    }
    std::map<std::string, MenuItems> declared;
    std::vector<std::wstring> declared_ids;
    for (size_t i = 0; i < page["builtIns"].size(); ++i) {
        const json::Value& b = page["builtIns"][i];
        const std::string id = b["id"].str();
        const std::string places = b["places"].str();
        if (kind == MenuKind::Tree ? places == "file-list" : places == "folder-tree") continue;
        if (hidden.count(id)) continue;
        const Spec* s = spec_of(id);
        if (!s) continue;
        declared[id] = built_in_items(main, *s, kind);
        declared_ids.push_back(core::wide(id));
    }

    // The user's own commands that apply here.
    std::map<std::string, MenuItem> commands;
    std::vector<std::wstring> command_ids;
    for (size_t i = 0; i < page["commands"].size(); ++i) {
        const json::Value& c = page["commands"][i];
        if (!c["showInMenu"].boolean()) continue;
        bool applies = false;
        for (const auto& t : targets) applies = applies || (t.is_directory ? c["appliesToDirectories"].boolean() : c["appliesToFiles"].boolean());
        if (!applies || commands.count(c["id"].str())) continue;
        const std::string id = c["id"].str();
        MainView* mv = &main;
        commands.emplace(id, MenuItem::make(escape_caption(w(c["name"])), [mv, id] { mv->run_command(L"custom:" + core::wide(id)); }));
        command_ids.push_back(core::wide(id));
    }

    // Other programs' entries, one group per extension that is ticked.
    std::shared_ptr<ShellMenuSession> session;
    std::map<std::string, const ShellGroup*> groups;
    std::vector<std::wstring> group_ids;
    if (ShellMenuSource* source = main.shell_source()) {
        try {
            session = std::shared_ptr<ShellMenuSession>(source->open(targets, context, folder));
        } catch (const std::exception&) {
        }
    }
    if (session) {
        std::vector<std::wstring> all;
        for (const ShellGroup& g : session->groups) all.push_back(core::wide(g.extension_id));
        const json::Value shown = query(core, "shell-visible", "{\"ids\":" + qa(all) + "}");
        std::set<std::string> visible;
        for (size_t i = 0; i < shown.size(); ++i) visible.insert(shown[i].str());
        for (const ShellGroup& g : session->groups) {
            if (!visible.count(g.extension_id) || groups.count(up(g.extension_id))) continue;
            groups[up(g.extension_id)] = &g;
            group_ids.push_back(core::wide(g.extension_id));
        }
    }

    // The order, from the one layout.
    const json::Value placements = query(core, "menu-arrange",
                                         "{\"builtIns\":" + qa(declared_ids) + ",\"commands\":" + qa(command_ids) + ",\"shell\":" + qa(group_ids) + "}");
    std::vector<MenuItems> slots;
    std::string slot_kinds = "[";
    for (size_t i = 0; i < placements.size(); ++i) {
        const std::string pk = placements[i]["kind"].str(), id = placements[i]["id"].str();
        MenuItems items;
        if (pk == "builtin") {
            for (const auto& key : declared) {
                std::string a = key.first, b = id;
                std::transform(a.begin(), a.end(), a.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
                std::transform(b.begin(), b.end(), b.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
                if (a == b) items = key.second;
            }
        } else if (pk == "command") {
            const auto it = commands.find(id);
            if (it != commands.end()) items.push_back(it->second);
        } else if (pk == "shell") {
            const auto it = groups.find(up(id));
            if (it != groups.end()) {
                for (const ShellEntry& e : it->second->entries) items.push_back(from_entry(e, session, main));
            }
        }
        const char* slot = pk == "separator" ? "separator" : items.empty() ? "hidden" : "item";
        slot_kinds += std::string(i ? "," : "") + "\"" + slot + "\"";
        if (pk == "separator") items.push_back(MenuItem::separator());
        slots.push_back(std::move(items));
    }
    slot_kinds += "]";
    const json::Value visible = query(core, "menu-tidy", "{\"slots\":" + slot_kinds + "}");
    for (size_t i = 0; i < slots.size(); ++i) {
        if (i < visible.size() && !visible[i].boolean()) continue;
        for (MenuItem& m : slots[i]) out.items.push_back(std::move(m));
    }
    out.keep = session;
    return out;
}

// ---- the Undo and Redo lists ----------------------------------------------------------------------------

bool build_step_menu(MainView& main, const std::wstring& kind, MenuItems* out) {
    const bool undo = kind == L"undo" || kind == L"undo-range";
    if (!undo && kind != L"redo") return false;
    core::Core& core = main.core();
    const json::Value h = query(core, "history");
    const json::Value& rows = h["rows"];
    // Newest first for undo; for redo the nearest to the cursor first, which is the oldest of the undone.
    std::vector<const json::Value*> list;
    if (undo) {
        for (size_t i = 0; i < rows.size(); ++i) {
            if (rows[i]["isDone"].boolean()) list.push_back(&rows[i]);
        }
    } else {
        for (size_t i = rows.size(); i-- > 0;) {
            if (!rows[i]["isDone"].boolean()) list.push_back(&rows[i]);
        }
    }
    constexpr size_t kDepth = 12;
    const std::wstring verb = undo ? L"Undo" : L"Redo";
    const int hover = kind == L"undo-range" ? 2 : -1;
    MenuItems items;
    BbApp* app = core.app();
    const size_t shown = std::min(list.size(), kDepth);
    for (size_t i = 0; i < shown; ++i) {
        std::wstring text = w((*list[i])["description"]);
        for (size_t at = 0; (at = text.find(L'&', at)) != std::wstring::npos; at += 2) text.insert(at, 1, L'&');
        MenuItem m = MenuItem::make(text, {}, w((*list[i])["whenText"]));
        m.icon = icon_for((*list[i])["icon"].str());
        const int steps = static_cast<int>((*list[i])["steps"].i64());
        m.on_invoke = [app, steps, undo] {
            uint64_t op = 0;
            bb_undo_step(app, undo ? steps : -steps, &op);
        };
        m.marked = hover >= 0 && static_cast<int>(i) <= std::min<int>(hover, static_cast<int>(shown) - 1);
        items.push_back(std::move(m));
    }
    if (list.empty()) {
        MenuItem none = MenuItem::make(L"Nothing to " + std::wstring(undo ? L"undo" : L"redo"));
        none.enabled = false;
        items.push_back(std::move(none));
    } else {
        if (list.size() > kDepth) {
            MenuItem more = MenuItem::make(L"…and " + std::to_wstring(list.size() - kDepth) + L" older in the history");
            more.enabled = false;
            items.push_back(std::move(more));
        }
        items.push_back(MenuItem::separator());
        const int through = hover >= 0 ? std::min<int>(hover, static_cast<int>(shown) - 1) : -1;
        std::wstring summary;
        if (through < 0) summary = L"Point at an action to " + std::wstring(undo ? L"undo back" : L"redo up") + L" to it";
        else summary = verb + L" " + std::to_wstring(through + 1) + (through == 0 ? L" action" : L" actions");
        MenuItem s = MenuItem::make(summary);
        s.enabled = false;
        items.push_back(std::move(s));
    }
    items.push_back(MenuItem::separator());
    MainView* mv = &main;
    BbOwnedStr g{};
    std::wstring gesture;
    if (bb_keymap_gesture(app, core::s(L"edit.undo-history"), &g) == BB_OK) gesture = core::take(g);
    items.push_back(MenuItem::make(L"Show history…", [mv] { mv->open_dialog(L"undo-history"); }, gesture));
    *out = std::move(items);
    return true;
}

}  // namespace bb::views::ops
