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
#include "views/tool_window.h"

#include "ui/inputs.h"

#include <atomic>
#include <map>
#include <memory>
#include <optional>
#include <set>
#include <unordered_map>

namespace bb::views {

constexpr UINT WM_BB_EVENT = WM_APP + 1;
constexpr UINT WM_BB_BARS = WM_APP + 2;
constexpr UINT WM_BB_TREE = WM_APP + 6;  // the folder tree's worker finished a read
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
// The duplicate finder's window: the core's model (`duplicates.model`) as last read, and the copies ticked here,
// by position - so they go whenever the core says the groups were replaced (`version`).
// A virtual key as a chord names it ("Esc", "Del", "F5", "A"); empty for one no chord uses.
std::wstring chord_key_name(unsigned vk);

struct DuplicatesState {
    bool has = false;
    json::Value result;
    std::set<std::pair<size_t, size_t>> ticked;  // group, file
    int64_t version = -1;
    // Copies a confirmed removal is deleting: each leaves its group once it is really gone from disk, so one
    // that could not be deleted stays listed. Settled as operations finish (`duplicates_settle_prune`).
    std::vector<std::wstring> pending_prune;
};

// Rows and selection of one tab, for the details list. Cells come from the snapshot; selection is the
// core's, mirrored here so painting a row costs an array read.
class ListAdapter : public ui::RowProvider {
public:
    ListAdapter(core::Core& core, uint64_t tab) : core_(core), tab_(tab) {}

    void reset(std::shared_ptr<core::Listing> l);
    void reload_selection();
    void set_cells(std::vector<int> cells) { cells_ = std::move(cells); }
    void set_match_column(int column_id) { match_column_ = column_id; }
    void set_size_column(int column_id) { size_column_ = column_id; }
    std::wstring cell_tip(size_t row, int column_id) const override;
    bool cell_marks(size_t row, int column_id, CellMarks* out) const override;
    bool row_is_tile(size_t row) const override;
    std::wstring tile_row_text(size_t row, int part) const override;
    bool row_faded(size_t row) const override;
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
    void select_rows(const std::vector<size_t>& rows) override;

private:
    core::Core& core_;
    uint64_t tab_;
    std::shared_ptr<core::Listing> listing_;
    std::vector<char> sel_;
    int match_column_ = -1;
    int size_column_ = -1;
    std::vector<int> cells_;
    // Whether the rows are real files a program's or shortcut's own icon can be read from - not the inside of
    // an archive, whose paths only look real. Asked of the disk once per place.
    std::wstring checked_path_;
    bool real_paths_ = true;
};

// A search box with its two buttons inside it, at the right: save this search, and clear it. Both are
// there only while there is something typed. Spec: DirectoryTabView.xaml (SearchBox, SearchClearButton).
class SearchField : public ui::Edit {
public:
    SearchField(std::function<void()> clear, std::function<void()> save);
    void arrange(ui::Rect r) override;
    void sync_buttons();  // shown while the box has text
    ui::Button* clear_button() const { return clear_; }
    ui::Button* save_button() const { return save_; }
    ui::Button* glyph() const { return glyph_; }

private:
    ui::Button* glyph_ = nullptr;
    ui::Button* clear_ = nullptr;
    ui::Button* save_ = nullptr;
};

// One tab: navigation row, path, search, the details list, the message over an empty folder.
class TabView : public ui::DockPanel {
public:
    static std::wstring filters_tip();
    void mirror_selection_to_tree();
    TabView(MainView& main, core::Core& core, uint64_t tab);
    ~TabView() override;
    void arrange(ui::Rect r) override;

    uint64_t tab() const { return tab_; }
    ui::ListView* list() const { return list_; }
    ui::TileView* tiles() const { return tiles_; }
    bool tiles_mode() const { return tiles_on_; }
    ListAdapter& adapter() { return adapter_; }
    ui::Edit* search_box() const { return search_; }
    ui::Edit* global_search_box() const;  // the title bar's, which acts on the active tab

    // Esc in a search box: a search still running is stopped and keeps what it found; otherwise the box is
    // cleared and the list takes the keyboard. Spec: DirectoryTabView.SearchBox_KeyDown.
    void search_escape(bool global);
    void clear_search(bool global);
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
    std::wstring last_tree_mirror_;  // the folder the selection last showed in the tree
    void end_path_edit();
    void sync_columns();
    void sync_view_mode(const BbTabState& st);
    void sync_side_pane(const BbTabState& st);
    void on_search_changed(bool global);
    // Explorer-style type-to-select: a letter jumps to (and, repeated, cycles through) the names starting
    // with it; letters typed in quick succession match a longer prefix. `typed` is everything typed inside
    // the widget's window, this keystroke last. Spec: DirectoryTabView.FileList_TextInput.
    void type_ahead(const std::wstring& typed);
    void open_row(size_t row);
    bool select_by_prefix(const std::wstring& prefix, size_t start);
    std::wstring typeahead_prefix_;
    uint64_t reveal_seq_ = 0;
    ui::ProgressBar* activity_ = nullptr;
    ui::TextBlock* notice_ = nullptr;
    ui::Widget* notice_panel_ = nullptr;
    void apply_reveal(const BbTabState& st);

