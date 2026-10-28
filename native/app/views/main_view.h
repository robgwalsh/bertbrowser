// The main window's view: a widget tree over the FFI. Events from the Rust sink are PostMessage'd to the
// window and drained on the UI thread; every view reads the core on demand (a listing snapshot, a tab's
// scalars) and never keeps a second copy of its truth.
#pragma once
#include "platform/window.h"
#include "ui/controls.h"
#include "ui/edit.h"
#include "ui/layout.h"
#include "ui/list_view.h"
#include "ui/scroll.h"
#include "ui/tree_view.h"
#include "views/chrome.h"
#include "views/core.h"
#include "views/json.h"

#include <atomic>
#include <memory>
#include <unordered_map>

namespace bb::views {

constexpr UINT WM_BB_EVENT = WM_APP + 1;

class MainView;
class FolderTreeProvider;

// Rows and selection of one tab, for the details list. Cells come from the snapshot; selection is the
// core's, mirrored here so painting a row costs an array read.
class ListAdapter : public ui::RowProvider {
public:
    ListAdapter(core::Core& core, uint64_t tab) : core_(core), tab_(tab) {}

    void reset(std::shared_ptr<core::Listing> l);
    void reload_selection();
    void set_cells(std::vector<int> cells) { cells_ = std::move(cells); }
    const std::shared_ptr<core::Listing>& listing() const { return listing_; }

    size_t row_count() const override { return listing_ ? listing_->size() : 0; }
    uint64_t row_id(size_t row) const override;
    std::wstring cell_text(size_t row, int column_id) const override;
    bool selected(size_t row) const override { return row < sel_.size() && sel_[row]; }
    void select(ui::SelectOp op, size_t first, size_t last) override;

private:
    core::Core& core_;
    uint64_t tab_;
    std::shared_ptr<core::Listing> listing_;
    std::vector<char> sel_;
    std::vector<int> cells_;
};

// One tab: navigation row, path, search, the details list, the message over an empty folder.
class TabView : public ui::DockPanel {
public:
    TabView(MainView& main, core::Core& core, uint64_t tab);

    uint64_t tab() const { return tab_; }
    ui::ListView* list() const { return list_; }
    ListAdapter& adapter() { return adapter_; }
    ui::Edit* search_box() const { return search_; }
    ui::Edit* global_search_box() const { return gsearch_; }

    void reload_listing();  // a new snapshot (and its columns)
    void reload_state();    // path, history, search text, selection, sort, messages
    // Types into a search box exactly as a person would: the text lands in the box and the core hears it.
    void type_search(const std::wstring& text, bool global);
    void begin_path_edit();

private:
    void end_path_edit();
    void sync_columns();
    void on_search_changed(bool global);

    MainView& main_;
    core::Core& core_;
    uint64_t tab_;
    ListAdapter adapter_;
    ui::StackPanel* nav_ = nullptr;
    ui::Button *back_ = nullptr, *forward_ = nullptr, *up_ = nullptr, *refresh_ = nullptr;
    ui::Panel* path_bar_ = nullptr;
    BreadcrumbBar* crumbs_ = nullptr;
    PathEdit* path_ = nullptr;
    ui::Edit *search_ = nullptr, *gsearch_ = nullptr;
    ui::Panel* banner_panel_ = nullptr;
    ui::TextBlock* banner_ = nullptr;
    ui::Panel* body_ = nullptr;
    ui::ListView* list_ = nullptr;
    Overlay* empty_ = nullptr;
    std::vector<core::ColumnInfo> columns_;
    std::wstring path_text_;
    bool suppress_ = false;
};

class PaneView : public ui::StackPanel {
public:
    PaneView(MainView& main, uint64_t pane) : ui::StackPanel(ui::Orientation::Vertical, 0), main_(main), pane_(pane) {}
    uint64_t pane() const { return pane_; }
    TabStrip* strip = nullptr;
    TabView* content = nullptr;
    bool on_mouse_down(const ui::MouseEvent&) override;

private:
    MainView& main_;
    uint64_t pane_;
};

class MainView {
public:
    MainView(core::Core& core, platform::Window& window);
    ~MainView();
    MainView(const MainView&) = delete;
    MainView& operator=(const MainView&) = delete;

    // Opens the first tab and loads everything.
    void start(const std::wstring& path);
    // Pulls the palette from the core into the window's resources.
    void apply_theme();
    // Re-reads the layout and every tab's view from the core.
    void sync_layout();
    // Drains events already posted to the window; returns how many were handled.
    size_t events_handled() const { return handled_; }

    core::Core& core() { return core_; }
    platform::Window& window() { return window_; }
    uint64_t active_tab() const { return active_tab_; }
    uint64_t active_pane() const { return active_pane_; }
    TabView* tab_view(uint64_t tab);
    TabView* active_view() { return tab_view(active_tab_); }
    size_t pane_count() const { return pane_views_.size(); }
    size_t tab_count_in_active_pane() const;
    std::vector<std::wstring> tab_titles_in_active_pane() const;
    std::vector<uint64_t> tab_ids_in_active_pane() const;
    std::vector<uint64_t> pane_ids() const;
    // Searches the active tab's widgets first, then the whole window.
    ui::Widget* find_named(const std::string& name);
    FolderTreeProvider& folder_tree() { return *tree_provider_; }
    ui::TreeView* tree_view() const { return tree_; }
    // Reveals the tab's folder in the tree without navigating.
    void reveal_in_tree(const std::wstring& path);
    // A tab navigated: the tree follows when it is the active one.
    void tab_navigated(uint64_t tab, const std::wstring& path);

    // Runs one command against the active tab through the one gate the keys and menus share.
    bool run_command(const std::wstring& id);
    void activate_tab(uint64_t tab);
    void close_tab(uint64_t tab);
    void activate_pane(uint64_t pane);

private:
    bool on_window_message(UINT msg, WPARAM wp, LPARAM lp);
    bool on_preview_key(const ui::KeyEvent& e);
    void handle(const BbEvent& e);
    void rebuild_layout(const json::Value& state);
    std::unique_ptr<ui::Widget> build_node(const json::Value& node, const json::Value& panes);
    void refresh_status();
    static void sink(void* ctx, BbEvent* evt);

    core::Core& core_;
    platform::Window& window_;
    std::atomic<HWND> hwnd_{nullptr};
    TitleBar* title_ = nullptr;
    StatusBar* status_ = nullptr;
    ui::Splitter* main_split_ = nullptr;
    ui::TreeView* tree_ = nullptr;
    std::unique_ptr<FolderTreeProvider> tree_provider_;
    ui::Panel* pane_host_ = nullptr;
    ui::Widget* layout_root_ = nullptr;
    std::string layout_sig_;
    std::unordered_map<uint64_t, std::unique_ptr<TabView>> parked_;  // tabs not on screen
    std::unordered_map<uint64_t, TabView*> by_tab_;
    std::vector<PaneView*> pane_views_;
    uint64_t active_tab_ = 0, active_pane_ = 0;
    std::wstring revealed_;
    bool revealing_ = false;
    size_t handled_ = 0;
    std::wstring status_text_;
};

}  // namespace bb::views
