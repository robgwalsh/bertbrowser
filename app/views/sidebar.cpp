#include "views/sidebar.h"

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
    if (sig == signature_) return;
    signature_ = sig;
    fill(*search_list_, *search_title_, searches_, L"SAVED SEARCHES", L"No saved searches", st["savedSearchesPlacement"].str() == "sidebar");
    fill(*space_list_, *space_title_, workspaces_, L"WORKSPACES", L"No saved workspaces", st["workspacesPlacement"].str() == "sidebar");
    const bool any = search_list_->flags.visible || space_list_->flags.visible;
    set_visible(any);
    request_layout();
}

}  // namespace bb::views
