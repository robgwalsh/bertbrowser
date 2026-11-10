#include "views/tools/tool_dialogs.h"
#include "views/tools/tool_state.h"
#include "views/main/main_view.h"

#include "gfx/icons.gen.h"
#include "views/main/folder_tree.h"
#include "views/panes/media_pane.h"
#include "views/bridge/ops.h"
#include "views/dialogs/ops_card.h"
#include "views/dialogs/ops_menus.h"
#include "views/panes/property_host.h"
#include "views/shell/shell_props.h"
#include "views/tools/tool_widgets.h"

#include <dwmapi.h>
#include <shobjidl.h>

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace bb::ui;

// ---- the menu bar and the toolbars ----------------------------------------------------------------------

MenuBuildEnv MainView::menu_env() {
    MenuBuildEnv env;
    env.core = &core_;
    env.run = [this](const std::wstring& id) { execute_command(id); };
    env.pick = [this](const std::string& list, const std::wstring& id, const std::wstring& name) { pick_list_item(list, id, name); };
    return env;
}

void MainView::pick_list_item(const std::string& list_id, const std::wstring& id, const std::wstring& name) {
    BbApp* app = core_.app();
    try {
        if (list_id == "drives" || list_id == "bookmarks") {
            if (active_tab_) core_.navigate(active_tab_, id);
        } else if (list_id == "workspaces") {
            core::check(bb_saved_do(app, 0, core::s(L"switch-workspace"), core::s(name)), "switch-workspace");
        } else if (list_id == "saved-searches") {
            core::check(bb_saved_do(app, active_tab_, core::s(L"run-search"), core::s(name)), "run-search");
        } else if (list_id == "command-bars") {
            // Asks first when the arrangement in force is saved under no name. Spec: MainWindow.CommandBars (ConfirmDiscardCommandBars).
            core::check(bb_barpage_do(app, core::s(L"apply-saved"), core::s(name)), "apply");
        } else if (list_id == "new-file-types") {
            // The New dialog for that type, in the active folder. Spec: MainWindow.CommandBars (NewFileTypeRows: RunNew).
            open_new_file(id, name, L"");
        } else if (list_id == "custom-commands") {
            // One of your commands is an ordinary command, through the same gate.
            std::wstring reason;
            if (!execute_command(id, &reason) && !reason.empty()) bb_status_set(app, core::s(reason));
        } else if (list_id == "undo-steps" || list_id == "redo-steps") {
            // Back (or forward) to that step, as the title bar's split buttons do.
            const int steps = _wtoi(id.c_str());
            uint64_t op = 0;
            if (steps > 0) bb_undo_step(app, list_id == "undo-steps" ? steps : -steps, &op);
        }
    } catch (const core::CoreError&) {
    }
}

std::vector<std::wstring> MainView::menu_bar_captions() const {
    std::vector<std::wstring> out;
    for (const MenuBarEntry& m : menu_bar_->menus()) out.push_back(plain_caption(m.text));
    return out;
}

MenuItems MainView::build_main_menu(const std::wstring& name, bool* found) {
    if (found) *found = false;
    const json::Value menu = try_model([&](BbOwnedStr* o) {
        return bb_bars_query(core_.app(), core::s(L"menu"), core::s(name), o);
    });
    if (menu.is_null()) return {};
    if (found) *found = true;
    return build_menu_items(menu_env(), read_bar_rows(menu["rows"]));
}

