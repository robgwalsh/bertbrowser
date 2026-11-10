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
#include "ui/popups/rich_tip.h"

#include <dwmapi.h>
#include <shobjidl.h>

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace bb::ui;

// ---- MainView ----------------------------------------------------------------------------------------

MainView::MainView(core::Core& core, platform::Window& window)
    : core_(core),
      window_(window),
      icons_(std::make_unique<ShellIcons>(window.hwnd())),
      tree_provider_(std::make_unique<FolderTreeProvider>(icons_.get())) {
    // WindowRoot holds the window and, last, the popup layer that covers it.
    auto outer = std::make_unique<Panel>();
    outer->name = "WindowRoot";
    auto* root = outer->emplace<DockPanel>();
    root->name = "MainDock";
    root->style.bg = tok::WindowBackground;
    setup_title_bar(*root);
    DockPanel* browser_dock = setup_browser(*root);
    setup_transfer_strip(*browser_dock);
    setup_indexer_banner(*browser_dock);
    setup_sidebar(*browser_dock);
    setup_pane_column();
    wire_tree_events();
    setup_overlays();
    wire_window(std::move(outer));
}

// The caption: icon, menu bar, toolbar, the switchers and the search group.
void MainView::setup_title_bar(DockPanel& root) {
    title_ = root.emplace<TitleBar>();
    title_->name = "TitleBarRoot";
    title_->dock = Dock::Top;
    title_->show_title = false;
    // The app's icon at the left of the caption; a click on it is the window menu. Spec: MainWindow.xaml
    // (ShowsAppIcon). The harness exe has no icon resource, and shows none.
    if (HICON icon = static_cast<HICON>(LoadImageW(GetModuleHandleW(nullptr), MAKEINTRESOURCEW(1), IMAGE_ICON, 32, 32, LR_DEFAULTCOLOR))) {
        auto* logo = title_->emplace<ImageView>();
        logo->name = "TitleAppIcon";
        logo->interactive = false;
        logo->checkerboard = false;
        logo->style.bg = gfx::kNoToken;
        logo->nc_hit = HTSYSMENU;
        logo->set_image(icon_picture(icon, 32));
        DestroyIcon(icon);
        title_->app_icon = logo;
    }
    menu_bar_ = title_->emplace<MenuBar>();
    menu_bar_->name = "MainMenu";
    menu_bar_->nc_hit = HTCLIENT;
    title_->left = menu_bar_;
    title_toolbar_ = title_->emplace<CommandToolbar>("title");
    title_toolbar_->name = "TitleToolbar";
    title_toolbar_->style.bg = gfx::kNoToken;
    title_toolbar_->nc_hit = HTCLIENT;
    title_->middle = title_toolbar_;
    setup_title_switchers();
    setup_title_search();
    setup_indexer_button();
}

namespace {
// A title-bar button whose tip is built each time it is shown (and again while it is up) from a model.
class RichTipButton : public Button {
public:
    std::function<RichTipContent()> tip;
    bool has_rich_tip() const override { return static_cast<bool>(tip); }
    std::unique_ptr<Widget> make_rich_tip(gfx::Resources& res) override { return std::make_unique<RichTip>(res, tip()); }
};
}  // namespace

void MainView::setup_indexer_button() {
    auto* b = title_->emplace<RichTipButton>();
    b->name = "IndexerButton";
    b->icon = &icons::Indexing;
    b->flat = true;  // the caption buttons' look: nothing until the pointer is over it
    b->width = 40;
    b->style.bg = tok::TitleBarBackground;
    b->style.border = tok::TitleBarBackground;
    b->style.fg = tok::TitleBarForeground;
    b->flags.tab_stop = false;
    b->flags.focusable = false;
    b->nc_hit = HTCLIENT;
    b->on_click = [this] {
        if (popup_) popup_->hide_tip();  // the page it opens says the same, and more
        bb_settings_page_open(core_.app(), core::s(L"search-index"));
    };
    b->tip = [this] {
        RichTipContent c;
        c.title = L"Search index helper";
        c.subtitle = core::wide(index_state_["helperText"].str());
        const json::Value& usage = index_state_["usage"];
        const json::Value& rows = usage["rows"];
        for (size_t i = 0; i < rows.size(); ++i) {
            c.rows.push_back({core::wide(rows[i]["label"].str()), core::wide(rows[i]["value"].str()),
                              core::wide(rows[i]["detail"].str())});
        }
        c.value_template = core::wide(usage["valueWidthTemplate"].str());
        if (c.rows.empty()) c.rows.push_back({L"Resource use", L"not reported yet"});
        c.footer = L"Click for the Search index settings";
        return c;
    };
    b->set_visible(false);
    title_->trailing = b;
    indexer_button_ = b;
}

void MainView::sync_indexer_button() {
    if (!indexer_button_) return;
    const bool running = index_state_["presence"].str() == "running";
    if (indexer_button_->flags.visible != running) {
        if (!running && popup_) popup_->hide_tip();
        indexer_button_->set_visible(running);
        title_->request_layout();
        title_->arrange(title_->bounds);
        title_->invalidate();
    }
    if (popup_) popup_->refresh_tip(indexer_button_);
}