    void open_context(ui::Point p, bool header, bool background = false);
    ui::Widget* active_view() const;

    MainView& main_;
    core::Core& core_;
    uint64_t tab_;
    ListAdapter adapter_;
    ui::StackPanel* nav_ = nullptr;
    ui::Button *back_ = nullptr, *forward_ = nullptr, *up_ = nullptr, *refresh_ = nullptr;
    ui::Button* side_buttons_[3] = {nullptr, nullptr, nullptr};
    ui::Button* flat_button_ = nullptr;
    ui::Panel* path_bar_ = nullptr;
    BreadcrumbBar* crumbs_ = nullptr;
    PathEdit* path_ = nullptr;
    SearchField* search_ = nullptr;
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
    void on_inner_activity() override;

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
    // `restore_session`: last session's panes (or the startup folder), `path` only as the fallback.
    void start(const std::wstring& path, bool restore_session = false);
    // Pulls the palette from the core into the window's resources.
    void apply_theme();
    // Re-reads the layout and every tab's view from the core.
    void sync_layout();
    // Drains events already posted to the window; returns how many were handled.
    size_t events_handled() const { return handled_; }

    core::Core& core() { return core_; }
    platform::Window& window() { return window_; }
    // The menu a toolbar button, its overflow or its chevron last dropped down (harness).
    const ui::MenuItems* floating_menu() const { return floating_menu_.get(); }
    bool menu_open() const { return popup_ && popup_->menu_open(); }  // an in-tree menu is up (harness)
    uint64_t active_tab() const { return active_tab_; }
    uint64_t active_pane() const { return active_pane_; }
    TabView* tab_view(uint64_t tab);
    TabView* active_view() { return tab_view(active_tab_); }
    size_t pane_count() const { return pane_views_.size(); }
    size_t tab_count_in_active_pane() const;
    TabStrip* active_strip() const;
    StatusBar* status_bar() const { return status_; }
    std::vector<std::wstring> tab_titles_in_active_pane() const;
    std::vector<uint64_t> tab_ids_in_active_pane() const;
    std::vector<uint64_t> pane_ids() const;
    // Searches the active tab's widgets first, then the whole window.
    ui::Widget* find_named(const std::string& name);
    FolderTreeProvider& folder_tree() { return *tree_provider_; }
    ui::TreeView* tree_view() const { return tree_; }
    // Reveals the tab's folder in the tree without navigating.
    void reveal_in_tree(const std::wstring& path);
    void sync_tree_hidden();
    // A tab navigated: the tree follows when it is the active one.
    void tab_navigated(uint64_t tab, const std::wstring& path);
    // The files on the clipboard (the system's, or a run's own), put there by Copy or Cut.
    bool clipboard_put(const std::vector<std::wstring>& paths, bool cut);
    std::optional<ClipboardFiles> clipboard_get();
    // Tells the core whether Paste has anything to paste.
    void push_clipboard_flag();
    // Edit > Paste (or Paste shortcut): what is on the clipboard, into the folder the tab shows.
    bool paste(bool shortcut);
    // Copy to / Move to: the selection into a folder chosen in Windows' folder picker.
    bool transfer_to_chosen(bool move);
    // Scans `folder` for duplicates and shows what it found.
    void open_duplicates(const std::wstring& folder);
    // Puts paths (quoted, one per line) or names on the clipboard and says so on the status line.
    void copy_text(const std::vector<std::wstring>& items, bool paths, bool this_folder);
    // The folder tree's right-click menu, about `folder` (the row right-clicked), at a point.
    void open_tree_menu(const std::wstring& folder, ui::Point at);
    // One of the tree menu's entries, carried out on `folder` rather than on the file list's selection.
    void tree_verb(const std::wstring& id, const std::wstring& folder);
    // The folder a dialog opened from the tree menu is about, while it is being built (New, Properties,
    // What changed); empty for one opened any other way, which reads the active tab.
    const std::wstring& dialog_folder() const { return dialog_folder_; }

