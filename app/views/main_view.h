// The main window's view: a widget tree over the FFI. Events from the Rust sink are PostMessage'd to the
// window and drained on the UI thread; every view reads the core on demand (a listing snapshot, a tab's
// scalars) and never keeps a second copy of its truth.
#pragma once
#include "platform/window.h"
#include "ui/controls.h"
#include "ui/edit.h"
#include "ui/layout.h"
#include "ui/list_view.h"
#include "ui/menu_bar.h"
#include "ui/scroll.h"
#include "ui/tile_view.h"
#include "ui/tree_view.h"
#include "views/chrome.h"
#include "views/command_ui.h"
#include "views/dialogs.h"
#include "views/file_drag.h"
#include "views/palette_view.h"
#include "views/shell_menu.h"
#include "views/settings_view.h"
#include "views/metadata_pane.h"
#include "views/preview_pane.h"
#include "views/thumbs.h"
#include "views/shell_icons.h"
#include "views/sidebar.h"
#include "views/core.h"
#include "views/json.h"

#include "ui/inputs.h"

#include <atomic>
#include <memory>
#include <set>
#include <unordered_map>

namespace bb::views {

constexpr UINT WM_BB_EVENT = WM_APP + 1;
constexpr UINT WM_BB_BARS = WM_APP + 2;
constexpr UINT WM_BB_DEFER = WM_APP + 30;

class MainView;
class FolderTreeProvider;

struct IndexerBannerInfo {
    bool shown = false;
    std::wstring message;
    bool can_start = false;
};

// One selected row, copied out of the listing so a dialog does not hold the snapshot.
struct SelectedRow {
    std::wstring name, path;
    bool dir = false;
    int64_t size = -1, modified = 0, created = 0;
};

// The duplicate finder's last result and which copies are ticked for removal.
struct DuplicatesState {
    bool has = false;
    json::Value result;
    std::set<std::pair<size_t, size_t>> ticked;  // group, file
};

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
    ui::ThumbPtr row_image(size_t row, uint32_t px, bool* faded) const override;
    const ui::IconData* row_icon(size_t row) const override;
    ui::Token row_tint(size_t row) const override;
    ShellIcons* icons = nullptr;
    std::wstring row_path(size_t row) const;
    bool selected(size_t row) const override { return row < sel_.size() && sel_[row]; }
    void select(ui::SelectOp op, size_t first, size_t last) override;

private:
    core::Core& core_;
    uint64_t tab_;
    std::shared_ptr<core::Listing> listing_;
    std::vector<char> sel_;
    std::vector<int> cells_;
    // Whether the rows are real files a program's or shortcut's own icon can be read from - not the inside of
    // an archive, whose paths only look real. Asked of the disk once per place.
    std::wstring checked_path_;
    bool real_paths_ = true;
};

// One tab: navigation row, path, search, the details list, the message over an empty folder.
class TabView : public ui::DockPanel {
public:
    TabView(MainView& main, core::Core& core, uint64_t tab);
    ~TabView() override;
    void arrange(ui::Rect r) override;

    uint64_t tab() const { return tab_; }
    ui::ListView* list() const { return list_; }
    ui::TileView* tiles() const { return tiles_; }
    bool tiles_mode() const { return tiles_on_; }
    ListAdapter& adapter() { return adapter_; }
    ui::Edit* search_box() const { return search_; }
    ui::Edit* global_search_box() const { return gsearch_; }

    void reload_listing();  // a new snapshot (and its columns)
    void reload_state();    // path, history, search text, selection, sort, messages
    void reload_columns();  // the column set moved
    // Types into a search box exactly as a person would: the text lands in the box and the core hears it.
    void type_search(const std::wstring& text, bool global);
    void begin_path_edit();