void MainView::refresh_index_usage() {
    index_state_ = try_model([&](BbOwnedStr* o) { return bb_index_state(core_.app(), o); });
    if (index_state_.is_null()) return;
    sync_indexer_button();
    if (settings_) settings_->sync_usage();
}

void MainView::setup_title_switchers() {
    // Workspaces and saved searches, when Settings puts them in the title bar rather than the sidebar.
    // Spec: MainWindow.xaml (WorkspaceSwitcher, SavedSearchSwitcher).
    switchers_ = title_->emplace<StackPanel>(Orientation::Horizontal, 6.0f);
    switchers_->name = "TitleSwitchers";
    switchers_->nc_hit = HTCLIENT;
    auto make = [this](const char* id, const IconData& icon, const wchar_t* tip, bool workspaces) {
        auto* b = switchers_->emplace<Button>();
        b->name = id;
        b->icon = &icon;
        b->tooltip = tip;
        b->height = 26;
        // The same well as the whole-PC search beside it, so the two read as one family. Spec: HeaderDropdownButton.
        b->style.bg = tok::WindowBackground;
        b->style.border = tok::BorderSubtle;
        b->flags.tab_stop = false;
        b->nc_hit = HTCLIENT;
        b->on_click = [this, workspaces] { open_switcher(workspaces); };
        return b;
    };
    workspace_switcher_ = make("WorkspaceSwitcher", icons::SplitRight, L"Switch workspace", true);
    search_switcher_ = make("SavedSearchSwitcher", icons::Search, L"Run a saved search", false);
    switchers_->set_visible(false);
    title_->after = switchers_;
}

void MainView::setup_title_search() {
    // The command palette beside the whole-PC search: both are "find it by typing", one for commands and
    // one for files. Centred on the bar. Spec: MainWindow.xaml (TitleBarSearchGroup).
    title_search_group_ = title_->emplace<StackPanel>(Orientation::Horizontal, 4.0f);
    title_search_group_->name = "TitleBarSearchGroup";
    title_search_group_->nc_hit = HTCLIENT;
    auto* palette = title_search_group_->emplace<Button>();
    palette->name = "PaletteButton";
    palette->icon = &icons::Palette;
    palette->width = 30;
    palette->height = 26;
    palette->style.bg = tok::TitleBarBackground;
    palette->style.border = tok::TitleBarBackground;
    palette->style.fg = tok::TitleBarForeground;
    palette->flags.tab_stop = false;
    palette->flags.focusable = false;  // the palette hands the keyboard back to what had it
    palette->nc_hit = HTCLIENT;
    palette->on_click = [this] { execute_command(L"app.palette"); };
    title_search_ = title_search_group_->emplace<SearchField>(
        [this] {
            if (TabView* tv = active_view()) tv->clear_search(true);
        },
        [this] { execute_command(L"search.save"); });
    title_search_->name = "GlobalSearchBox";
    title_search_->width = 300;
    title_search_->height = 26;
    // A well in the title bar: the window's own colour inside a faint edge. Spec: HeaderFieldBorder.
    title_search_->style.bg = tok::WindowBackground;
    title_search_->style.border = tok::BorderSubtle;
    for (Button* b : {title_search_->clear_button(), title_search_->save_button()}) {
        b->style.bg = tok::WindowBackground;
        b->style.border = tok::WindowBackground;
    }
    title_search_->placeholder = L"Search this PC";
    title_search_->nc_hit = HTCLIENT;
    title_search_->clear_button()->name = "GlobalSearchClearButton";
    title_search_->save_button()->name = "GlobalSaveSearchButton";
    title_search_->on_changed = [this](const std::wstring& text) {
        title_search_->sync_buttons();
        if (title_search_suppress_ || !active_tab_) return;
        bb_tab_set_search(core_.app(), active_tab_, core::s(text), true);
    };
    title_search_->on_cancel = [this] {
        if (TabView* tv = active_view()) tv->search_escape(true);
    };
    // The syntax reference, to the right of the field it explains. Spec: MainWindow.xaml (SearchSyntaxButton).
    auto* syntax = title_search_group_->emplace<Button>();
    syntax->name = "SearchSyntaxButton";
    syntax->icon = &icons::Properties;
    syntax->tooltip = L"What you can type here";
    syntax->width = 30;
    syntax->height = 26;
    syntax->style.bg = tok::TitleBarBackground;
    syntax->style.border = tok::TitleBarBackground;
    syntax->style.fg = tok::TitleBarForeground;
    syntax->flags.tab_stop = false;
    syntax->nc_hit = HTCLIENT;
    syntax->on_click = [this] { execute_command(L"search.syntax"); };
    title_->center = title_search_group_;
    refresh_tooltips();  // and again whenever a key is rebound
}

// Everything under the title bar: the toolbar rows and the status bar. Returns the dock the rest is put in.
DockPanel* MainView::setup_browser(DockPanel& root) {
    browser_ = root.emplace<Panel>();
    browser_->name = "BrowserRoot";
    browser_->dock = Dock::Fill;
    auto* browser_dock = browser_->emplace<DockPanel>();
    browser_dock->name = "BrowserDock";
    rows_panel_ = browser_dock->emplace<StackPanel>(Orientation::Vertical, 0.0f);
    rows_panel_->name = "ToolbarRows";
    rows_panel_->dock = Dock::Top;
    rows_panel_->set_visible(false);
    status_ = browser_dock->emplace<StatusBar>();
    status_->name = "StatusBar";
    status_->dock = Dock::Bottom;
    status_->on_right_click = [this] {
        if (index_state_["canRetry"].boolean()) bb_index_do(core_.app(), core::s(L"retry"));
    };
    status_->on_undo_click = [this] { execute_command(L"edit.undo-history"); };
    status_->on_scale = [this](double scale) {
        if (active_tab_) bb_tab_set_thumbnail_scale(core_.app(), active_tab_, scale);
    };
    return browser_dock;
}

