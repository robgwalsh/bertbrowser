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
    make(space_title_, space_list_, "WorkspacesList");
    make(bookmark_title_, bookmark_list_, "BookmarksList");
    bookmark_title_->set_text(L"BOOKMARKS");
    bookmark_list_->on_select = [this](int i) {
        if (i >= 0 && static_cast<size_t>(i) < bookmark_paths_.size()) open_bookmark(L"here", bookmark_paths_[static_cast<size_t>(i)]);
    };
    bookmark_list_->on_context = [this](int i, Point at) { bookmark_menu(i, at); };
    search_list_->on_activate = [this](int i) {
        if (i >= 0 && static_cast<size_t>(i) < searches_.size()) main_.pick_list_item("saved-searches", L"", searches_[static_cast<size_t>(i)]);
    };
    search_list_->on_select = [this](int i) {
        if (i >= 0 && static_cast<size_t>(i) < searches_.size()) main_.pick_list_item("saved-searches", L"", searches_[static_cast<size_t>(i)]);
    };
    space_list_->on_select = [this](int i) {
        if (i >= 0 && static_cast<size_t>(i) < workspaces_.size()) main_.pick_list_item("workspaces", L"", workspaces_[static_cast<size_t>(i)]);
    };
}

void SavedSidebar::fill(f::ItemList& list, TextBlock& title, const std::vector<std::wstring>& names, const wchar_t* heading,
                        const wchar_t* empty, bool shown) {
    title.set_visible(shown);
    list.set_visible(shown);
    if (!shown) return;
    title.set_text(heading);
    std::vector<f::ListEntry> entries;
    for (const auto& n : names) {
        f::ListEntry e;
        e.text = n;
        entries.push_back(std::move(e));
    }
    if (entries.empty()) {
        f::ListEntry e;
        e.text = empty;
        e.muted = true;
        entries.push_back(std::move(e));
    }
    list.height = 24.0f * static_cast<float>(entries.size()) + 2;
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
        e.text = L"No bookmarks";
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
    std::string sig = st["savedSearchesPlacement"].str() + "|" + st["workspacesPlacement"].str();
    for (size_t i = 0; i < st["searches"].size(); ++i) {
        searches_.push_back(core::wide(st["searches"][i]["name"].str()));
        sig += "/" + st["searches"][i]["name"].str();
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
    fill(*search_list_, *search_title_, searches_, L"SAVED SEARCHES", L"No saved searches", st["savedSearchesPlacement"].str() == "sidebar");
    fill(*space_list_, *space_title_, workspaces_, L"WORKSPACES", L"No saved workspaces", st["workspacesPlacement"].str() == "sidebar");
    fill_bookmarks(st["bookmarks"]);
    request_layout();
}

}  // namespace bb::views