void MainView::refresh_bars() {
    BbApp* app = core_.app();
    // The headings: rebuilt only when the arrangement changed, since a dropdown holds its items by pointer.
    const json::Value bar = try_model([&](BbOwnedStr* o) { return bb_bars_query(app, core::s(L"menubar"), core::s(L""), o); });
    std::string sig;
    for (size_t i = 0; i < bar.size(); ++i) sig += bar[i]["caption"].str() + "|" + bar[i]["name"].str() + ";";
    if (sig != menu_sig_ && !bar.is_null()) {
        menu_sig_ = sig;
        std::vector<MenuBarEntry> entries;
        for (size_t i = 0; i < bar.size(); ++i) {
            MenuBarEntry e;
            e.text = core::wide(bar[i]["caption"].str());
            const std::wstring name = core::wide(bar[i]["name"].str());
            e.on_opening = [this, name](MenuItems& items) { items = build_main_menu(name); };
            entries.push_back(std::move(e));
        }
        menu_bar_->set_menus(std::move(entries));
        title_->request_layout();
    }
    const json::Value title = try_model([&](BbOwnedStr* o) { return bb_bars_query(app, core::s(L"toolbar"), core::s(L"title"), o); });
    if (!title.is_null()) title_toolbar_->update(title);
    sync_row_toolbars(try_model([&](BbOwnedStr* o) { return bb_bars_query(app, core::s(L"state"), core::s(L""), o); }));
}

void MainView::sync_row_toolbars(const json::Value& state) {
    const size_t want = state["rows"].size();
    if (want != row_toolbars_.size()) {
        while (row_toolbars_.size() > want) {
            rows_panel_->remove_child(row_toolbars_.back());
            row_toolbars_.pop_back();
        }
        while (row_toolbars_.size() < want) {
            auto* tb = rows_panel_->emplace<CommandToolbar>("row" + std::to_string(row_toolbars_.size() + 1));
            tb->name = "ToolbarRow" + std::to_string(row_toolbars_.size() + 1);
            tb->on_activate = [this](const BarRow& r, Rect anchor) {
                if (r.kind == "list" || r.kind == "placeholder") open_toolbar_list(r, anchor);
                else if (r.kind == "menu") open_overflow(r.children, anchor);
                else execute_command(core::wide(r.id));
            };
            tb->on_overflow = [this](const std::vector<BarRow>& rows, Rect anchor) { open_overflow(rows, anchor); };
            tb->on_context = [this](const BarRow& r, Rect anchor) { open_step_menu(r, anchor); };
            row_toolbars_.push_back(tb);
        }
        rows_panel_->height = static_cast<float>(want) * CommandToolbar::bar_height();
        rows_panel_->set_visible(want > 0);
        browser_->request_layout();
    }
    for (size_t i = 0; i < row_toolbars_.size(); ++i) {
        const json::Value m = try_model([&](BbOwnedStr* o) {
            return bb_bars_query(core_.app(), core::s(L"toolbar"), core::s(L"row" + std::to_wstring(i + 1)), o);
        });
        if (!m.is_null()) row_toolbars_[i]->update(m);
    }
}

void MainView::set_title_search_text(const std::wstring& text) {
    if (!title_search_ || title_search_->text() == text) return;
    title_search_suppress_ = true;
    title_search_->set_text(text);
    title_search_suppress_ = false;
    title_search_->sync_buttons();
}

void MainView::sync_title_search() {
    if (!title_search_ || !active_tab_ || title_search_->focused()) return;
    try {
        const BbTabState st = core_.tab_state(active_tab_);
        set_title_search_text(st.is_global_search ? core_.tab_text(active_tab_, BB_TEXT_SEARCH) : std::wstring());
    } catch (const core::CoreError&) {
    }
}

void MainView::sync_switchers() {
    if (!switchers_) return;
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_saved_state(core_.app(), o); });
    if (st.is_null()) return;
    const bool spaces = st["workspacesPlacement"].str() == "titlebar";
    const bool searches = st["savedSearchesPlacement"].str() == "titlebar";
    // Named after the workspace the window last came from, or plain "Workspaces" before there is one.
    const std::wstring current = core::wide(st["currentWorkspace"].str());
    const std::wstring label = (current.empty() ? std::wstring(L"Workspaces") : current) + L"  \u25BE";
    if (workspace_switcher_->text != label) {
        workspace_switcher_->text = label;
        workspace_switcher_->request_layout();
    }
    search_switcher_->text = L"Saved searches  \u25BE";
    workspace_switcher_->set_visible(spaces);
    search_switcher_->set_visible(searches);
    if (switchers_->flags.visible != (spaces || searches)) {
        switchers_->set_visible(spaces || searches);
        title_->request_layout();
    }
    title_->arrange(title_->bounds);
    title_->invalidate();
}

