// The sidebar's saved lists under the folder tree: saved searches and workspaces, each offered where the
// settings say (the sidebar, the title bar or nowhere), then the bookmarks. Which rows there are, and where,
// is the core's.
#pragma once
#include "views/forms.h"
#include "views/model.h"

namespace bb::views {

class MainView;

class SavedSidebar : public ui::StackPanel {
public:
    SavedSidebar(MainView& main, core::Core& core);
    void sync();
    // Names the sidebar lists now (harness and tests).
    const std::vector<std::wstring>& searches() const { return searches_; }
    const std::vector<std::wstring>& workspaces() const { return workspaces_; }
    const std::vector<std::wstring>& bookmarks() const { return bookmark_paths_; }

private:
    void fill(forms::ItemList& list, ui::TextBlock& title, const std::vector<std::wstring>& names, const wchar_t* heading,
              const wchar_t* empty, bool shown);
    void fill_bookmarks(const json::Value& rows);
    void open_bookmark(const std::wstring& how, const std::wstring& path);
    void bookmark_menu(int row, ui::Point at);
    MainView& main_;
    core::Core& core_;
    ui::TextBlock *search_title_ = nullptr, *space_title_ = nullptr, *bookmark_title_ = nullptr;
    forms::ItemList *search_list_ = nullptr, *space_list_ = nullptr, *bookmark_list_ = nullptr;
    std::vector<std::wstring> searches_, workspaces_, bookmark_paths_;
    std::string signature_;
};

}  // namespace bb::views