void MainView::setup_transfer_strip(DockPanel& browser_dock) {
    // What a copy or move is doing: the headline, a bar, and the numbers. Above the status line.
    auto* strip = browser_dock.emplace<StackPanel>(Orientation::Horizontal, 14.0f);
    strip->name = "TransferStrip";
    strip->dock = Dock::Bottom;
    strip->height = 36;
    strip->pad = 8;
    strip->style.bg = tok::SurfaceRaised;
    strip->set_visible(false);
    transfer_strip_ = strip;
    transfer_head_ = strip->emplace<TextBlock>();
    transfer_head_->width = 250;
    transfer_head_->style.font.weight = 600;
    transfer_bar_ = strip->emplace<ProgressBar>();
    transfer_bar_->width = 220;
    transfer_detail_ = strip->emplace<TextBlock>();
    transfer_detail_->flex = 1;
    transfer_detail_->style.fg = tok::TextSecondary;
    // How many more are queued behind this one; blank when there is only the running job. Spec:
    // MainWindow.xaml (TransferQueue.WaitingText).
    transfer_waiting_ = strip->emplace<TextBlock>();
    transfer_waiting_->name = "TransferWaiting";
    transfer_waiting_->style.fg = tok::TextSecondary;
    // Links, as the original's were. Spec: MainWindow.xaml (the transfer strip's LinkButtons).
    transfer_pause_ = strip->emplace<Button>(L"Pause");
    transfer_pause_->name = "TransferPause";
    transfer_pause_->link = true;
    transfer_pause_->on_click = [this] {
        const bool paused = queue_["paused"].boolean();
        BbOwnedStr o{};
        bb_ops_do(core_.app(), core::s(paused ? L"queue-resume" : L"queue-pause"), core::s(L""), &o);
        bb_str_free(o);
    };
    auto* details = strip->emplace<Button>(L"Details");
    details->name = "TransferDetails";
    details->link = true;
    details->tooltip = L"Show every item in this transfer, and everything queued behind it";
    details->on_click = [this] { open_dialog(L"transfer"); };
    transfer_cancel_ = strip->emplace<Button>(L"Cancel");
    transfer_cancel_->name = "TransferCancel";
    transfer_cancel_->link = true;
    transfer_cancel_->style.font.weight = 600;
    transfer_cancel_->tooltip = L"Stop this transfer. Whatever has already been transferred stays, and can be undone.";
    transfer_cancel_->on_click = [this] {
        BbOwnedStr o{};
        bb_ops_do(core_.app(), core::s(L"cancel-running"), core::s(L""), &o);
        bb_str_free(o);
    };
}

void MainView::setup_indexer_banner(DockPanel& browser_dock) {
    // The offer to start the search index helper, above the status bar and the transfer strip:
    // shown until answered, Dismiss lasts the session. A neutral surface rather than a warning,
    // on purpose: nothing is wrong, search falls back to a crawl, and this is an offer. Spec:
    // MainWindow.xaml (IndexerBanner).
    auto* banner = browser_dock.emplace<StackPanel>(Orientation::Vertical, 0.0f);
    banner->name = "IndexerBanner";
    banner->dock = Dock::Bottom;
    banner->set_visible(false);
    indexer_banner_ = banner;
    auto* rule = banner->emplace<Widget>();
    rule->height = 1;
    rule->style.bg = tok::BorderSubtle;
    auto* row = banner->emplace<StackPanel>(Orientation::Horizontal, 12.0f);
    row->pad = 7;
    row->style.bg = tok::SurfaceRaised;
    auto* glyph = row->emplace<Icon>(&icons::Indexing, 14.0f);
    glyph->style.fg = tok::TextSecondary;
    indexer_text_ = row->emplace<TextBlock>();
    indexer_text_->style.fg = tok::TextSecondary;
    indexer_text_->wrap = true;
    indexer_text_->vcenter = true;
    indexer_text_->flex = 1;
    auto* start = row->emplace<Button>(L"Start indexer");
    start->name = "IndexerStart";
    start->icon = &icons::Shield;  // a UAC prompt, as everywhere else that asks for administrator rights
    start->tooltip = L"Ask for administrator rights so drives can be indexed";
    start->on_click = [this] { bb_index_do(core_.app(), core::s(L"start")); };
    auto* dismiss = row->emplace<Button>(L"Dismiss");
    dismiss->name = "IndexerDismiss";
    dismiss->link = true;
    dismiss->tooltip = L"Hide this until the next time BertBrowser starts";
    dismiss->on_click = [this] { bb_index_do(core_.app(), core::s(L"dismiss-banner")); };
}

