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
    r.hidden = st["hidden"].boolean();
    return r;
}

// What a built-in entry is made of, and whether the folder tree's menu has it. The file list's menu has
// every entry, greyed while it cannot run, and leaves out only what the verb rule hides (Extract on
// something that is not an archive): one menu over a selection and over empty space alike.
// Spec: DirectoryTabView.PrepareFileListMenu (Offer).
struct Spec {
    const char* id;
    std::vector<const char*> commands;  // one item each; the group's caption is used when `group` is set
    const wchar_t* group;               // a submenu of the commands, or null for one item per command
    bool tree;
};

const std::vector<Spec>& specs() {
    // id, commands, group, tree
    static const std::vector<Spec> table = {
        {"new", {"file.new-folder", "file.new-file"}, L"N&ew", true},
        {"open", {"file.open"}, nullptr, false},
        {"run-as-admin", {"file.run-as-admin"}, nullptr, false},
        {"open-new-tab", {"file.open-new-tab"}, nullptr, true},
        {"open-new-pane", {"file.open-pane-right", "file.open-pane-below"}, L"Open in new &pane", true},
        {"open-terminal", {"file.open-terminal"}, nullptr, true},
        {"open-vscode", {"file.open-vscode"}, nullptr, true},
        {"disk-usage", {"tools.disk-usage"}, nullptr, true},
        {"duplicates", {"tools.duplicates"}, nullptr, true},
        {"changes", {"tools.changes"}, nullptr, true},
        {"compare-panes", {"tools.compare"}, nullptr, false},
        {"compress", {"file.compress"}, nullptr, false},
        {"extract", {"file.extract-here", "file.extract-to"}, nullptr, false},
        {"copy-path", {"edit.copy-path"}, nullptr, true},
        {"copy-name", {"edit.copy-name"}, nullptr, false},
        {"compare-files", {"file.compare-files"}, nullptr, false},
        {"settle-by-content", {"file.settle-by-content"}, nullptr, false},
        {"verify-checksums", {"file.verify-checksums"}, nullptr, false},
        {"checksum", {"file.checksum"}, nullptr, false},
        {"cut", {"edit.cut"}, nullptr, false},
        {"copy", {"edit.copy"}, nullptr, false},
        {"paste", {"edit.paste"}, nullptr, false},
        {"rename", {"file.rename"}, nullptr, false},
        {"delete", {"file.delete"}, nullptr, true},
        {"delete-permanently", {"file.delete-permanently"}, nullptr, false},
        {"bookmark", {"file.bookmark"}, nullptr, true},
        {"properties", {"file.properties"}, nullptr, true},
    };
    return table;
}

const Spec* spec_of(const std::string& id) {
    for (const Spec& s : specs()) {
        if (id == s.id) return &s;
    }
    return nullptr;
}

// The tree's entries act on the folder right-clicked, not on the file list's selection, so each one is the
// command's caption and icon with the tree's own action; the shortcut is the file list's, so none is shown.
MenuItems tree_items(MainView& main, const Spec& s, const std::wstring& subject) {
    MenuItems items;
    for (const char* c : s.commands) {
        BarRow r = command_row(main.core(), c);
        if (r.id.empty()) continue;
        std::wstring caption = r.caption;
        if (r.id == "file.bookmark") {
            caption = bb_is_bookmarked(main.core().app(), core::s(subject)) ? L"Remove &bookmark" : L"&Bookmark";
        }
        MenuItem m = MenuItem::make(caption);
        m.icon = icon_for(r.icon);
        // A drive is not a folder that can be deleted.
        m.enabled = !(r.id == "file.delete" && subject.size() <= 3);
        MainView* mv = &main;
        const std::wstring id = core::wide(r.id);
        m.on_invoke = [mv, id, subject] { mv->tree_verb(id, subject); };
        items.push_back(std::move(m));
    }
    if (items.empty() || !s.group) return items;
    MenuItem group = MenuItem::make(s.group);
    group.icon = icon_for(command_row(main.core(), s.commands.front()).icon);
    group.sub = std::move(items);
    return MenuItems{std::move(group)};
}