    // What the list is showing: column ids in order, and how many rows have a container.
    std::vector<std::wstring> column_ids() const;
    // What the cell of a row under a column reads, as the list would draw it.
    std::wstring cell_by_id(size_t row, const std::wstring& column_id) const;
    size_t realized_rows() const;
    void scroll_to_offset(float y);
    void reveal_row(size_t row);
    // The checklist a right-click on a column header opens, and the one a right-click on a file opens.
    ui::MenuItems build_column_menu();
    ui::MenuItems build_files_menu(bool background);
    // Whether a right-click on the header strip past the last column would open the column menu.
    bool header_opens_column_menu() const { return static_cast<bool>(list_->on_header_menu); }
    void refresh_tooltips();
    ui::Button* side_button(int which) const { return side_buttons_[which]; }
    void drain_thumbnails();
    size_t thumbnails_pending() const { return thumbs_ ? thumbs_->pending() : 0; }
    void set_thumbnail_source(ThumbSource s);
    const std::wstring& path_text() const { return path_text_; }

    // ---- the side pane ----
    // Null until the pane has been shown once: both are built on first show, not with the tab.
    PreviewPane* preview() const { return preview_; }
    MetadataPane* metadata() const { return metadata_; }
    PreviewPane* ensure_preview();
    MetadataPane* ensure_metadata();
    // Widens the side pane to fit what the preview holds, or as far as the tab allows.
    void fit_side_width();
    // Selects what plays next in the list, and plays it when it arrives.
    void select_next_media();
    bool side_pending() const { return preview_ && preview_->pending(); }

private:
    void end_path_edit();
    void sync_columns();
    void sync_view_mode(const BbTabState& st);
    void sync_side_pane(const BbTabState& st);
    void on_search_changed(bool global);
    void open_context(ui::Point p, bool header, bool background = false);
    ui::Widget* active_view() const;

    MainView& main_;
    core::Core& core_;
    uint64_t tab_;
    ListAdapter adapter_;
    ui::StackPanel* nav_ = nullptr;
    ui::Button *back_ = nullptr, *forward_ = nullptr, *up_ = nullptr, *refresh_ = nullptr;
    ui::Button* side_buttons_[3] = {nullptr, nullptr, nullptr};
    ui::Panel* path_bar_ = nullptr;
    BreadcrumbBar* crumbs_ = nullptr;
    PathEdit* path_ = nullptr;
    ui::Edit *search_ = nullptr, *gsearch_ = nullptr;
    ui::Widget* banner_panel_ = nullptr;
    ui::TextBlock* banner_ = nullptr;
    ui::Button* unlock_ = nullptr;
    ui::DockPanel* body_ = nullptr;
    ui::Panel* view_area_ = nullptr;
    ui::ListView* list_ = nullptr;
    ui::TileView* tiles_ = nullptr;
    std::unique_ptr<ThumbnailService> thumbs_;
    ThumbSource thumb_source_ = ThumbSource::Shell;
    ui::DockPanel* side_pane_ = nullptr;
    PreviewPane* preview_ = nullptr;
    MetadataPane* metadata_ = nullptr;
    int side_mode_ = 0;
    float side_width_ = 360.0f;
    bool tiles_on_ = false;
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
    // As run_command, but a command that is the host's to carry out is carried out here. `reason` says why
    // a command that could not run could not; `ui` is set when the host did it.
    bool execute_command(const std::wstring& id, std::wstring* reason = nullptr, bool* ui = nullptr);
    void activate_tab(uint64_t tab);
    void close_tab(uint64_t tab);
    void activate_pane(uint64_t pane);

