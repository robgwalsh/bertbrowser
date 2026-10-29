// The Context menu page: one list of rows - the app's entries, other programs', the user's own commands,
// separators and the `@more` marker where anything unplaced lands - and what is in the list *is* the
// membership: saving writes the layout, the two hidden lists and each command's "show in menu" from it, so no
// arrangement can un-hide anything on its own. Spec: SettingsViewModel.ContextMenu.cs, SettingsView.xaml.
#include "views/main_view.h"
#include "views/ops.h"
#include "views/settings_view.h"

#include <algorithm>
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
    std::wstring name, detail;
    const IconData* icon = nullptr;
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

Row row_for_built_in(const json::Value& b) {
    Row r;
    r.token = b["token"].str();
    r.name = core::wide(b["name"].str());
    r.detail = core::wide(b["detail"].str());
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
            r = row_for_built_in(*it->second);
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
    std::string cmds = "[";
    bool changed = false;
    for (size_t i = 0; i < m.page["commands"].size(); ++i) {
        const json::Value& c = m.page["commands"][i];
        const bool in_menu = has_row(rows, "cmd:" + c["id"].str());
        changed = changed || in_menu != c["showInMenu"].boolean();
        cmds += std::string(i ? "," : "") + "{\"id\":" + ops::q(core::wide(c["id"].str())) + ",\"name\":" + ops::q(core::wide(c["name"].str())) +
                ",\"command\":" + ops::q(core::wide(c["command"].str())) + ",\"arguments\":" + ops::q(core::wide(c["arguments"].str())) +
                ",\"showInMenu\":" + (in_menu ? "true" : "false") + ",\"appliesToFiles\":" + (c["appliesToFiles"].boolean() ? "true" : "false") +
                ",\"appliesToDirectories\":" + (c["appliesToDirectories"].boolean() ? "true" : "false") +
                ",\"runElevated\":" + (c["runElevated"].boolean() ? "true" : "false") + "}";
    }
    if (changed) set("customCommands", cmds + "]");
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
            e.detail = r.detail;
            e.icon = r.icon;
        }
        entries.push_back(std::move(e));
    }
    if (menu_selected_ >= static_cast<int>(entries.size())) menu_selected_ = -1;
    list->set_entries(std::move(entries), menu_selected_);
    list->on_select = [this](int i) { menu_selected_ = i; };

    auto commit = [this, model, &main](std::vector<Row> rows, int select) {
        menu_selected_ = select;
        apply_rows(*this, main, *model, rows);
        rebuild_if_changed(true);
    };
    auto* tools = f::row(*left, 6);
    f::button(*tools, L"Move up", [this, model, commit] {
        const int at = menu_selected_;
        if (at <= 0 || at >= static_cast<int>(model->rows.size())) return;
        std::vector<Row> rows = model->rows;
        std::swap(rows[static_cast<size_t>(at)], rows[static_cast<size_t>(at) - 1]);
        commit(std::move(rows), at - 1);
    })->name = "MenuMoveUp";
    f::button(*tools, L"Move down", [this, model, commit] {
        const int at = menu_selected_;
        if (at < 0 || at + 1 >= static_cast<int>(model->rows.size())) return;
        std::vector<Row> rows = model->rows;
        std::swap(rows[static_cast<size_t>(at)], rows[static_cast<size_t>(at) + 1]);
        commit(std::move(rows), at + 1);
    })->name = "MenuMoveDown";
    f::button(*tools, L"Remove", [this, model, commit] {
        const int at = menu_selected_;
        if (at < 0 || at >= static_cast<int>(model->rows.size()) || model->rows[static_cast<size_t>(at)].more()) return;
        std::vector<Row> rows = model->rows;
        rows.erase(rows.begin() + at);
        commit(std::move(rows), -1);
    })->name = "MenuRemove";
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

    if (dragging_pose_) {
        // The line a drag leaves between two rows, placed rather than dragged.
        auto* line = left->emplace<Widget>();
        line->height = 2;
        line->style.bg = tok::AccentBackground;
        line->name = "MenuInsertionLine";
    }

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
    auto choice = [&](const std::string& token, const std::wstring& label, const std::wstring& detail) {
        f::ListEntry e;
        e.text = std::wstring(has_row(model->rows, token) ? L"\u2611  " : L"\u2610  ") + label;
        e.detail = detail;
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
        for (const auto& e : model->catalog) choice(e.id, core::wide(e.name), shell_detail(&e));
    }
    if (model->page["commands"].size() > 0) {
        heading(L"Your commands");
        for (size_t i = 0; i < model->page["commands"].size(); ++i) {
            const json::Value& c = model->page["commands"][i];
            choice("cmd:" + c["id"].str(), core::wide(c["name"].str()), L"");
        }
    }
    choices->set_entries(std::move(ce));
    choices->on_activate = [this, model, commit, tokens](int i) {
        if (i < 0 || static_cast<size_t>(i) >= tokens.size() || tokens[static_cast<size_t>(i)].empty()) return;
        const std::string& token = tokens[static_cast<size_t>(i)];
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
        auto* files = f::check(*switches, L"Files", true, nullptr);
        auto* dirs = f::check(*switches, L"Folders", false, nullptr);
        auto* admin = f::check(*switches, L"Run as administrator", false, nullptr);
        f::note(*card, L"{path} is the item's full path, {name} its file name, {dir} the folder it is in. The command is offered in the menu once saved.");
        auto* buttons = f::row(*card);
        f::button(*buttons, L"Save", [this, name_box, command, args, files, dirs, admin, model, &main] {
            if (name_box->text().empty() || command->text().empty()) return;
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