    // Runs one command against the active tab through the one gate the keys and menus share.
    bool run_command(const std::wstring& id);
    // As run_command, but a command that is the host's to carry out is carried out here. `reason` says why
    // a command that could not run could not; `ui` is set when the host did it.
    bool execute_command(const std::wstring& id, std::wstring* reason = nullptr, bool* ui = nullptr);
    void activate_tab(uint64_t tab);
    void close_tab(uint64_t tab);
    void activate_pane(uint64_t pane);
    // A tab's right-click menu: Duplicate, Close others, Move to new pane, Close. Spec: FilePaneView.xaml.
    ui::MenuItems tab_menu(uint64_t tab);

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
    void wire_tab_strip(TabStrip& strip, uint64_t pane);
    // The flags the core wants beside a key: what overlay is up.
    uint32_t focus_flags();
    BbFocusKind focus_kind() const;
    // Carries out a command that is the host's to carry out. false when it wants a person (a dialog) and was not.
    bool host_command(const std::wstring& id);
    bool dialog_key(const ui::KeyEvent& e);
    // What follows a command the core carried out (the keyboard back on the active list).
    void after_command(const std::wstring& id);
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
    std::function<void()> on_sync_progress;  // the Sync dialog's live progress (its own slot: the Transfers window has the other)
    // The same for an open History window.
    std::function<void()> on_undo_changed;
    std::function<void()> on_delete_survey;  // the delete confirmation measured another item
    // A question the core is waiting on (a clash, an offer of administrator rights) puts its dialog up.
    void sync_questions(const wchar_t* kind);
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
    // It also swaps the system clipboard for one held in memory: a run's copy and paste must not touch the
    // person's clipboard, and what is on theirs must not leak into the run.
    void set_headless_dialogs(bool on);
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
    // A dialog that is a popup (the column picker), opened with its corner at `at`.
    void open_dialog_at(const std::wstring& kind, ui::Point at) {
        dialog_anchor_ = at;
        open_dialog(kind);
        dialog_anchor_.reset();
    }
    void refresh_tool(const std::wstring& kind);
    void sync_devices(bool refresh);
    void sync_wheel_speed();
    void open_device(const std::wstring& path);
    // A card that says something and has an OK: what a refused plan reports (C#'s MessageDialog), raised on
    // the UI thread, unlike the notice seam a worker parks on.
    void show_message(std::wstring title, std::wstring text);
    std::pair<std::wstring, std::wstring> message() const { return message_; }
    // A key in a tool window that is not Esc: true when the window's own bindings took it.
    bool tool_window_key(const std::wstring& key, const ui::KeyEvent& e);
    // Gives the keyboard to the active tab's list (or its tiles). False when there is no tab.
    bool focus_file_list();
    // The delete confirmation over `paths` rather than the selection; `after` hears whether Delete was
    // pressed once the dialog has gone.
    void ask_delete(std::vector<std::wstring> paths, std::function<void(bool confirmed)> after);
    // New <type> from the New submenu: the New file dialog with that type's extension and name, in `folder`
    // (empty: the tab's).
    void open_new_file(const std::wstring& extension, const std::wstring& label, const std::wstring& folder);
    std::pair<std::wstring, std::wstring> new_file_template() const { return new_file_template_; }
    // The Save search dialog editing a saved search, and the rename of a saved workspace: the one edit for
    // the sidebar's menu and the Settings page alike.
    void edit_saved_search(const std::wstring& name);
    void rename_workspace(const std::wstring& name);
    const std::wstring& editing_saved() const { return editing_saved_; }
    // Opens (or rebuilds) a dialog about these items rather than the selection.
    void reopen_on(const std::wstring& kind, std::vector<std::wstring> paths);
    std::vector<std::wstring>& properties_paths() { return properties_paths_; }
    const std::vector<std::wstring>& dialog_paths() const { return dialog_paths_; }
    // A scripted run normally reports a dialog instead of putting it up; with this on it really opens,
    // so a script can type into it and press its buttons (`dialog_widget`).
    void set_live_dialogs(bool on) { live_dialogs_ = on; }
    bool shows_dialogs() const { return !headless_dialogs_ || live_dialogs_; }
    // A scripted run: nothing that reads or changes the machine itself (the registry, the scheduler).
    bool scripted() const { return headless_dialogs_; }
    bool dialog_up() const { return dialog_open_; }
    const std::wstring& dialog_kind() const { return dialog_kind_; }
    ui::Widget* dialog_widget(std::string_view name);
    // The tool windows (disk usage, duplicates, ...): modeless windows of their own, as the original's
    // were, so the folders stay live behind them. `open_dialog` sends their kinds here; opening one that is
    // already up rebuilds it in place and brings it forward.
    static bool is_tool_window(const std::wstring& kind);
    ToolWindow* tool_window(const std::wstring& kind);
    void close_tool(const std::wstring& kind);
    // Re-reads the duplicate finder's model, and rebuilds its window if it is open.
    void sync_duplicates();
    void refresh_index_windows();
    void refresh_changes(bool force);
    static constexpr UINT_PTR kChangesTimer = 0xBB01;
    // The Properties dialog's Details: the one path whose shell properties were last read, and what they were.
    std::pair<std::wstring, std::vector<std::pair<std::wstring, std::wstring>>>& properties_details() { return properties_details_; }
    void style_tool_window(ToolWindow& w);
    std::vector<std::wstring> open_tool_windows() const;
    std::function<void(bool)> take_after_delete() { return std::exchange(after_delete_, nullptr); }
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
    size_t tree_pending() const;  // folder-tree reads still on a worker
    // The property system still being read for the Add column panel (the harness waits for it).
    bool column_props_pending();
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
    std::unique_ptr<ui::Widget> build_node(const json::Value& node, const json::Value& panes, const std::vector<uint32_t>& path);
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
    bool live_dialogs_ = false;
    std::pair<std::wstring, std::vector<std::pair<std::wstring, std::wstring>>> properties_details_;
    std::map<std::wstring, std::unique_ptr<ToolWindow>> tools_;  // by tool_key
    std::string changes_signature_;  // what the What changed window last showed
    bool follow_windows_ = true;
    std::wstring dialog_folder_;
    std::vector<std::wstring> dialog_paths_;  // what the delete confirmation being built asks about
    std::vector<std::wstring> properties_paths_;  // what the Properties dialog up is about
    std::wstring editing_saved_;                   // the saved search or workspace the dialog being built edits
    std::pair<std::wstring, std::wstring> message_;  // title, text: the message card
    std::pair<std::wstring, std::wstring> new_file_template_;  // extension, label: the New file dialog being built
    std::function<void(bool)> after_delete_;
    // A run's clipboard (see set_headless_dialogs), and its change count, standing in for Windows'.
    std::optional<ClipboardFiles> run_clipboard_;
    uint32_t run_clipboard_seq_ = 0;
    // A paste of cut files under way: the clipboard is emptied once it has moved them, as Explorer's is -
    // unless something else has been put on the clipboard meanwhile.
    uint64_t cut_paste_op_ = 0;
    std::vector<std::wstring> cut_paste_paths_;  // what the cut paste was moving
    uint32_t cut_paste_seq_ = 0;
    uint32_t clipboard_seq() const;
    std::unique_ptr<ShellMenuSource> shell_source_;
    std::unique_ptr<FileDrag> file_drag_;
    std::shared_ptr<void> menu_session_;
    std::vector<std::function<void()>> deferred_;
    json::Value queue_;
    ui::StackPanel* transfer_strip_ = nullptr;
    ui::TextBlock *transfer_head_ = nullptr, *transfer_detail_ = nullptr;
    ui::ProgressBar* transfer_bar_ = nullptr;
    ui::Button *transfer_pause_ = nullptr, *transfer_cancel_ = nullptr;
    ui::TextBlock* transfer_waiting_ = nullptr;
    DuplicatesState duplicates_;
    bool machine_props_set_ = false;
    std::optional<ui::Point> dialog_anchor_;
    bool machine_props_asked_ = false;  // the worker has been started
    DialogLayer* dialogs_ = nullptr;
    ui::StackPanel* indexer_banner_ = nullptr;
    ui::TextBlock* indexer_text_ = nullptr;
    json::Value index_state_;
    ThumbSource thumb_source_ = ThumbSource::Shell;
    bool dialog_open_ = false;
    bool refreshing_tool_ = false;
    std::string changes_stamp_;  // the change log as What changed last read it
    int changes_ticks_ = 0;
    std::string theme_editor_id_;  // the theme the open editor shows, so another one rebuilds it
    ui::Widget* focus_before_dialog_ = nullptr;  // only ever compared, or used once found in the tree again  // refresh_tool: rebuild without taking the foreground
    bool questions_waiting_ = false;  // a question or notice arrived while a dialog it may not replace was up
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
    bool index_was_building_ = false;
    // The title bar's workspace and saved-search dropdowns, shown when Settings places them there.
    ui::StackPanel* switchers_ = nullptr;
    ui::Button* workspace_switcher_ = nullptr;
    ui::Button* search_switcher_ = nullptr;
    void sync_switchers();
    void open_switcher(bool workspaces);
    // The whole-PC search: one box in the title bar, the active tab's query. Spec: MainWindow.xaml (GlobalSearchField).
    ui::StackPanel* title_search_group_ = nullptr;
    SearchField* title_search_ = nullptr;
    bool title_search_suppress_ = false;

public:
    SearchField* title_search() const { return title_search_; }
    // Shows `text` in the title bar's box without it counting as typing.
    void set_title_search_text(const std::wstring& text);
    // The box follows the active tab: its query when that is a whole-PC one, else nothing.
    void sync_title_search();

private:

public:
    // The title bar switcher's dropdown, as it would open (the harness photographs it).
    ui::MenuItems switcher_items(bool workspaces);

private:
    std::wstring revealed_;
    bool revealing_ = false;
    size_t handled_ = 0;
    std::wstring status_text_;
};

}  // namespace bb::views