    // ---- menu bar and toolbars (the core's bars model drawn) ----
    ui::MenuBar* menu_bar() const { return menu_bar_; }
    TitleBar* title_bar() const { return title_; }
    CommandToolbar* title_toolbar() const { return title_toolbar_; }
    const std::vector<CommandToolbar*>& row_toolbars() const { return row_toolbars_; }
    // Re-reads the menu bar's headings and every toolbar now (they are otherwise refreshed once the
    // events that change them have been handled).
    void refresh_bars();
    std::vector<std::wstring> menu_bar_captions() const;
    // The rows a click on a heading would show, built exactly as the dropdown builds them. Empty when the
    // bar has no such menu.
    ui::MenuItems build_main_menu(const std::wstring& name, bool* found = nullptr);
    MenuBuildEnv menu_env();
    // The flags the core wants beside a key: what overlay is up.
    uint32_t focus_flags();
    BbFocusKind focus_kind() const;
    // Carries out a command that is the host's to carry out. false when it wants a person (a dialog) and was not.
    bool host_command(const std::wstring& id);
    // A context menu by kind (files, background, columns) as it would be built now; false for an unknown kind.
    bool build_named_menu(const std::wstring& kind, ui::MenuItems* out);
    SettingsView* settings() const { return settings_; }
    PaletteView* palette() const { return palette_; }
    SavedSidebar* sidebar() const { return sidebar_; }
    // ---- what dialogs and strips read ----
    std::vector<SelectedRow> selected_rows();
    // The queue and the run in flight as the core describes them (`bb_ops_query("queue")`): what the
    // strip and the Transfers window draw. Re-read on every `Model::Transfer` event.
    const json::Value& queue_model() const { return queue_; }
    void sync_transfer();
    std::wstring transfer_text() const;
    // An open Transfers window refreshes itself through this; empty when none is open.
    std::function<void()> on_transfer_changed;
    // The same for an open History window.
    std::function<void()> on_undo_changed;
    // A question the core is waiting on (a clash, an offer of administrator rights) puts its dialog up.
    void sync_questions(bool conflicts);
    DuplicatesState& duplicates() { return duplicates_; }
    // Tells the core which shell properties this machine has, once (the Add column list reads them).
    void ensure_machine_properties();
    // Detached photograph of a dialog (null for an unknown kind).
    std::unique_ptr<gfx::BitmapSurface> render_dialog(const std::wstring& kind);
    void close_dialog();
    bool dialog_showing() const { return dialog_open_; }
    // Tells the core what Windows looks like now (light or dark, high contrast); the window does it at
    // start and whenever Windows says it changed. The harness poses the answer instead (`system-theme`).
    void push_system_appearance();
    void set_follow_windows(bool on) { follow_windows_ = on; }
    // The harness never opens a dialog nobody could answer: a command that would is reported instead.
    void set_headless_dialogs(bool on) { headless_dialogs_ = on; }
    // Poses the insertion line on the active pane's tab strip (see TabStrip::set_drag_gap). false if no such gap.
    bool pose_tab_drag(int gap);
    // The search index offer and line, as the window shows them.
    IndexerBannerInfo indexer_banner() const;
    std::wstring indexing_status() const;
    bool indexing_can_retry() const;
    // Brings every overlay (Settings, palette) in step with the core's models now.
    void sync_overlays();
    // Opens a dialog by kind in the window's dialog layer (modal until closed).
    void open_dialog(const std::wstring& kind);
    // Opens a menu at a point (a right click), keeping its items alive while it is up.
    void show_context_menu(ui::MenuItems items, ui::Point at);
    // Where other programs' menu entries come from: the product's registry-and-COM source until a harness
    // poses a canned one. Null source = no extensions.
    ShellMenuSource* shell_source();
    // Drag and drop over OLE: the window becomes a drop target and the list can start a drag. The product turns
    // it on; a scripted run has no pointer to drag with and never does.
    void enable_file_drag_drop();
    FileDrag* file_drag() const { return file_drag_.get(); }
    void set_shell_source(std::unique_ptr<ShellMenuSource> source) { shell_source_ = std::move(source); }
    // The shell session behind the menu just built; dropped after that menu closes (deferred), because the
    // click that invokes an entry has to have run first.
    void keep_menu_session(std::shared_ptr<void> session) { menu_session_ = std::move(session); }
    // Runs `fn` on the UI thread after the current message.
    void defer(std::function<void()> fn);
    // Thumbnails asked for and not yet drawn, across every tab (the harness waits for them to be none).
    size_t thumbnails_pending() const;
    // File, folder and drive icons, shared by every list and the folder tree.
    ShellIcons& shell_icons() { return *icons_; }
    size_t icons_pending() const { return icons_->pending(); }
    // A preview scheduled or being read in any tab (the harness waits for it to land).
    bool preview_pending() const;
    // Where tile thumbnails come from: the shell (the product) or a generator (the harness).
    void set_thumbnail_source(ThumbSource s);
    ThumbSource thumbnail_source() const { return thumb_source_; }
    // Re-reads the keymap into the tooltips that name a key.
    void refresh_tooltips();
    // A list row (a drive, a workspace, a saved search) was picked from a menu or toolbar.
    void pick_list_item(const std::string& list_id, const std::wstring& id, const std::wstring& name);

private:
    bool on_window_message(UINT msg, WPARAM wp, LPARAM lp);
    bool on_preview_key(const ui::KeyEvent& e);
    void supply_properties(uint64_t tab);
    void deliver_preview(uint64_t tab, PreviewResult* result);
    void sync_index();
    void open_toolbar_list(const BarRow& row, ui::Rect anchor);
    void open_overflow(const std::vector<BarRow>& rows, ui::Rect anchor);
    void open_step_menu(const BarRow& row, ui::Rect anchor);
    void sync_row_toolbars(const json::Value& state);
    void handle(const BbEvent& e);
    void rebuild_layout(const json::Value& state);
    std::unique_ptr<ui::Widget> build_node(const json::Value& node, const json::Value& panes);
    void refresh_status();
    static void sink(void* ctx, BbEvent* evt);

