// The Context menu page: one list of rows - the app's entries, other programs', the user's own commands,
// separators and the `@more` marker where anything unplaced lands - and what is in the list *is* the
// membership: saving writes the layout, the two hidden lists and each command's "show in menu" from it, so no
// arrangement can un-hide anything on its own. Spec: SettingsViewModel.ContextMenu.cs, SettingsView.xaml.
#include "views/main/main_view.h"
#include "views/dialogs/file_pickers.h"
#include "views/bridge/ops.h"
#include "views/settings/settings_view.h"

#include <algorithm>
#include <map>
#include <set>

namespace bb::views {

using namespace bb::ui;
namespace f = forms;

namespace {

std::string upper(std::string s) {
    std::transform(s.begin(), s.end(), s.begin(), [](unsigned char c) { return static_cast<char>(std::toupper(c)); });
    return s;
}

struct Row {
    std::string token;  // `-`, `@more`, `app:id`, `cmd:id`, or an extension id
    std::wstring name, detail, gesture;
    const IconData* icon = nullptr;
    // An entry another program adds: faded while the master switch for those is off.
    bool shell = false;
    bool separator() const { return token == "-"; }
    bool more() const { return upper(token) == "@MORE"; }
};

struct Model {
    json::Value page;
    std::vector<ShellExtensionInfo> catalog;
    std::vector<Row> rows;
};

std::wstring shell_detail(const ShellExtensionInfo* e) {
    if (!e) return L"not found on this computer";
    return e->handler ? L"extension" : L"command";
}

// What the menu itself will print beside an entry: the shortcut in force, not the one it shipped with.
std::wstring gesture_text(BbApp* app, const json::Value& b) {
    if (b["commandId"].str().empty()) return L"";
    BbOwnedStr o{};
    return bb_keymap_gesture(app, core::s(core::wide(b["commandId"].str())), &o) == BB_OK ? core::take(o) : L"";
}

Row row_for_built_in(BbApp* app, const json::Value& b) {
    Row r;
    r.token = b["token"].str();
    r.name = core::wide(b["name"].str());
    r.detail = core::wide(b["detail"].str());
    r.gesture = gesture_text(app, b);
    r.icon = icon_for(b["icon"].str());
    return r;
}

const ShellExtensionInfo* find_ext(const Model& m, const std::string& id) {
    for (const auto& e : m.catalog) {
        if (upper(e.id) == upper(id)) return &e;
    }
    return nullptr;
}

bool has_row(const std::vector<Row>& rows, const std::string& token) {
    for (const Row& r : rows) {
        if (!r.separator() && upper(r.token) == upper(token)) return true;
    }
    return false;
}

size_t more_index(const std::vector<Row>& rows) {
    for (size_t i = 0; i < rows.size(); ++i) {
        if (rows[i].more()) return i;
    }
    return rows.size();
}

// The rows the saved settings describe, with whatever is new placed ahead of `@more`.
Model read_model(MainView& main) {
    Model m;
    m.page = ops::query(main.core(), "context-menu");
    try {
        if (ShellMenuSource* s = main.shell_source()) m.catalog = s->catalog();
    } catch (const std::exception&) {
    }
    std::set<std::string> hidden_built, hidden_shell;
    for (size_t i = 0; i < m.page["hiddenBuiltIns"].size(); ++i) hidden_built.insert(upper(m.page["hiddenBuiltIns"][i].str()));
    for (size_t i = 0; i < m.page["hiddenShell"].size(); ++i) hidden_shell.insert(upper(m.page["hiddenShell"][i].str()));
    std::map<std::string, const json::Value*> built_ins, commands;
    for (size_t i = 0; i < m.page["builtIns"].size(); ++i) built_ins[upper(m.page["builtIns"][i]["id"].str())] = &m.page["builtIns"][i];
    for (size_t i = 0; i < m.page["commands"].size(); ++i) commands[m.page["commands"][i]["id"].str()] = &m.page["commands"][i];

    const json::Value& layout = m.page["layout"];
    for (size_t i = 0; i < layout.size(); ++i) {
        const std::string token = layout[i].str();
        Row r;
        if (token == "-") {
            r.token = token;
        } else if (upper(token) == "@MORE") {
            r.token = token;
            r.name = L"New entries from other programs appear here";
        } else if (token.rfind("app:", 0) == 0) {
            const auto it = built_ins.find(upper(token.substr(4)));
            if (it == built_ins.end() || hidden_built.count(upper(token.substr(4)))) continue;
            r = row_for_built_in(main.core().app(), *it->second);
        } else if (token.rfind("cmd:", 0) == 0) {
            const auto it = commands.find(token.substr(4));
            if (it == commands.end() || !(*it->second)["showInMenu"].boolean()) continue;
            r.token = token;
            r.name = core::wide((*it->second)["name"].str());
            r.detail = L"your command";
            r.icon = icon_for((*it->second)["runElevated"].boolean() ? "Icon.Shield" : "Icon.CustomCommand");
        } else {
            if (hidden_shell.count(upper(token))) continue;
            const ShellExtensionInfo* e = find_ext(m, token);
            r.token = token;
            r.name = e ? core::wide(e->name) : core::wide(token);
            r.detail = shell_detail(e);
            r.shell = true;
        }
        m.rows.push_back(std::move(r));
    }

    // Anything the layout has not placed: the user's commands, then other programs' entries.
    std::vector<Row> fresh;
    for (const auto& [id, c] : commands) {
        if (!(*c)["showInMenu"].boolean() || has_row(m.rows, "cmd:" + id)) continue;
        Row r;
        r.token = "cmd:" + id;
        r.name = core::wide((*c)["name"].str());
        r.detail = L"your command";
        r.icon = icon_for("Icon.CustomCommand");
        fresh.push_back(std::move(r));
    }
    for (const auto& e : m.catalog) {
        if (hidden_shell.count(upper(e.id)) || has_row(m.rows, e.id)) continue;
        Row r;
        r.token = e.id;
        r.name = core::wide(e.name);
        r.detail = shell_detail(&e);
        r.shell = true;
        fresh.push_back(std::move(r));
    }
    if (!fresh.empty()) {
        size_t at = more_index(m.rows);
        Row sep;
        sep.token = "-";
        m.rows.insert(m.rows.begin() + static_cast<std::ptrdiff_t>(at++), sep);
        for (Row& r : fresh) m.rows.insert(m.rows.begin() + static_cast<std::ptrdiff_t>(at++), std::move(r));
    }
    return m;
}

// One command as the settings file spells it, with what the editor changed laid over it.
struct CommandFields {
    std::string id;
    std::wstring name, command, arguments;
    bool show_in_menu = true, files = true, folders = false, elevated = false;
};

CommandFields command_of(const json::Value& c) {
    return {c["id"].str(),
            core::wide(c["name"].str()),
            core::wide(c["command"].str()),
            core::wide(c["arguments"].str()),
            c["showInMenu"].boolean(),
            c["appliesToFiles"].boolean(),
            c["appliesToDirectories"].boolean(),
            c["runElevated"].boolean()};
}

std::string command_json(const CommandFields& c) {
    return "{" + (c.id.empty() ? std::string() : "\"id\":" + ops::q(core::wide(c.id)) + ",") + "\"name\":" + ops::q(c.name) +
           ",\"command\":" + ops::q(c.command) + ",\"arguments\":" + ops::q(c.arguments) + ",\"showInMenu\":" + (c.show_in_menu ? "true" : "false") +
           ",\"appliesToFiles\":" + (c.files ? "true" : "false") + ",\"appliesToDirectories\":" + (c.folders ? "true" : "false") +
           ",\"runElevated\":" + (c.elevated ? "true" : "false") + "}";
}

std::string commands_json(const std::vector<CommandFields>& all) {
    std::string out = "[";
    for (size_t i = 0; i < all.size(); ++i) out += (i ? "," : "") + command_json(all[i]);
    return out + "]";
}

std::vector<CommandFields> commands_of_list(const json::Value& list) {
    std::vector<CommandFields> out;
    for (size_t i = 0; i < list.size(); ++i) out.push_back(command_of(list[i]));
    return out;
}

std::vector<CommandFields> commands_of(const json::Value& page) { return commands_of_list(page["commands"]); }

bool is_blank(const std::wstring& t) {
    return std::all_of(t.begin(), t.end(), [](wchar_t c) { return iswspace(c) != 0; });
}

// What the core refuses to save. Spec: SettingsViewModel.CommandProblem.
bool incomplete(const CommandFields& c) { return is_blank(c.name) || is_blank(c.command) || (!c.files && !c.folders); }

std::string array_of(const std::vector<std::string>& items) {
    std::string out = "[";
    for (size_t i = 0; i < items.size(); ++i) out += (i ? "," : "") + ops::q(core::wide(items[i]));
    return out + "]";
}

}  // namespace

// What the list holds is the membership: writing all three from it is how no arrangement can un-hide anything.
static void apply_rows(SettingsView& view, MainView& main, const Model& m, const std::vector<Row>& rows) {
    (void)view;
    BbApp* app = main.core().app();
    auto set = [&](const char* key, const std::string& json) { bb_settings_set(app, core::s(core::wide(key)), core::s(core::wide(json))); };
    std::vector<std::string> layout, hidden_built, hidden_shell;
    for (const Row& r : rows) layout.push_back(r.token);
    for (size_t i = 0; i < m.page["builtIns"].size(); ++i) {
        const std::string token = m.page["builtIns"][i]["token"].str();
        if (!has_row(rows, token)) hidden_built.push_back(m.page["builtIns"][i]["id"].str());
    }
    // Hidden ids of extensions that are not installed stay hidden until the page is saved with them present.
    std::set<std::string> seen;
    for (const auto& e : m.catalog) seen.insert(upper(e.id));
    for (size_t i = 0; i < m.page["hiddenShell"].size(); ++i) {
        const std::string id = m.page["hiddenShell"][i].str();
        if (!seen.count(upper(id)) && !has_row(rows, id)) hidden_shell.push_back(id);
    }
    for (const auto& e : m.catalog) {
        if (!has_row(rows, e.id)) hidden_shell.push_back(e.id);
    }
    set("contextMenuLayout", array_of(layout));
    set("hiddenBuiltInMenuItems", array_of(hidden_built));
    set("hiddenShellExtensions", array_of(hidden_shell));
    // Each command's own switch follows its row.
    std::vector<CommandFields> cmds = commands_of(m.page);
    bool changed = false;
    for (CommandFields& c : cmds) {
        const bool in_menu = has_row(rows, "cmd:" + c.id);
        changed = changed || in_menu != c.show_in_menu;
        c.show_in_menu = in_menu;
    }
    if (changed) set("customCommands", commands_json(cmds));
}

void SettingsView::open_new_command() {
    command_editor_ = true;
    signature_ += "*";
    rebuild_if_changed(true);
}

void SettingsView::page_context_menu(Widget& p) {
    MainView& main = main_;
    f::heading(p, L"Context menu");
    f::note(p, L"What a right-click on a file offers. Entries added by other programs (7-Zip, Git, TortoiseSVN and the like) are listed "
               L"with a tick beside each; untick one to hide it. Anything new that gets installed shows up without a visit here.");
    bool_row(p, L"Show entries added by other programs", "showShellExtensions");

    auto model = std::make_shared<Model>(read_model(main));
    if (model->catalog.empty() && get("showShellExtensions").boolean(true)) {
        auto* st = f::text(p, L"No program on this computer adds right-click entries.", tok::TextMuted, 13.0f);
        st->name = "ShellExtensionsStatus";
    }
    auto* cols = f::row(p, 14);
    cols->height = 430;

    // -- the menu, in order
    auto* left = cols->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    left->flex = 1;
    f::text(*left, L"In the menu", tok::TextSecondary, 13.0f, 600);
    auto* list = left->emplace<f::ItemList>();
    list->name = "MenuLayoutList";
    list->flex = 1;
    list->row_height = 28;
    const bool show_shell = get("showShellExtensions").boolean(true);
    std::vector<f::ListEntry> entries;
    for (const Row& r : model->rows) {
        f::ListEntry e;
        if (r.separator()) {
            e.text = L"\u2500\u2500\u2500\u2500\u2500  separator  \u2500\u2500\u2500\u2500\u2500";
            e.muted = true;
        } else if (r.more()) {
            e.text = r.name;
            e.muted = true;
        } else {
            e.text = r.name;
            // The note, then the shortcut, as the menu prints them. Other programs' rows fade while they are switched off.
            e.detail = r.detail + (r.detail.empty() || r.gesture.empty() ? L"" : L"   ") + r.gesture;
            e.icon = r.icon;
            e.muted = r.shell && !show_shell;
        }
        entries.push_back(std::move(e));
    }
    if (menu_selected_ >= static_cast<int>(entries.size())) menu_selected_ = -1;
    list->set_entries(std::move(entries), menu_selected_);
    // Selecting one of your commands opens its editor below, as selecting it on the right does.
    list->on_select = [this, model](int i) {
        menu_selected_ = i;
        const bool command = i >= 0 && static_cast<size_t>(i) < model->rows.size() && model->rows[static_cast<size_t>(i)].token.rfind("cmd:", 0) == 0;
        const std::string id = command ? model->rows[static_cast<size_t>(i)].token.substr(4) : std::string();
        if (id != command_editing_ && (command || !command_editing_.empty())) {
            command_editing_ = id;
            command_editor_ = false;
            main_.defer([this] { rebuild_if_changed(true); });
        }
    };

    auto commit = [this, model, &main](std::vector<Row> rows, int select) {
        menu_selected_ = select;
        apply_rows(*this, main, *model, rows);
        rebuild_if_changed(true);
    };
    auto move_by = [this, model, commit](int d) {
        const int at = menu_selected_, to = at + d, n = static_cast<int>(model->rows.size());
        if (at < 0 || at >= n || to < 0 || to >= n) return;
        std::vector<Row> rows = model->rows;
        std::swap(rows[static_cast<size_t>(at)], rows[static_cast<size_t>(to)]);
        commit(std::move(rows), to);
    };
    auto remove_selected = [this, model, commit] {
        const int at = menu_selected_;
        if (at < 0 || at >= static_cast<int>(model->rows.size()) || model->rows[static_cast<size_t>(at)].more()) return;
        std::vector<Row> rows = model->rows;
        rows.erase(rows.begin() + at);
        commit(std::move(rows), -1);
    };
    // Alt+Up/Down moves the selected row and Del takes it off the menu. Spec: SettingsView.xaml.cs (MenuLayoutList_KeyDown).
    // The list is rebuilt by either, so focus comes back through its automation id and a Del waits for the key to finish.
    list->automation_id = "MenuLayoutList";
    nudge_ = move_by;
    nudge_for_ = list;
    // A row dragged to a new place. Spec: SettingsView.xaml.cs (MenuLayoutList drag).
    list->on_reorder = [model, commit](int from, int to) {
        const int n = static_cast<int>(model->rows.size());
        if (from < 0 || from >= n || to < 0 || to >= n) return;
        std::vector<Row> rows = model->rows;
        Row moved = std::move(rows[static_cast<size_t>(from)]);
        rows.erase(rows.begin() + from);
        rows.insert(rows.begin() + to, std::move(moved));
        commit(std::move(rows), to);
    };
    list->on_key = [this, remove_selected](unsigned vk) {
        if (vk != VK_DELETE) return false;
        main_.defer(remove_selected);
        return true;
    };
    auto* tools = f::row(*left, 6);
    f::button(*tools, L"Move up", [move_by] { move_by(-1); })->name = "MenuMoveUp";
    f::button(*tools, L"Move down", [move_by] { move_by(1); })->name = "MenuMoveDown";
    f::button(*tools, L"Remove", [remove_selected] { remove_selected(); })->name = "MenuRemove";
    auto* tools2 = f::row(*left, 6);
    f::button(*tools2, L"Add separator", [this, model, commit] {
        std::vector<Row> rows = model->rows;
        const size_t at = menu_selected_ >= 0 ? static_cast<size_t>(menu_selected_) + 1 : more_index(rows);
        Row sep;
        sep.token = "-";
        rows.insert(rows.begin() + static_cast<std::ptrdiff_t>(std::min(at, rows.size())), sep);
        commit(std::move(rows), static_cast<int>(at));
    })->name = "MenuAddSeparator";
    f::button(*tools2, L"New command\u2026", [this] { open_new_command(); })->name = "MenuNewCommand";
    // The app's entries back in their original order and all on the menu; your commands and other programs'
    // entries keep whether they are on it and go back to where unplaced entries land. Spec: ResetMenu.
    auto* reset = f::button(*tools2, L"Reset", [this, model, commit] {
        std::vector<Row> rows;
        std::map<std::string, const json::Value*> built_ins;
        for (size_t i = 0; i < model->page["builtIns"].size(); ++i) built_ins[upper(model->page["builtIns"][i]["token"].str())] = &model->page["builtIns"][i];
        for (size_t i = 0; i < model->page["defaultLayout"].size(); ++i) {
            const std::string token = model->page["defaultLayout"][i].str();
            if (token == "-") {
                Row sep;
                sep.token = token;
                rows.push_back(sep);
            } else if (upper(token) == "@MORE") {
                Row more;
                more.token = token;
                more.name = L"New entries from other programs appear here";
                rows.push_back(more);
            } else if (const auto it = built_ins.find(upper(token)); it != built_ins.end()) {
                rows.push_back(row_for_built_in(core_.app(), *it->second));
            }
        }
        // Your commands, then other programs' entries: each group behind a separator of its own, just above the
        // "new entries" row. Spec: SettingsViewModel.PlaceUnplaced.
        std::vector<Row> commands, shell;
        for (const Row& r : model->rows) {
            if (r.separator() || r.more() || r.token.rfind("app:", 0) == 0) continue;
            (r.token.rfind("cmd:", 0) == 0 ? commands : shell).push_back(r);
        }
        for (std::vector<Row>* group : {&commands, &shell}) {
            if (group->empty()) continue;
            size_t at = more_index(rows);
            Row sep;
            sep.token = "-";
            rows.insert(rows.begin() + static_cast<std::ptrdiff_t>(at++), sep);
            for (Row& r : *group) rows.insert(rows.begin() + static_cast<std::ptrdiff_t>(at++), std::move(r));
        }
        commit(std::move(rows), -1);
    });
    reset->name = "MenuReset";
    reset->tooltip = L"BertBrowser's entries back in their original order, all on the menu";
    f::text(*left, L"Alt+↑/↓ moves · Del removes", tok::TextMuted, 11.0f);

    if (dragging_pose_) list->set_drop_line(3);  // the line a drag draws between two rows, placed rather than dragged

    // -- what can be in it
    auto* right = cols->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    right->width = 320;
    f::text(*right, L"Add or remove", tok::TextSecondary, 13.0f, 600);
    auto* choices = right->emplace<f::ItemList>();
    choices->name = "MenuChoiceList";
    choices->flex = 1;
    choices->row_height = 26;
    std::vector<f::ListEntry> ce;
    std::vector<std::string> tokens;
    auto heading = [&](const wchar_t* t) {
        f::ListEntry h;
        h.heading = true;
        h.text = t;
        ce.push_back(std::move(h));
        tokens.emplace_back();
    };
    // Other programs' entries cannot be added or taken out while the switch for them is off.
    std::set<std::string> switched_off;
    auto choice = [&](const std::string& token, const std::wstring& label, const std::wstring& detail, bool shell = false) {
        f::ListEntry e;
        e.text = std::wstring(has_row(model->rows, token) ? L"\u2611  " : L"\u2610  ") + label;
        e.detail = detail;
        if (shell && !show_shell) {
            e.enabled = false;
            switched_off.insert(token);
        }
        ce.push_back(std::move(e));
        tokens.push_back(token);
    };
    heading(L"This app");
    for (size_t i = 0; i < model->page["builtIns"].size(); ++i) {
        const json::Value& b = model->page["builtIns"][i];
        choice(b["token"].str(), core::wide(b["name"].str()), core::wide(b["detail"].str()));
    }
    if (!model->catalog.empty()) {
        heading(L"Other programs");
        for (const auto& e : model->catalog) choice(e.id, core::wide(e.name), shell_detail(&e), true);
    }
    if (model->page["commands"].size() > 0) {
        heading(L"Your commands");
        for (size_t i = 0; i < model->page["commands"].size(); ++i) {
            const json::Value& c = model->page["commands"][i];
            choice("cmd:" + c["id"].str(), core::wide(c["name"].str()), L"");
        }
    }
    choices->set_entries(std::move(ce));
    // A choice dragged onto the menu lands exactly where it is dropped; one already on it moves there.
    // Spec: SettingsView.xaml.cs (MenuChoice drag, MenuLayoutList_Drop).
    choices->tooltip = L"Double-click to add or take out; drag onto the menu to put it exactly where you want";
    choices->can_drag = [tokens, switched_off](int i) {
        return i >= 0 && static_cast<size_t>(i) < tokens.size() && !tokens[static_cast<size_t>(i)].empty() && !switched_off.count(tokens[static_cast<size_t>(i)]);
    };
    choices->drag_into = list;
    list->on_drop_in = [model, commit, tokens, switched_off](int from, int at) {
        if (from < 0 || static_cast<size_t>(from) >= tokens.size() || tokens[static_cast<size_t>(from)].empty()) return;
        const std::string& token = tokens[static_cast<size_t>(from)];
        if (switched_off.count(token)) return;
        std::vector<Row> rows = model->rows;
        Row r;
        r.token = token;
        for (size_t k = 0; k < rows.size(); ++k) {
            if (!rows[k].separator() && upper(rows[k].token) == upper(token)) {
                r = std::move(rows[k]);
                rows.erase(rows.begin() + static_cast<std::ptrdiff_t>(k));
                if (static_cast<int>(k) < at) --at;
                break;
            }
        }
        at = std::clamp(at, 0, static_cast<int>(rows.size()));
        rows.insert(rows.begin() + at, std::move(r));
        commit(std::move(rows), at);
    };
    choices->on_select = [this, tokens](int i) {
        if (i < 0 || static_cast<size_t>(i) >= tokens.size()) return;
        const std::string& token = tokens[static_cast<size_t>(i)];
        const std::string id = token.rfind("cmd:", 0) == 0 ? token.substr(4) : std::string();
        if (id.empty() || id == command_editing_) return;
        command_editing_ = id;
        command_editor_ = false;
        main_.defer([this] { rebuild_if_changed(true); });
    };
    choices->on_activate = [this, model, commit, tokens, switched_off](int i) {
        if (i < 0 || static_cast<size_t>(i) >= tokens.size() || tokens[static_cast<size_t>(i)].empty()) return;
        const std::string& token = tokens[static_cast<size_t>(i)];
        if (switched_off.count(token)) return;
        std::vector<Row> rows = model->rows;
        for (size_t k = 0; k < rows.size(); ++k) {
            if (!rows[k].separator() && upper(rows[k].token) == upper(token)) {
                rows.erase(rows.begin() + static_cast<std::ptrdiff_t>(k));
                commit(std::move(rows), -1);
                return;
            }
        }
        Row r;
        r.token = token;
        const size_t at = menu_selected_ >= 0 ? static_cast<size_t>(menu_selected_) + 1 : more_index(rows);
        rows.insert(rows.begin() + static_cast<std::ptrdiff_t>(std::min(at, rows.size())), r);
        commit(std::move(rows), static_cast<int>(at));
    };
    f::note(*right, L"Double-click to add or take out. The list on the left is the order of both right-click menus.");
    f::note(*right, L"Programs of your own. In Arguments, {path}, {name} and {dir} expand per item \u2014 quote tokens that may contain spaces, e.g. \"{path}\". Blank arguments default to the quoted full path.");

    // -- one of your commands, edited where it is: every change is written at once (a command missing a
    // field keeps the whole list unwritten, and the strip says why). Spec: SettingsView.xaml (CommandEditor).
    // The commands as typed while the core holds them unwritten (a missing field), else the saved ones. The hold is
    // the core's: once it is gone (fixed, discarded, left) the saved list is the truth again.
    if (try_model([&](BbOwnedStr* o) { return bb_settings_page_state(main.core().app(), o); })["problem"].str().empty()) pending_commands_ = json::Value();
    auto all = std::make_shared<std::vector<CommandFields>>(pending_commands_.is_null() ? commands_of(model->page) : commands_of_list(pending_commands_));
    if (reveal_problem_ && !pending_commands_.is_null()) {
        reveal_problem_ = false;
        const auto bad = std::find_if(all->begin(), all->end(), incomplete);
        if (bad != all->end()) {
            command_editing_ = bad->id;
            command_editor_ = false;
        }
    }
    const auto editing = std::find_if(all->begin(), all->end(), [this](const CommandFields& c) { return c.id == command_editing_; });
    if (!command_editor_ && !command_editing_.empty() && editing != all->end()) {
        const size_t index = static_cast<size_t>(editing - all->begin());
        // Written at once; if the core kept it out of the settings (a missing field) the list stays here, as typed.
        auto write = [this, all] {
            const std::string text = commands_json(*all);
            set_raw("customCommands", text);
            pending_commands_ = commands_json(commands_of_list(get("customCommands"))) == text ? json::Value() : json::parse(text);
        };
        f::gap(p, 6);
        f::text(p, L"Your command", tok::TextSecondary, 14.0f, 600);
        auto* card = p.emplace<StackPanel>(Orientation::Vertical, 8.0f);
        card->name = "CommandEditor";
        card->pad = 12;
        card->style.bg = tok::SurfaceRaised;
        card->style.border = tok::BorderSubtle;
        const CommandFields& c = (*all)[index];
        auto field = [&](const wchar_t* label, const std::wstring& value, const char* id, std::function<void(CommandFields&, const std::wstring&)> apply) {
            auto* r = f::row(*card);
            f::row_label(*r, label, 130);
            auto* e = f::field(*r, value, 360, [all, index, write, apply](const std::wstring& t) {
                apply((*all)[index], t);
                write();
            });
            e->name = id;
            e->automation_id = id;
            return r;
        };
        field(L"Name", c.name, "CommandName", [](CommandFields& f2, const std::wstring& t) { f2.name = t; });
        auto* program = field(L"Program", c.command, "CommandProgram", [](CommandFields& f2, const std::wstring& t) { f2.command = t; });
        auto* browse = f::button(*program, L"\u2026", [this, all, index, write] {
            PickPath pick;
            pick.title = L"Choose a program";
            pick.filters = {{L"Programs", L"*.exe;*.cmd;*.bat;*.com"}, {L"All files", L"*.*"}};
            if (const auto chosen = pick_open_file(main_.window().hwnd(), pick)) {
                (*all)[index].command = *chosen;
                write();
                main_.defer([this] { rebuild_if_changed(true); });
            }
        }, 32);
        browse->tooltip = L"Browse for a program";
        browse->set_enabled(!main.scripted());
        auto* args = field(L"Arguments", c.arguments, "CommandArguments", [](CommandFields& f2, const std::wstring& t) { f2.arguments = t; });
        args->tooltip = L"{path} = full path, {name} = file name, {dir} = parent directory";
        auto* switches = f::row(*card, 16);
        f::check(*switches, L"Show for files", c.files, [all, index, write](bool v) {
            (*all)[index].files = v;
            write();
        });
        f::check(*switches, L"Show for folders", c.folders, [all, index, write](bool v) {
            (*all)[index].folders = v;
            write();
        });
        auto* admin = f::check(*card, L"Run as administrator", c.elevated, [all, index, write](bool v) {
            (*all)[index].elevated = v;
            write();
        });
        admin->tooltip = L"Windows asks for confirmation each time this command runs.";
        auto* buttons = f::row(*card);
        f::button(*buttons, L"Delete", [this, all, index, write] {
            // Off the menu and out of settings. Spec: DeleteCustomCommand.
            all->erase(all->begin() + static_cast<std::ptrdiff_t>(index));
            write();
            command_editing_.clear();
            menu_selected_ = -1;
            main_.defer([this] { rebuild_if_changed(true); });
        })->name = "CommandDelete";
        f::button(*buttons, L"Done", [this] {
            command_editing_.clear();
            main_.defer([this] { rebuild_if_changed(true); });
        })->name = "CommandDone";
    }

    // -- a command of your own
    if (command_editor_) {
        f::gap(p, 6);
        f::text(p, L"New command", tok::TextSecondary, 14.0f, 600);
        auto* card = p.emplace<StackPanel>(Orientation::Vertical, 8.0f);
        card->name = "CommandEditor";
        card->pad = 12;
        card->style.bg = tok::SurfaceRaised;
        card->style.border = tok::BorderSubtle;
        auto field = [&](const wchar_t* label, const wchar_t* hint, const char* id) {
            auto* r = f::row(*card);
            f::row_label(*r, label, 130);
            auto* e = f::field(*r, L"", 360, nullptr, hint);
            e->name = id;
            return e;
        };
        auto* name_box = field(L"Name", L"What the menu says", "CommandName");
        auto* command = field(L"Program", L"C:\\Tools\\diff.exe", "CommandProgram");
        auto* args = field(L"Arguments", L"{path}", "CommandArguments");
        auto* switches = f::row(*card, 16);
        auto* files = f::check(*switches, L"Show for files", true, nullptr);
        auto* dirs = f::check(*switches, L"Show for folders", false, nullptr);
        auto* admin = f::check(*switches, L"Run as administrator", false, nullptr);
        f::note(*card, L"{path} is the item's full path, {name} its file name, {dir} the folder it is in. The command is offered in the menu once saved.");
        auto* buttons = f::row(*card);
        f::button(*buttons, L"Save", [this, name_box, command, args, files, dirs, admin, model, &main] {
            // Said, not ignored. Spec: SettingsViewModel.CommandProblem.
            if (is_blank(name_box->text()) || is_blank(command->text())) {
                refused_ = L"Every command needs a name and a program.";
                show_problem(L"");
                return;
            }
            std::string cmds = "[";
            for (size_t i = 0; i < model->page["commands"].size(); ++i) {
                const json::Value& c = model->page["commands"][i];
                cmds += "{\"id\":" + ops::q(core::wide(c["id"].str())) + ",\"name\":" + ops::q(core::wide(c["name"].str())) +
                        ",\"command\":" + ops::q(core::wide(c["command"].str())) + ",\"arguments\":" + ops::q(core::wide(c["arguments"].str())) +
                        ",\"showInMenu\":" + (c["showInMenu"].boolean() ? "true" : "false") + ",\"appliesToFiles\":" +
                        (c["appliesToFiles"].boolean() ? "true" : "false") + ",\"appliesToDirectories\":" +
                        (c["appliesToDirectories"].boolean() ? "true" : "false") + ",\"runElevated\":" + (c["runElevated"].boolean() ? "true" : "false") + "},";
            }
            cmds += "{\"name\":" + ops::q(name_box->text()) + ",\"command\":" + ops::q(command->text()) + ",\"arguments\":" + ops::q(args->text()) +
                    ",\"showInMenu\":true,\"appliesToFiles\":" + (files->checked() ? "true" : "false") + ",\"appliesToDirectories\":" +
                    (dirs->checked() ? "true" : "false") + ",\"runElevated\":" + (admin->checked() ? "true" : "false") + "}]";
            bb_settings_set(main.core().app(), core::s(L"customCommands"), core::s(core::wide(cmds)));
            command_editor_ = false;
            rebuild_if_changed(true);
        })->name = "CommandSave";
        f::button(*buttons, L"Cancel", [this] {
            command_editor_ = false;
            rebuild_if_changed(true);
        })->name = "CommandCancel";
    }
}

}  // namespace bb::views