// The sidebar column: saved searches and workspaces, and the folder tree under its header.
void MainView::setup_sidebar(DockPanel& browser_dock) {
    main_split_ = browser_dock.emplace<Splitter>(Orientation::Horizontal);
    main_split_->dock = Dock::Fill;
    main_split_->name = "MainSplitter";
    main_split_->set_position(260);  // Spec: MainWindow.xaml (sidebar column Width 260, MinWidth 150)
    main_split_->min_first = 150;
    auto* sidebar_column = main_split_->emplace<DockPanel>();
    sidebar_column->name = "SidebarColumn";
    sidebar_ = sidebar_column->emplace<SavedSidebar>(*this, core_);
    sidebar_->dock = Dock::Bottom;
    {
        // DRIVES, under the same kind of header the other sections have, with Collapse all beside it.
        // Spec: MainWindow.xaml (the folder tree's header), FolderTreeViewModel.CollapseAll.
        auto* head = sidebar_column->emplace<StackPanel>(Orientation::Horizontal, 4.0f);
        head->name = "FolderTreeHeader";
        head->dock = Dock::Top;
        head->pad = 4;
        head->height = 28;
        head->style.bg = tok::SurfaceBackground;
        auto* title = head->emplace<TextBlock>(L"DRIVES");
        title->style.fg = tok::TextMuted;
        title->style.font.size = 11.5f;
        title->style.font.weight = 600;
        title->vcenter = true;
        title->flex = 1;
        auto* collapse = head->emplace<Button>();
        collapse->name = "CollapseFolderTree";
        collapse->icon = &icons::CollapseAll;
        collapse->tooltip = L"Collapse all";
        collapse->width = 22;
        collapse->height = 20;
        collapse->style.bg = tok::SurfaceBackground;
        collapse->style.border = tok::SurfaceBackground;
        collapse->flags.tab_stop = false;
        collapse->on_click = [this] {
            // Every drive and folder folded up; what was loaded stays, so opening one again costs no rescan.
            if (!tree_ || !tree_provider_) return;
            for (const uint64_t id : tree_provider_->loaded_ids()) {
                if (tree_->is_expanded(id)) tree_->collapse(id);
            }
        };
    }
    tree_ = sidebar_column->emplace<TreeView>();
    tree_->dock = Dock::Fill;
    tree_->name = "FolderTree";
    // Sizes beside folders come from the core's cache (one lookup per batch, never a scan); drives say
    // the space they use. Spec: FolderTreeViewModel (SizeText).
    // A worker's read of a folder lands on this thread through the window. Only a post: see on_worker_done.
    tree_provider_->on_worker_done = [hwnd = window_.hwnd()] { PostMessageW(hwnd, WM_BB_TREE, 0, 0); };
    tree_provider_->size_source = [this](const std::vector<std::wstring>& paths) {
        std::string list = "[";
        for (size_t i = 0; i < paths.size(); ++i) list += (i ? "," : "") + ops::q(paths[i]);
        list += "]";
        const json::Value sizes = try_model([&](BbOwnedStr* o) { return bb_tree_sizes(core_.app(), core::s(core::wide(list)), o); });
        std::vector<std::wstring> out;
        for (size_t i = 0; i < sizes.size(); ++i) out.push_back(core::wide(sizes[i].str()));
        return out;
    };
    tree_provider_->format_bytes = [](int64_t bytes) {
        BbOwnedStr o{};
        return bb_byte_size(bytes, &o) == BB_OK ? core::take(o) : std::wstring();
    };
    tree_->set_provider(tree_provider_.get());
    sync_tree_hidden();
    // A program's or special folder's own icon came back from the shell: repaint whatever shows it.
    icons_->on_ready = [this] {
        window_.root().invalidate_all();
        for (auto& kv : tools_) kv.second->window().root().invalidate_all();
    };
}

void MainView::setup_pane_column() {
    // The panes, with the compare banner above them and not above the sidebar: a comparison is one
    // thing spanning two panes. Spec: MainWindow.xaml (PaneHostSite).
    auto* pane_column = main_split_->emplace<DockPanel>();
    pane_column->name = "PaneColumn";
    pane_column->add(make_compare_banner(*this));
    pane_host_ = pane_column->emplace<Panel>();
    pane_host_->name = "PaneHostSite";
    pane_host_->dock = Dock::Fill;
}

void MainView::wire_tree_events() {
    // A folder middle-clicked opens in a pane to the right; a drive as the "middle-click a drive" setting says
    // (a new panel, or a new tab). Spec: MainWindow.FolderTreeItem_PreviewMiddleClick, ShellViewModel.OpenDrive.
    tree_->on_middle = [this](uint64_t id) {
        const std::wstring path = tree_provider_->path_of(id);
        if (path.empty()) return;
        if (tree_provider_->is_device(id)) {
            open_device(path);
            return;
        }
        BbApp* app = core_.app();
        BbOwnedStr o{};
        const bool panel = bb_settings_get(app, core::s(L"drivesOpenTarget"), &o) == BB_OK && core::take(o).find(L"NewPanel") != std::wstring::npos;
        if (path.size() <= 3 && !panel) {
            uint64_t tab = 0;
            bb_tab_open(app, 0, core::s(path), true, &tab);
        } else {
            uint64_t pane = 0;
            bb_pane_split(app, 0, BB_VERTICAL, core::s(path), &pane);
        }
    };
    // No menu on a device: there is no path behind it for any entry to act on. Spec: MainWindow.FolderTree_ContextMenu.
    tree_->on_context = [this](uint64_t id, Point at) {
        if (dialog_up()) return;
        const std::wstring path = tree_provider_->path_of(id);
        if (!path.empty() && !tree_provider_->is_device(id)) open_tree_menu(path, at);
    };
    // A device is no path to browse: a double-click (or a middle-click) opens it in Explorer, and selecting it
    // navigates nowhere. Spec: MainWindow.FolderTree_DoubleClick, ShellViewModel.MiddleClickDriveOrDevice.
    tree_->on_activate = [this](uint64_t id) {
        if (tree_provider_->is_device(id)) open_device(tree_provider_->path_of(id));
    };
    tree_->on_select = [this](uint64_t id) {
        if (revealing_ || !active_tab_ || tree_provider_->is_device(id)) return;
        const std::wstring path = tree_provider_->path_of(id);
        if (!path.empty()) {
            try {
                core_.navigate(active_tab_, path);
            } catch (const core::CoreError&) {
            }
        }
    };
}