// The dropdown: every workspace (or saved search), then Save and Manage. Spec: MainWindow.SavedWorkspaces
// BuildWorkspaceMenuItems, MainWindow.SavedSearches BuildSavedSearchMenuItems.
void MainView::open_switcher(bool workspaces) {
    floating_menu_ = std::make_unique<MenuItems>(switcher_items(workspaces));
    MenuOpen o;
    o.anchor = (workspaces ? workspace_switcher_ : search_switcher_)->bounds;
    popup_->open_menu(*floating_menu_, o);
}

MenuItems MainView::switcher_items(bool workspaces) {
    BarRow list;
    list.kind = "list";
    list.id = workspaces ? "workspaces" : "saved-searches";
    MenuItems items = build_menu_items(menu_env(), {list});
    if (items.empty()) {
        MenuItem none = MenuItem::make(workspaces ? L"No saved workspaces yet" : L"No saved searches yet");
        none.enabled = false;
        items.push_back(std::move(none));
    }
    items.push_back(MenuItem::separator());
    BbApp* app = core_.app();
    if (workspaces) {
        MenuItem save = MenuItem::make(L"&Save current layout\u2026", [this] { execute_command(L"app.save-workspace"); });
        save.icon = &icons::Save;
        items.push_back(std::move(save));
    } else {
        // Only with something to save: a greyed entry says why nothing would happen.
        MenuItem save = MenuItem::make(L"&Save current search\u2026", [this] { execute_command(L"search.save"); });
        save.icon = &icons::Save;
        save.enabled = active_tab_ && !core_.tab_text(active_tab_, BB_TEXT_SEARCH).empty();
        items.push_back(std::move(save));
    }
    MenuItem manage = MenuItem::make(workspaces ? L"&Manage workspaces\u2026" : L"&Manage saved searches\u2026", [app, workspaces] {
        bb_settings_page_open(app, core::s(workspaces ? L"workspaces" : L"saved-searches"));
    });
    manage.icon = &icons::Settings;
    items.push_back(std::move(manage));
    return items;
}

void MainView::open_toolbar_list(const BarRow& row, Rect anchor) {
    BarRow list = row;
    list.kind = "list";
    floating_menu_ = std::make_unique<MenuItems>(build_menu_items(menu_env(), {list}));
    if (floating_menu_->empty()) return;
    MenuOpen o;
    o.anchor = anchor;
    popup_->open_menu(*floating_menu_, o);
}

// A right click on Undo or Redo: the list of what they would take back or put again, from the nearest.
void MainView::open_step_menu(const BarRow& row, Rect anchor) {
    const bool undo = row.id == "edit.undo";
    if (!undo && row.id != "edit.redo") return;
    MenuItems items;
    if (!ops::build_step_menu(*this, undo ? L"undo" : L"redo", &items) || items.empty()) return;
    floating_menu_ = std::make_unique<MenuItems>(std::move(items));
    MenuOpen o;
    o.anchor = anchor;
    popup_->open_menu(*floating_menu_, o);
}

void MainView::open_overflow(const std::vector<BarRow>& rows, Rect anchor) {
    floating_menu_ = std::make_unique<MenuItems>(build_menu_items(menu_env(), rows));
    if (floating_menu_->empty()) return;
    MenuOpen o;
    o.anchor = anchor;
    popup_->open_menu(*floating_menu_, o);
}

void MainView::supply_properties(uint64_t tab) {
    // The core hands out each cell once; keep asking until it has nothing left (bounded: a long list is
    // read as it scrolls into view, not all at once).
    for (int round = 0; round < 8; ++round) {
        const json::Value req = try_model([&](BbOwnedStr* o) { return bb_props_requests(core_.app(), tab, 0, 0, o); });
        if (req.size() == 0) break;
        const std::string answer = read_property_cells(req);
        bb_props_supply(core_.app(), tab, core::s(core::wide(answer)));
    }
}


}  // namespace bb::views