// The file types the New submenu offers after New file, each a New file dialog with the type's extension and
// name (Settings > New items). Spec: Views/NewItemMenu.
void add_new_file_types(MainView& main, MenuItems& items, const std::wstring& folder, bool enabled) {
    if (items.empty() || !items.front().is_submenu()) return;
    const json::Value types = try_model([&](BbOwnedStr* o) { return bb_new_file_types(main.core().app(), o); });
    if (types.size() == 0) return;
    MenuItems& sub = items.front().sub;
    sub.push_back(MenuItem::separator());
    for (size_t i = 0; i < types.size(); ++i) {
        const std::wstring label = core::wide(types[i]["label"].str());
        const std::wstring ext = core::wide(types[i]["extension"].str());
        std::wstring caption;
        for (const wchar_t ch : label) caption += ch == L'&' ? std::wstring(L"&&") : std::wstring(1, ch);
        MenuItem m = MenuItem::make(caption, {}, ext);
        m.icon = icon_for("NewFile");
        m.enabled = enabled;
        MainView* mv = &main;
        m.on_invoke = [mv, ext, label, folder] { mv->open_new_file(ext, label, folder); };
        sub.push_back(std::move(m));
    }
}

MenuItems built_in_items(MainView& main, const Spec& s, MenuKind kind, const std::wstring& subject) {
    const bool background = kind == MenuKind::Background, tree = kind == MenuKind::Tree;
    if (tree && !s.tree) return {};
    if (tree) {
        MenuItems items = tree_items(main, s, subject);
        if (std::string_view(s.id) == "new") add_new_file_types(main, items, subject, true);
        return items;
    }
    std::vector<BarRow> rows;
    for (const char* c : s.commands) {
        // The folder's own path is what Copy as path means over empty space.
        std::string id = c;
        if (background && id == "edit.copy-path") id = "edit.copy-folder-path";
        BarRow r = command_row(main.core(), id);
        if (r.id.empty() || r.hidden) continue;
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
    MenuItems items = build_menu_items(main.menu_env(), rows);
    if (std::string_view(s.id) == "new") {
        add_new_file_types(main, items, L"", command_row(main.core(), "file.new-file").enabled);
    }
    return items;
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

ComposedMenu compose_context_menu(MainView& main, MenuKind kind, const std::wstring& subject) {
    core::Core& core = main.core();
    ComposedMenu out;
    const bool tree = kind == MenuKind::Tree;
    // A folder in the tree is an item in its parent, which is what the handlers are told; a drive root has
    // no parent and stands in for itself.
    std::wstring folder = core.tab_text(main.active_tab(), BB_TEXT_PATH);
    if (tree) {
        folder = subject;
        const size_t cut = subject.find_last_of(L'\\');
        if (subject.size() > 3 && cut != std::wstring::npos) folder = subject.substr(0, cut < 3 ? 3 : cut);
    }

    // Who the menu is about.
    std::vector<ShellTarget> targets;
    if (kind == MenuKind::Files) {
        for (const auto& r : main.selected_rows()) targets.push_back({r.path, r.dir});
    }
    if (tree) targets.push_back({subject, true});
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
        declared[id] = built_in_items(main, *s, kind, subject);
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
        if (tree) {
            BbApp* app = core.app();
            commands.emplace(id, MenuItem::make(escape_caption(w(c["name"])), [app, id, subject] {
                bb_launch_command_at(app, core::s(L"custom:" + core::wide(id)), core::s(subject));
            }));
        } else {
            commands.emplace(id, MenuItem::make(escape_caption(w(c["name"])), [mv, id] { mv->run_command(L"custom:" + core::wide(id)); }));
        }
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
        const size_t summary_at = items.size();
        items.push_back(std::move(s));
        // Pointing at a step marks it and every newer one, and the summary counts them, which is what
        // a click would undo. Spec: MainWindow.UndoHistory (Mark).
        for (size_t i = 0; i < shown; ++i) {
            items[i].on_hover = [shown, summary_at, verb](MenuItems& level, int at) {
                for (size_t k = 0; k < shown; ++k) level[k].marked = static_cast<int>(k) <= at;
                level[summary_at].text = verb + L" " + std::to_wstring(at + 1) + (at == 0 ? L" action" : L" actions");
            };
        }
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