void MainView::setup_overlays() {
    // The overlays sit over everything under the title bar, in the order they stack: Settings, then (later)
    // the palette and the dialog layer.
    settings_ = browser_->emplace<SettingsView>(*this, core_);
    settings_->name = "SettingsLayer";
    palette_ = browser_->emplace<PaletteView>(*this, core_);
    palette_->name = "PaletteLayer";
    dialogs_ = browser_->emplace<DialogLayer>();
    dialogs_->name = "DialogLayer";
    // The search syntax card is a reference sheet: any click puts it away. Spec: SearchSyntaxDialog.
    dialogs_->on_click = [this] {
        if (dialog_open_ && (dialog_kind_ == L"search-syntax" || dialogs_->is_popup())) defer([this] { close_dialog(); });
    };
    window_.on_timer = [this](UINT_PTR id) {
        if (id == kChangesTimer) refresh_changes(false);
    };
    // Tab stays inside whatever lies over the folders: a dialog card, else the Settings page.
    window_.root().tab_scope = [this]() -> Widget* {
        if (dialog_open_ && dialogs_) return dialogs_;
        if (settings_ && settings_->is_open()) return settings_;
        return nullptr;
    };
    dialogs_->on_closed = [this] {
        dialog_open_ = false;
        dialog_kind_.clear();
        // The keyboard goes back where it was, if that is still there.
        if (window_.root().contains(focus_before_dialog_) && !dialogs_->holds(window_.root().focused())) {
            window_.root().set_focus(focus_before_dialog_);
        }
        focus_before_dialog_ = nullptr;
        bb_ui_set_dialog_open(core_.app(), false);
        if (questions_waiting_) {
            questions_waiting_ = false;
            // Deferred: a dialog is also closed on the way to opening the next one.
            defer([this] {
                if (dialog_open_) return;
                for (const wchar_t* kind : {L"conflicts", L"elevation", L"confirm", L"notice"}) sync_questions(kind);
            });
        }
    };
}

void MainView::wire_window(std::unique_ptr<Panel> outer) {
    Widget* content = window_.root().set_content(std::move(outer));
    popup_ = PopupLayer::install(*content);

    menu_bar_->hook(window_.root());
    menu_bar_->allows_mode = [this] { return (focus_flags() & (BB_FOCUS_PALETTE_OPEN | BB_FOCUS_CONTENT_BLOCKED | BB_FOCUS_RECORDING)) == 0; };
    title_toolbar_->on_activate = [this](const BarRow& r, Rect anchor) {
        if (r.kind == "list" || r.kind == "placeholder") open_toolbar_list(r, anchor);
        else if (r.kind == "menu") open_overflow(r.children, anchor);  // a submenu drops down its rows
        else execute_command(core::wide(r.id));
    };
    title_toolbar_->on_overflow = [this](const std::vector<BarRow>& rows, Rect anchor) { open_overflow(rows, anchor); };
    title_toolbar_->on_context = [this](const BarRow& r, Rect anchor) { open_step_menu(r, anchor); };

    hwnd_ = window_.hwnd();
    window_.on_message = [this](UINT msg, WPARAM wp, LPARAM lp) { return on_window_message(msg, wp, lp); };
    // Paste is available while the clipboard holds files: told now, and again whenever it changes. A system
    // message, so it has its own hook - `on_message` only sees the app's own (WM_APP and up).
    window_.on_clipboard_changed = [this] { push_clipboard_flag(); };
    AddClipboardFormatListener(window_.hwnd());
    push_clipboard_flag();
    window_.root().on_preview_key = [this](const KeyEvent& e) { return on_preview_key(e); };
    bb_app_set_event_sink(core_.app(), &MainView::sink, this);
    // After the sink, so the event that says the read landed cannot arrive before anyone listens.
    sync_devices(false);  // starts reading the portable devices; their rows join the drives when it lands
    sync_wheel_speed();
    sync_icon_size();
}

MainView::~MainView() {
    // The tool windows' trees call into the core as they go, so they close while it is still alive.
    KillTimer(window_.hwnd(), kChangesTimer);
    window_.on_timer = nullptr;
    tools_.clear();
    RemoveClipboardFormatListener(window_.hwnd());
    window_.on_clipboard_changed = nullptr;
    bb_app_set_event_sink(core_.app(), nullptr, nullptr);
    window_.on_message = nullptr;
    window_.root().on_preview_key = nullptr;
}