    core::Core& core_;
    platform::Window& window_;
    std::atomic<HWND> hwnd_{nullptr};
    TitleBar* title_ = nullptr;
    ui::MenuBar* menu_bar_ = nullptr;
    CommandToolbar* title_toolbar_ = nullptr;
    ui::StackPanel* rows_panel_ = nullptr;
    std::vector<CommandToolbar*> row_toolbars_;
    ui::Panel* browser_ = nullptr;
    ui::PopupLayer* popup_ = nullptr;
    SettingsView* settings_ = nullptr;
    PaletteView* palette_ = nullptr;
    SavedSidebar* sidebar_ = nullptr;
    std::string menu_sig_;
    bool bars_pending_ = false;
    bool headless_dialogs_ = false;
    bool follow_windows_ = true;
    std::unique_ptr<ShellMenuSource> shell_source_;
    std::unique_ptr<FileDrag> file_drag_;
    std::shared_ptr<void> menu_session_;
    std::vector<std::function<void()>> deferred_;
    json::Value queue_;
    ui::StackPanel* transfer_strip_ = nullptr;
    ui::TextBlock *transfer_head_ = nullptr, *transfer_detail_ = nullptr;
    ui::ProgressBar* transfer_bar_ = nullptr;
    ui::Button *transfer_pause_ = nullptr, *transfer_cancel_ = nullptr;
    DuplicatesState duplicates_;
    bool machine_props_set_ = false;
    DialogLayer* dialogs_ = nullptr;
    ui::StackPanel* indexer_banner_ = nullptr;
    ui::TextBlock* indexer_text_ = nullptr;
    json::Value index_state_;
    ThumbSource thumb_source_ = ThumbSource::Shell;
    bool dialog_open_ = false;
    std::wstring dialog_kind_;
    std::unique_ptr<ui::MenuItems> floating_menu_;
    StatusBar* status_ = nullptr;
    ui::Splitter* main_split_ = nullptr;
    ui::TreeView* tree_ = nullptr;
    std::unique_ptr<ShellIcons> icons_;
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
