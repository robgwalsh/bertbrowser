#include "views/sidebar.h"

#include "gfx/icons.gen.h"
#include "views/main_view.h"

namespace bb::views {

using namespace bb::ui;
namespace f = forms;

SavedSidebar::SavedSidebar(MainView& main, core::Core& core) : StackPanel(Orientation::Vertical, 0.0f), main_(main), core_(core) {
    name = "SavedSidebar";
    pad = 4;
    style.bg = tok::SurfaceBackground;
    auto make = [&](TextBlock*& title, f::ItemList*& list, const char* list_name) {
        title = f::text(*this, L"", tok::TextMuted, 11.5f, 600);
        title->name = std::string(list_name) + "Title";
        list = emplace<f::ItemList>();
        list->name = list_name;
        list->row_height = 24;
        list->style.bg = gfx::kNoToken;
        list->style.border = gfx::kNoToken;
        list->flags.tab_stop = false;
    };
    make(search_title_, search_list_, "SavedSearchesList");
    // WORKSPACES, with "+" beside it: saving the panes as they are is what the section is for.
    space_head_ = emplace<StackPanel>(Orientation::Horizontal, 4.0f);
    space_title_ = f::text(*space_head_, L"", tok::TextMuted, 11.5f, 600);
    space_title_->name = "WorkspacesListTitle";
    space_title_->flex = 1;
    space_add_ = f::button(*space_head_, L"+", [this] { main_.execute_command(L"app.save-workspace"); }, 24);
    space_add_->name = "SaveWorkspaceButton";
    space_add_->tooltip = L"Save the current panes and tabs as a workspace";
    space_add_->height = 20;
    space_list_ = emplace<f::ItemList>();
    space_list_->name = "WorkspacesList";
    space_list_->row_height = 24;
    space_list_->style.bg = gfx::kNoToken;
    space_list_->style.border = gfx::kNoToken;
    space_list_->flags.tab_stop = false;
    make(bookmark_title_, bookmark_list_, "BookmarksList");
    bookmark_title_->set_text(L"BOOKMARKS");
    bookmark_list_->on_select = [this](int i) {
        if (i >= 0 && static_cast<size_t>(i) < bookmark_paths_.size()) open_bookmark(L"here", bookmark_paths_[static_cast<size_t>(i)]);
    };
    bookmark_list_->on_context = [this](int i, Point at) { bookmark_menu(i, at); };
    // Middle-click opens a bookmarked folder in a pane of its own, to the right. Spec: MainWindow (bookmark row).
    bookmark_list_->on_middle = [this](int i) {
        if (i >= 0 && static_cast<size_t>(i) < bookmark_paths_.size()) open_bookmark(L"pane-right", bookmark_paths_[static_cast<size_t>(i)]);
    };
    // A click runs it once; the second click of a double-click is a second click, as the original's mouse-up was.
    search_list_->on_select = [this](int i) {
        if (i >= 0 && static_cast<size_t>(i) < searches_.size()) run_search(searches_[static_cast<size_t>(i)], false);
    };
    search_list_->on_middle = [this](int i) {
        if (i >= 0 && static_cast<size_t>(i) < searches_.size()) run_search(searches_[static_cast<size_t>(i)], true);
    };
    search_list_->on_context = [this](int i, Point at) { search_menu(i, at); };
    space_list_->on_select = [this](int i) {
        if (i >= 0 && static_cast<size_t>(i) < workspaces_.size()) main_.pick_list_item("workspaces", L"", workspaces_[static_cast<size_t>(i)]);
    };
    space_list_->on_context = [this](int i, Point at) { workspace_menu(i, at); };
}

void SavedSidebar::run_search(const std::wstring& search, bool new_tab) {
    bb_saved_do(core_.app(), 0, core::s(new_tab ? L"run-search-new-tab" : L"run-search"), core::s(search));
}

// Run, Run in new tab, Edit..., Delete. Spec: MainWindow.SavedSearches.
void SavedSidebar::search_menu(int row, Point at) {
    if (row < 0 || static_cast<size_t>(row) >= searches_.size()) return;
    const std::wstring item = searches_[static_cast<size_t>(row)];
    MenuItems items;
    MenuItem run = MenuItem::make(L"&Run", [this, item] { run_search(item, false); });
    run.icon = &icons::Search;
    items.push_back(std::move(run));
    MenuItem tab = MenuItem::make(L"Run in new &tab", [this, item] { run_search(item, true); });
    tab.icon = &icons::OpenInNewTab;
    items.push_back(std::move(tab));
    items.push_back(MenuItem::separator());
    MainView* main = &main_;
    items.push_back(MenuItem::make(L"&Edit\u2026", [main, item] { main->edit_saved_search(item); }));
    BbApp* app = core_.app();
    MenuItem remove = MenuItem::make(L"&Delete", [app, item] { bb_saved_do(app, 0, core::s(L"remove-search"), core::s(item)); });
    remove.icon = &icons::Delete;
    items.push_back(std::move(remove));
    main_.show_context_menu(std::move(items), at);
}

// Switch to, Rename..., Delete. Spec: MainWindow.SavedWorkspaces.
void SavedSidebar::workspace_menu(int row, Point at) {
    if (row < 0 || static_cast<size_t>(row) >= workspaces_.size()) return;
    const std::wstring item = workspaces_[static_cast<size_t>(row)];
    MenuItems items;
    MainView* main = &main_;
    items.push_back(MenuItem::make(L"&Switch to", [main, item] { main->pick_list_item("workspaces", L"", item); }));
    items.push_back(MenuItem::separator());
    items.push_back(MenuItem::make(L"Re&name\u2026", [main, item] { main->rename_workspace(item); }));
    BbApp* app = core_.app();
    MenuItem remove = MenuItem::make(L"&Delete", [app, item] { bb_saved_do(app, 0, core::s(L"remove-workspace"), core::s(item)); });
    remove.icon = &icons::Delete;
    items.push_back(std::move(remove));
    main_.show_context_menu(std::move(items), at);
}

void SavedSidebar::fill(f::ItemList& list, TextBlock& title, const std::vector<std::wstring>& names, const wchar_t* heading,
                        const wchar_t* empty, bool shown, const std::vector<std::wstring>& details) {
    title.set_visible(shown);
    list.set_visible(shown);
    if (&title == space_title_ && space_head_) space_head_->set_visible(shown);
    if (!shown) return;
    title.set_text(heading);
    std::vector<f::ListEntry> entries;
    for (size_t i = 0; i < names.size(); ++i) {
        f::ListEntry e;
        e.text = names[i];
        if (i < details.size()) e.detail = details[i];
        entries.push_back(std::move(e));
    }
    if (entries.empty()) {
        f::ListEntry e;
        e.text = empty;
        e.muted = true;
        entries.push_back(std::move(e));
    }
    // Up to about eight rows, then it scrolls: a long list must not squeeze the drives out.
    list.height = std::min(200.0f, 24.0f * static_cast<float>(entries.size()) + 2);
    list.set_entries(std::move(entries));
}

void SavedSidebar::fill_bookmarks(const json::Value& rows) {
    bookmark_paths_.clear();
    std::vector<f::ListEntry> entries;
    for (size_t i = 0; i < rows.size(); ++i) {
        const json::Value& r = rows[i];
        const std::wstring path = core::wide(r["path"].str());
        const bool dir = r["isDirectory"].boolean();
        const bool hidden = r["hidden"].boolean();
        bookmark_paths_.push_back(path);
        f::ListEntry e;
        e.text = core::wide(r["name"].str());
        e.faded = hidden;
        ShellIcons* icons = &main_.shell_icons();
        const uint32_t attributes = (dir ? FILE_ATTRIBUTE_DIRECTORY : 0) | (hidden ? FILE_ATTRIBUTE_HIDDEN : 0);
        e.picture = [icons, path, dir, attributes](uint32_t px) {
            // A drive's path is its root ("C:\").
            return path.size() == 3 ? icons->drive(path, px) : icons->file(path, dir, attributes, px);
        };
        entries.push_back(std::move(e));
    }
    if (entries.empty()) {
        f::ListEntry e;
        e.text = L"Right-click a file or folder to bookmark it.";
        e.muted = true;
        entries.push_back(std::move(e));
    }
    // Sized to its rows up to ten, then it scrolls, so a long list never pushes the tree off the sidebar.
    const size_t rows_shown = std::min<size_t>(entries.size(), 10);
    bookmark_list_->height = 24.0f * static_cast<float>(rows_shown) + 2;
    bookmark_list_->set_entries(std::move(entries));
}

void SavedSidebar::open_bookmark(const std::wstring& how, const std::wstring& path) {
    bb_saved_do(core_.app(), 0, core::s(L"open-bookmark"), core::s(how + L" " + path));
}

void SavedSidebar::bookmark_menu(int row, Point at) {
    if (row < 0 || static_cast<size_t>(row) >= bookmark_paths_.size()) return;
    const std::wstring path = bookmark_paths_[static_cast<size_t>(row)];
    MenuItems items;
    MenuItem tab = MenuItem::make(L"Open in new &tab", [this, path] { open_bookmark(L"new-tab", path); });
    tab.icon = &icons::OpenInNewTab;
    items.push_back(std::move(tab));
    MenuItem pane = MenuItem::make(L"Open in new &pane");
    pane.icon = &icons::OpenInNewPane;
    pane.sub.push_back(MenuItem::make(L"To the &right", [this, path] { open_bookmark(L"pane-right", path); }));
    pane.sub.push_back(MenuItem::make(L"&Below", [this, path] { open_bookmark(L"pane-below", path); }));
    items.push_back(std::move(pane));
    MenuItem open = MenuItem::make(L"&Open", [this, path] { open_bookmark(L"here", path); });
    open.icon = &icons::Open;
    items.push_back(std::move(open));
    items.push_back(MenuItem::separator());
    BbApp* app = core_.app();
    MenuItem remove = MenuItem::make(L"&Remove bookmark", [app, path] {
        bb_saved_do(app, 0, core::s(L"toggle-bookmark"), core::s(path));
    });
    remove.icon = &icons::Delete;
    items.push_back(std::move(remove));
    main_.show_context_menu(std::move(items), at);
}

void SavedSidebar::sync() {
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_saved_state(core_.app(), o); });
    if (st.is_null()) return;
    searches_.clear();
    workspaces_.clear();
    std::vector<std::wstring> scopes;
    std::string sig = st["savedSearchesPlacement"].str() + "|" + st["workspacesPlacement"].str();
    for (size_t i = 0; i < st["searches"].size(); ++i) {
        searches_.push_back(core::wide(st["searches"][i]["name"].str()));
        scopes.push_back(core::wide(st["searches"][i]["scopeText"].str()));
        sig += "/" + st["searches"][i]["name"].str() + "=" + st["searches"][i]["scopeText"].str();
    }
    sig += "|";
    for (size_t i = 0; i < st["workspaces"].size(); ++i) {
        workspaces_.push_back(core::wide(st["workspaces"][i]["name"].str()));
        sig += "/" + st["workspaces"][i]["name"].str();
    }
    sig += "|";
    for (size_t i = 0; i < st["bookmarks"].size(); ++i) {
        const json::Value& b = st["bookmarks"][i];
        sig += "/" + b["path"].str() + (b["hidden"].boolean() ? "*" : "");
    }
    if (sig == signature_) return;
    signature_ = sig;
    fill(*search_list_, *search_title_, searches_, L"SAVED SEARCHES", L"Save a search from the search box to keep it here.",
         st["savedSearchesPlacement"].str() == "sidebar", scopes);
    fill(*space_list_, *space_title_, workspaces_, L"WORKSPACES", L"Save the current panes and tabs here, to switch back to them by name.",
         st["workspacesPlacement"].str() == "sidebar");
    fill_bookmarks(st["bookmarks"]);
    request_layout();
}

}  // namespace bb::views