void MainView::push_system_appearance() {
    DWORD light = 1, size = sizeof(light);
    RegGetValueW(HKEY_CURRENT_USER, L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize", L"AppsUseLightTheme",
                 RRF_RT_REG_DWORD, nullptr, &light, &size);
    HIGHCONTRASTW hc{};
    hc.cbSize = sizeof(hc);
    const bool high_contrast = SystemParametersInfoW(SPI_GETHIGHCONTRAST, sizeof(hc), &hc, 0) && (hc.dwFlags & HCF_HIGHCONTRASTON);
    bb_theme_set_system_appearance(core_.app(), light == 0, high_contrast);
}

void MainView::start(const std::wstring& path, bool restore_session) {
    bb_ui_set_flag(core_.app(), core::s(L"propertyProvider"), true);
    bb_set_property_host(core_.app(), make_property_host());
    if (follow_windows_) {
        push_system_appearance();
        window_.on_system_settings_changed = [this] { push_system_appearance(); };
    }
    apply_theme();
    uint64_t tab = 0;
    if (restore_session) core::check(bb_session_restore(core_.app(), core::s(path), &tab), "restore the session");
    else core::check(bb_app_initialize(core_.app(), core::s(path), &tab), "initialize");
    sync_layout();
    refresh_bars();
    sync_overlays();
}

bool MainView::listing_in_view() {
    TabView* tv = active_view();
    if (!tv || !tv->adapter().listing()) return false;
    try {
        const BbTabState st = core_.tab_state(active_tab_);
        return !st.is_loading && tv->adapter().listing()->version() == st.version;
    } catch (const core::CoreError&) {
        return false;
    }
}

void MainView::await_first_listing(DWORD budget_ms) {
    const ULONGLONG deadline = GetTickCount64() + budget_ms;
    const HWND hwnd = window_.hwnd();
    for (;;) {
        // Only the core's events: anything else would be the window running before it has been shown.
        MSG msg;
        while (PeekMessageW(&msg, hwnd, WM_BB_EVENT, WM_BB_EVENT, PM_REMOVE)) {
            on_window_message(msg.message, msg.wParam, msg.lParam);
        }
        if (listing_in_view()) return;
        const ULONGLONG now = GetTickCount64();
        if (now >= deadline) return;
        MsgWaitForMultipleObjectsEx(0, nullptr, static_cast<DWORD>(deadline - now), QS_POSTMESSAGE, 0);
    }
}

void MainView::apply_theme() {
    const std::vector<uint32_t> argb = core_.palette();
    if (argb.size() != gfx::tok::Count) {
        throw std::runtime_error("the core's palette has " + std::to_string(argb.size()) + " colours; the views expect " +
                                 std::to_string(gfx::tok::Count));
    }
    window_.root().resources().palette.set_colors(argb);
    auto rgb = [&](gfx::Token t) {
        const uint32_t v = argb[t];
        return RGB((v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF);
    };
    const uint32_t bg = argb[gfx::tok::WindowBackground];
    const int luma = (((bg >> 16) & 0xFF) * 299 + ((bg >> 8) & 0xFF) * 587 + (bg & 0xFF) * 114) / 1000;
    window_.set_frame_colors(luma < 128, rgb(gfx::tok::TitleBarBorder), rgb(gfx::tok::TitleBarBackground),
                             rgb(gfx::tok::TitleBarForeground));
    window_.root().invalidate_all();
    for (auto& kv : tools_) style_tool_window(*kv.second);
    // A different theme (picked in Settings, or Windows switching at sunset) is a different editor; an
    // override typed into it is the same one and must not rebuild it under a drag.
    // Spec: ThemeEditorWindow.WatchThemeChanges.
    if (tool_window(L"theme-editor")) {
        const std::string id = try_model([&](BbOwnedStr* o) { return bb_theme_state(core_.app(), o); })["id"].str();
        if (id != theme_editor_id_) defer([this] { refresh_tool(L"theme-editor"); });  // which records the new id
    } else {
        theme_editor_id_.clear();
    }
}

TabView* MainView::tab_view(uint64_t tab) {
    const auto it = by_tab_.find(tab);
    return it == by_tab_.end() ? nullptr : it->second;
}

size_t MainView::tab_count_in_active_pane() const {
    for (const PaneView* pv : pane_views_) {
        if (pv->pane() == active_pane_) return pv->strip->tabs().size();
    }
    return 0;
}

TabStrip* MainView::active_strip() const {
    for (const PaneView* pv : pane_views_) {
        if (pv->pane() == active_pane_) return pv->strip;
    }
    return nullptr;
}

std::vector<std::wstring> MainView::tab_titles_in_active_pane() const {
    std::vector<std::wstring> out;
    for (const PaneView* pv : pane_views_) {
        if (pv->pane() != active_pane_) continue;
        for (const TabEntry& t : pv->strip->tabs()) out.push_back(t.title);
    }
    return out;
}

std::vector<uint64_t> MainView::tab_ids_in_active_pane() const {
    std::vector<uint64_t> out;
    for (const PaneView* pv : pane_views_) {
        if (pv->pane() != active_pane_) continue;
        for (const TabEntry& t : pv->strip->tabs()) out.push_back(t.id);
    }
    return out;
}

std::vector<uint64_t> MainView::pane_ids() const {
    std::vector<uint64_t> out;
    for (const PaneView* pv : pane_views_) out.push_back(pv->pane());
    return out;
}

Widget* MainView::find_named(const std::string& name) {
    if (TabView* tv = active_view()) {
        if (tv->name == name) return tv;
        if (Widget* w = tv->find(name)) return w;
    }
    return window_.root().find(name);
}

void MainView::tab_navigated(uint64_t tab, const std::wstring& path) {
    if (tab == active_tab_) reveal_in_tree(path);
}

void MainView::reveal_in_tree(const std::wstring& path) {
    if (path == revealed_ || path.empty()) return;
    revealed_ = path;
    const std::vector<uint64_t> chain = tree_provider_->chain_to(path);
    if (chain.empty()) return;
    // A folder clicked in the tree stays where it was clicked; anything else lands 40% of the way down, with
    // its ancestors above it in view. Spec: MainWindow.ScrollTreeChainIntoView.
    const bool anchored = tree_->selected_id() == chain.back();
    revealing_ = true;
    for (size_t i = 0; i + 1 < chain.size(); ++i) tree_->expand(chain[i]);
    tree_->select(chain.back());
    if (!anchored) tree_->place(chain.back(), 0.4f);
    revealing_ = false;
}

// "Show hidden items" governs the tree as well as the file list. The tree re-filters in place, then reveals the
// active folder again: turning it on may expose the rest of the chain to a hidden folder already open.
void MainView::sync_tree_hidden() {
    BbOwnedStr o{};
    const bool on = bb_settings_get(core_.app(), core::s(L"showHiddenItems"), &o) == BB_OK && core::take(o) == L"true";
    if (!tree_provider_->set_show_hidden(on)) return;
    tree_->refresh();
    revealed_.clear();
    if (active_tab_) reveal_in_tree(core_.tab_text(active_tab_, BB_TEXT_PATH));
}

bool MainView::run_command(const std::wstring& id) {
    return bb_command_run(core_.app(), core::s(id)) == BB_OK;
}

void MainView::activate_tab(uint64_t tab) { bb_tab_activate(core_.app(), tab); }
void MainView::close_tab(uint64_t tab) { bb_tab_close(core_.app(), tab); }
void MainView::activate_pane(uint64_t pane) { bb_pane_activate(core_.app(), pane); }

void MainView::sync_index() {
    index_state_ = try_model([&](BbOwnedStr* o) { return bb_index_state(core_.app(), o); });
    if (index_state_.is_null()) return;
    // A volume that has just finished indexing is when folder sizes first exist: the tree fills them in
    // without being collapsed and opened again. Spec: FolderTreeViewModel.RefreshSizesAsync.
    const bool building = index_state_["building"].boolean();
    if (index_was_building_ && !building && tree_ && tree_provider_) {
        tree_provider_->refresh_sizes();
        tree_->invalidate();
    }
    index_was_building_ = building;
    const json::Value& b = index_state_["banner"];
    indexer_banner_->set_visible(b["show"].boolean());
    indexer_text_->set_text(core::wide(b["message"].str()));
    if (Widget* start = indexer_banner_->find("IndexerStart")) start->set_visible(b["canStart"].boolean());
    browser_->request_layout();
    refresh_status();
    sync_indexer_button();
}

bool MainView::pose_tab_drag(int gap) {
    for (PaneView* pv : pane_views_) {
        if (pv->pane() != active_pane_) continue;
        if (gap < 0 || static_cast<size_t>(gap) > pv->strip->tabs().size()) return false;
        pv->strip->set_drag_gap(gap);
        return true;
    }
    return false;
}

IndexerBannerInfo MainView::indexer_banner() const {
    const json::Value& b = index_state_["banner"];
    return {b["show"].boolean(), core::wide(b["message"].str()), b["canStart"].boolean()};
}

std::wstring MainView::indexing_status() const { return core::wide(index_state_["status"].str()); }
bool MainView::indexing_can_retry() const { return index_state_["canRetry"].boolean(); }

void MainView::sync_overlays() {
    sync_index();
    if (settings_) settings_->sync();
    if (palette_) palette_->sync();
    if (sidebar_) sidebar_->sync();
    sync_switchers();
    // Behind the Settings page the whole-PC search and the switchers would change the tab under the scrim; the
    // palette button stays live. Spec: MainWindow.xaml.cs (OpenSettings, CloseSettings: GlobalSearchGroup).
    const bool live = !(settings_ && settings_->is_open());
    if (title_search_ && title_search_->flags.enabled != live) {
        title_search_->set_enabled(live);
        title_search_->clear_button()->set_enabled(live);
        title_search_->save_button()->set_enabled(live);
        title_search_->glyph()->set_enabled(live);
        if (switchers_) {
            for (const auto& child : switchers_->children()) child->set_enabled(live);
        }
    }
}

void MainView::enable_file_drag_drop() {
    if (!file_drag_) file_drag_ = std::make_unique<FileDrag>(*this);
    file_drag_->enable();
}

ShellMenuSource* MainView::shell_source() {
    if (!shell_source_) shell_source_ = make_registry_shell_source(*this);
    return shell_source_.get();
}

void MainView::defer(std::function<void()> fn) {
    deferred_.push_back(std::move(fn));
    if (hwnd_.load()) PostMessageW(hwnd_.load(), WM_BB_DEFER, 0, 0);
}

void MainView::show_context_menu(MenuItems items, Point at) {
    floating_menu_ = std::make_unique<MenuItems>(std::move(items));
    if (floating_menu_->empty()) return;
    MenuOpen o;
    o.anchor = Rect{at.x, at.y, at.x, at.y};
    // The shell session behind the entries is let go after the menu has closed - and only the one this menu
    // opened with, since a right-click may have opened the next by then.
    const auto session = menu_session_;
    o.on_closed = [this, session](MenuClose) {
        defer([this, session] {
            if (menu_session_ == session) menu_session_.reset();
        });
    };
    popup_->open_menu(*floating_menu_, o);
}

bool MainView::preview_pending() const {
    for (const auto& kv : by_tab_) {
        if (kv.second->side_pending()) return true;
    }
    return false;
}

size_t MainView::thumbnails_pending() const {
    size_t n = 0;
    for (const auto& kv : by_tab_) n += kv.second->thumbnails_pending();
    return n;
}

void MainView::set_thumbnail_source(ThumbSource s) {
    thumb_source_ = s;
    for (auto& kv : by_tab_) kv.second->set_thumbnail_source(s);
}

// The portable devices under "This PC", read off the UI thread the first time they are asked for (and again on
// `refresh`). Spec: FolderTreeViewModel (LoadDevicesAsync).
void MainView::sync_devices(bool refresh) {
    if (!tree_provider_) return;
    const json::Value m = try_tool_call(*this, "devices.list", refresh ? "{\"refresh\":true}" : "{}");
    devices_pending_ = !m["loaded"].boolean();  // the core is reading them; Model::Devices says when it is done
    if (devices_pending_) return;
    std::vector<std::pair<std::wstring, std::wstring>> devices;
    for (size_t i = 0; i < m["devices"].size(); ++i) {
        devices.push_back({core::wide(m["devices"][i]["name"].str()), core::wide(m["devices"][i]["path"].str())});
    }
    if (tree_provider_->set_devices(devices) && tree_) tree_->refresh();
}

void MainView::open_device(const std::wstring& path) {
    const json::Value r = try_tool_call(*this, "devices.open", "{\"path\":" + json_quote(path) + "}");
    if (!r["ok"].boolean() && !r["error"].is_null()) bb_status_set(core_.app(), core::s(core::wide(r["error"].str())));
}

void MainView::sync_wheel_speed() {
    BbOwnedStr o{};
    if (bb_settings_get(core_.app(), core::s(L"scrollSpeedMultiplier"), &o) == BB_OK) ui::set_wheel_multiplier(_wtof(core::take(o).c_str()));
}

// The toolbar buttons and their pictures: the title bar toolbar, any rows of toolbars under it, and each
// folder header row all read the one size, so a change reaches every one of them.
void MainView::sync_icon_size() {
    BbOwnedStr o{};
    if (bb_settings_get(core_.app(), core::s(L"iconSize"), &o) != BB_OK) return;
    const ui::IconSize size = ui::parse_icon_size(json::parse(core::utf8(core::take(o))).str().c_str());
    if (size == ui::g_icon_size) return;
    ui::g_icon_size = size;
    const ui::IconMetrics m = ui::icon_metrics();
    title_->height = std::max(TitleBar::kHeight, m.bar);
    title_toolbar_->apply_icon_size();
    for (CommandToolbar* tb : row_toolbars_) tb->apply_icon_size();
    rows_panel_->height = static_cast<float>(row_toolbars_.size()) * m.bar;
    for (auto& kv : by_tab_) kv.second->apply_icon_size();
    window_.root().request_layout();
}

size_t MainView::tree_pending() const { return (tree_provider_ ? tree_provider_->pending() : 0) + (devices_pending_ ? 1 : 0); }

void MainView::refresh_tooltips() {
    for (auto& kv : by_tab_) kv.second->refresh_tooltips();
    BbApp* app = core_.app();
    if (Widget* palette = title_search_group_ ? title_search_group_->find("PaletteButton") : nullptr) {
        palette->tooltip = gesture_tip(app, L"Command palette \u2014 find any command by name", L"app.palette");
    }
    if (title_search_) title_search_->tooltip = gesture_tip(app, L"Search every indexed drive", L"search.pc");
}

void MainView::refresh_status() {
    if (!active_tab_) return;
    const std::wstring right = core::wide(index_state_["status"].str());
    status_->set_text(core_.tab_text(active_tab_, BB_TEXT_STATUS), right,
                      !right.empty() && index_state_["canRetry"].boolean() ? L"Try again" : L"");
    status_->undo_tip = gesture_tip(core_.app(), L"Show the undo history", L"edit.undo-history");
    // Undo's caption is what it would undo, and there is one only while there is something to undo.
    const json::Value undo = try_model([&](BbOwnedStr* o) { return bb_command_state(core_.app(), core::s(L"edit.undo"), o); });
    status_->set_extras(undo.is_null() ? std::wstring() : core::wide(undo["caption"].str()),
                        core_.tab_text(active_tab_, BB_TEXT_SELECTION));
    status_->set_scale(core_.tab_state(active_tab_).thumbnail_scale);
}


}  // namespace bb::views
