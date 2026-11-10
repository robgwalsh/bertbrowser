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

namespace {

// The edge of the side pane: a hairline that is also a handle. Dragging it resizes the pane; releasing it
// is when the width is remembered.
class SideGrip : public Widget {
public:
    SideGrip(std::function<void(float)> resized, std::function<void()> done, std::function<float()> right_edge)
        : resized_(std::move(resized)), done_(std::move(done)), right_edge_(std::move(right_edge)) {}
    void paint(PaintContext& pc) override {
        pc.fill(Rect{bounds.l, bounds.t, bounds.l + 1.0f, bounds.b}, tok::BorderDefault);
        if (dragging_ || hovered()) pc.fill(Rect{bounds.l, bounds.t, bounds.r, bounds.b}, tok::SplitterHoverBackground);
    }
    bool on_mouse_down(const MouseEvent& e) override {
        if (e.button != MouseButton::Left) return false;
        dragging_ = true;
        root()->capture(this);
        invalidate();
        return true;
    }
    bool on_mouse_move(const MouseEvent& e) override {
        if (!dragging_) return false;
        resized_(right_edge_() - e.pt.x);
        return true;
    }
    bool on_mouse_up(const MouseEvent&) override {
        if (!dragging_) return false;
        dragging_ = false;
        root()->release_capture();
        done_();
        invalidate();
        return true;
    }
    void on_mouse_enter() override { invalidate(); }
    void on_mouse_leave() override { invalidate(); }
    Cursor cursor_at(Point) const override { return Cursor::SizeWE; }

private:
    std::function<void(float)> resized_;
    std::function<void()> done_;
    std::function<float()> right_edge_;
    bool dragging_ = false;
};

}  // namespace

// ---- ListAdapter -------------------------------------------------------------------------------------

void ListAdapter::reset(std::shared_ptr<core::Listing> l) {
    listing_ = std::move(l);
    reload_selection();
    std::wstring path = core_.tab_text(tab_, BB_TEXT_PATH);
    if (path != checked_path_) {
        // A whole-PC search has no folder and its rows come from the index: real. Inside an archive the place
        // is the archive file itself, or a path under it that does not exist on disk.
        const DWORD attrs = path.empty() ? FILE_ATTRIBUTE_DIRECTORY : GetFileAttributesW(path.c_str());
        real_paths_ = attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY);
        checked_path_ = std::move(path);
    }
}

void ListAdapter::reload_selection() {
    sel_.assign(row_count(), 0);
    for (const uint32_t i : core_.selection(tab_)) {
        if (i < sel_.size()) sel_[i] = 1;
    }
}

uint64_t ListAdapter::row_id(size_t row) const {
    BbRow r{};
    return listing_ && listing_->row(row, r) ? r.row_id : row;
}

std::wstring ListAdapter::cell_text(size_t row, int column_id) const {
    if (!listing_ || column_id < 0 || static_cast<size_t>(column_id) >= cells_.size()) return {};
    const int cell = cells_[static_cast<size_t>(column_id)];
    return cell < 0 ? std::wstring() : listing_->cell(row, static_cast<uint32_t>(cell));
}

// Pictures and videos are tiles; folders and every other file are rows above them. Spec: ThumbnailTemplateSelector.
bool ListAdapter::row_is_tile(size_t row) const {
    BbRow r{};
    return listing_ && listing_->row(row, r) && (r.flags & BB_ROW_MEDIA) && !(r.flags & BB_ROW_DIRECTORY);
}

std::wstring ListAdapter::tile_row_text(size_t row, int part) const {
    auto index_of = [](const wchar_t* id) {
        int32_t i = -1;
        return bb_cell_index(core::s(id), &i) == BB_OK ? i : -1;
    };
    static const int size_cell = index_of(L"Size"), modified_cell = index_of(L"Modified");
    const int cell = part == 0 ? size_cell : modified_cell;
    return listing_ && cell >= 0 ? listing_->cell(row, static_cast<uint32_t>(cell)) : std::wstring();
}

std::wstring ListAdapter::cell_tip(size_t row, int column_id) const {
    return column_id == size_column_ && listing_ ? listing_->size_tip(row) : std::wstring();
}

bool ListAdapter::row_faded(size_t row) const {
    BbRow r{};
    return listing_ && listing_->row(row, r) && (r.flags & BB_ROW_HIDDEN);
}

bool ListAdapter::cell_marks(size_t row, int column_id, CellMarks* out) const {
    if (column_id != match_column_ || !listing_) return false;
    const int line = listing_->match(row, &out->start, &out->length);
    if (line <= 0) return false;
    out->lead = std::to_wstring(line);
    return true;
}

Token ListAdapter::row_tint(size_t row) const {
    BbRow r{};
    if (!listing_ || !listing_->row(row, r)) return gfx::kNoToken;
    // What a folder comparison says about the row, in the bits above the row flags.
    switch ((r.flags >> BB_ROW_COMPARE_SHIFT) & 7) {
        case 1: return tok::CompareUnknown;
        case 3: return tok::CompareOnlyHere;
        case 4: return tok::CompareNewer;
        case 5: return tok::CompareOlder;
        case 6: return tok::CompareDiffers;
        default: return gfx::kNoToken;
    }
}

ui::ThumbPtr ListAdapter::row_image(size_t row, uint32_t px, bool* faded) const {
    BbRow r{};
    if (!icons || !listing_ || !listing_->row(row, r)) return nullptr;
    if (faded) *faded = (r.flags & BB_ROW_HIDDEN) != 0;
    const bool dir = (r.flags & BB_ROW_DIRECTORY) != 0;
    // A virtual path never reaches the shell as a file: its type's icon only.
    return icons->file(core::w(r.full_path), dir, r.attributes, px, real_paths_);
}

const IconData* ListAdapter::row_icon(size_t row) const {
    BbRow r{};
    if (!listing_ || !listing_->row(row, r)) return nullptr;
    static const IconData* const folder = icons::find("Folder");
    static const IconData* const archive = icons::find("Archive");
    static const IconData* const file = icons::find("NewFile");
    if (r.flags & BB_ROW_DIRECTORY) return folder;
    const std::wstring name = core::w(r.name);
    for (const wchar_t* ext : {L".zip", L".7z", L".rar", L".tar", L".gz", L".tgz", L".bz2", L".xz"}) {
        const size_t n = wcslen(ext);
        if (name.size() > n && _wcsicmp(name.c_str() + name.size() - n, ext) == 0) return archive;
    }
    return file;
}

std::wstring ListAdapter::row_path(size_t row) const {
    BbRow r{};
    return listing_ && listing_->row(row, r) ? core::w(r.full_path) : std::wstring();
}

void ListAdapter::select_rows(const std::vector<size_t>& rows) {
    std::vector<uint32_t> idx;
    idx.reserve(rows.size());
    for (const size_t r : rows) idx.push_back(static_cast<uint32_t>(r));
    core_.select(tab_, BB_SELECT_REPLACE, idx);
    reload_selection();
}

void ListAdapter::select(SelectOp op, size_t first, size_t last) {
    if (op == SelectOp::Clear) {
        core_.select(tab_, BB_SELECT_CLEAR, {});
    } else {
        std::vector<uint32_t> idx;
        const size_t n = row_count();
        for (size_t i = first; i <= last && i < n; ++i) idx.push_back(static_cast<uint32_t>(i));
        core_.select(tab_, op == SelectOp::Replace ? BB_SELECT_REPLACE : op == SelectOp::Add ? BB_SELECT_ADD : BB_SELECT_TOGGLE,
                     idx);
    }
    reload_selection();
}

// ---- TabView -----------------------------------------------------------------------------------------

TabView::TabView(MainView& main, core::Core& core, uint64_t tab)
    : main_(main), core_(core), tab_(tab), adapter_(core, tab) {
    adapter_.icons = &main.shell_icons();
    style.bg = tok::ListBackground;
    setup_nav_bar();
    setup_banners();
    setup_body();
    wire_list_events();
}

// The row of buttons above the list: back, forward, up, refresh, the path, the folder search, and the
// buttons for flat view and the side pane.
void TabView::setup_nav_bar() {
    BbApp* app = core_.app();
    auto nav = std::make_unique<StackPanel>(Orientation::Horizontal, 2.0f);
    nav->pad = 4;
    nav->dock = Dock::Top;
    nav->name = "NavBar";
    nav->style.bg = tok::ToolbarBackground;
    nav_ = nav.get();
    // Drawn outlines, as the other toolbar buttons are; the tooltips name the key. Spec: DirectoryTabView.xaml.
    auto button = [&](const IconData& icon, std::function<void()> fn) {
        auto b = std::make_unique<Button>();
        b->icon = &icon;
        b->flat = true;  // like the title bar buttons: no box until the pointer is over it
        b->on_click = std::move(fn);
        return nav_->add(std::move(b));
    };
    back_ = button(icons::Back, [this, app] { bb_tab_back(app, tab_); });
    forward_ = button(icons::Forward, [this, app] { bb_tab_forward(app, tab_); });
    up_ = button(icons::Up, [this, app] { bb_tab_up(app, tab_); });
    refresh_ = button(icons::Refresh, [this, app] { bb_tab_refresh(app, tab_); });
    back_->name = "BackButton";
    forward_->name = "ForwardButton";
    up_->name = "UpButton";
    refresh_->name = "RefreshButton";

    path_bar_ = nav_->emplace<Panel>();
    path_bar_->flex = 1;
    path_bar_->name = "PathBar";
    crumbs_ = path_bar_->emplace<BreadcrumbBar>();
    crumbs_->name = "Breadcrumb";
    crumbs_->on_navigate = [this](const std::wstring& p) { core_.navigate(tab_, p); };
    // An ancestor middle-clicked opens in a pane of its own to the right. Spec: DirectoryTabView.Breadcrumb_PreviewMouseDown.
    crumbs_->on_middle = [app](const std::wstring& p) {
        uint64_t pane = 0;
        bb_pane_split(app, 0, BB_VERTICAL, core::s(p), &pane);
    };
    crumbs_->on_edit = [this] { begin_path_edit(); };
    path_ = path_bar_->emplace<PathEdit>();
    path_->name = "PathBox";
    path_->set_visible(false);
    path_->on_commit = [this] {
        // Trimmed, and an empty box goes nowhere rather than "Folder not found". Spec: DirectoryTabView.PathBox_KeyDown.
        std::wstring target = path_->text();
        const size_t a = target.find_first_not_of(L" \t"), b = target.find_last_not_of(L" \t");
        target = a == std::wstring::npos ? std::wstring() : target.substr(a, b - a + 1);
        end_path_edit();
        if (target.empty()) return;
        try {
            core_.navigate(tab_, target);
        } catch (const core::CoreError&) {
        }
    };
    path_->on_cancel = [this] { end_path_edit(); };
    path_->on_blur = [this] { end_path_edit(); };

    // The folder search starts as a button; a click on it opens the box. Spec: DirectoryTabView.xaml (SearchBox).
    search_button_ = nav_->emplace<Button>();
    search_button_->name = "SearchButton";
    search_button_->icon = &icons::Search;
    search_button_->flat = true;
    search_button_->on_click = [this] { reveal_search(true); };
    auto save = [this] { main_.execute_command(L"search.save"); };
    search_ = nav_->emplace<SearchField>([this] { clear_search(false); }, save);
    search_->set_visible(false);
    search_->on_blur = [this] { collapse_search(); };
    search_->name = "SearchBox";
    search_->width = 280;  // Spec: DirectoryTabView.xaml (Width 280, MinWidth 150)
    search_->placeholder = L"Search this folder";
    search_->on_changed = [this](const std::wstring&) { on_search_changed(false); };
    search_->on_cancel = [this] { search_escape(false); };
    search_->clear_button()->name = "SearchClearButton";
    search_->save_button()->name = "SaveSearchButton";
    // Flat view, lit while it is on. It comes before the side pane's buttons because it is about the list.
    // Spec: DirectoryTabView.xaml (FlatViewToggle).
    flat_button_ = nav_->emplace<Button>();
    flat_button_->name = "FlatViewToggle";
    flat_button_->flat = true;
    flat_button_->icon = &icons::FlatView;
    flat_button_->on_click = [this] { main_.execute_command(L"view.flat"); };
    {
        // Three buttons choose what the right-hand pane shows: nothing, the preview, or the metadata.
        const char* names[3] = {"SidePaneNone", "SidePanePreview", "SidePaneMetadata"};
        const char* icons_[3] = {"SidePaneNone", "PreviewPane", "Metadata"};
        for (int i = 0; i < 3; ++i) {
            auto* b = nav_->emplace<Button>();
            b->name = names[i];
            b->flat = true;
            b->icon = icons::find(icons_[i]);
            b->on_click = [this, app, i] { bb_tab_set_side_pane(app, tab_, static_cast<uint32_t>(i)); };
            side_buttons_[i] = b;
        }
    }
    add(std::move(nav));
    apply_icon_size();
}

// The activity strip and the two banners under the nav bar.
void TabView::setup_banners() {
    // Activity: a listing or a search on its way. A fixed 3px strip either way, so nothing below it moves
    // when it comes and goes. Spec: DirectoryTabView.xaml (the activity indicator).
    {
        auto strip = std::make_unique<Panel>();
        strip->name = "ActivityStrip";
        strip->dock = Dock::Top;
        strip->height = 3;
        activity_ = strip->emplace<ProgressBar>();
        activity_->name = "ActivityBar";
        activity_->set_visible(false);
        add(std::move(strip));
    }
    // Something true about the listing that it cannot say for itself (a flat view that stopped at its
    // ceiling). Not an error: the error banner below is for those. Spec: DirectoryTabView.xaml (NoticeBanner).
    {
        auto notice = std::make_unique<StackPanel>(Orientation::Horizontal, 8.0f);
        notice->name = "NoticeBanner";
        notice->dock = Dock::Top;
        notice->pad = 6;
        notice->style.bg = tok::WarningBackground;
        notice_ = notice->emplace<TextBlock>();
        notice_->style.fg = tok::WarningForeground;
        notice_->wrap = true;
        notice_->flex = 1;
        notice->set_visible(false);
        notice_panel_ = add(std::move(notice));
    }

    auto banner = std::make_unique<StackPanel>(Orientation::Horizontal, 8.0f);
    banner->pad = 6;  // grows with what it says: an access-denied message is long, and a pane can be narrow
    banner->dock = Dock::Top;
    banner->name = "ErrorBanner";
    // A locked archive is a listing state, so Unlock rides in this banner rather than a dialog of its
    // own. Spec: DirectoryTabView.xaml (the error banner).
    banner->style.bg = tok::WarningBackground;
    banner_ = banner->emplace<TextBlock>();
    banner_->style.fg = tok::WarningForeground;
    banner_->wrap = true;
    banner_->vcenter = true;
    banner_->flex = 1;
    unlock_ = banner->emplace<Button>(L"Unlock…");
    unlock_->name = "UnlockArchive";
    unlock_->width = 90;
    unlock_->set_visible(false);
    unlock_->on_click = [this] { main_.open_dialog(L"archive-password"); };
    banner->set_visible(false);
    banner_panel_ = add(std::move(banner));
}

// The side pane at the right, and the list and tile views beside it.
void TabView::setup_body() {
    BbApp* app = core_.app();
    auto body = std::make_unique<DockPanel>();
    body->dock = Dock::Fill;
    body_ = add(std::move(body));
    // The side pane sits at the right, shown for the preview and the metadata choices. Its left edge is a
    // grip: dragged, it resizes the pane, and the width it ends on is the one new tabs start from.
    {
        double saved = 0;
        BbOwnedStr o{};
        if (bb_settings_get(app, core::s(L"previewPaneWidth"), &o) == BB_OK) saved = _wtof(core::take(o).c_str());
        if (saved > 0) side_width_ = static_cast<float>(std::clamp(saved, 180.0, 1200.0));
    }
    auto* side = body_->emplace<DockPanel>();
    side->name = "SidePane";
    side->dock = Dock::Right;
    side->width = side_width_;
    side->style.bg = tok::SurfaceBackground;
    side->set_visible(false);
    side_pane_ = side;
    {
        auto* grip = side->emplace<SideGrip>(
            [this](float wanted) {
                side_width_ = std::clamp(wanted, 180.0f, std::max(180.0f, body_->bounds.width() * 0.9f));
                side_pane_->width = side_width_;
                body_->request_layout();
            },
            [this, app] {
                char buf[32];
                snprintf(buf, sizeof(buf), "%.0f", side_width_);
                bb_settings_set(app, core::s(L"previewPaneWidth"), core::s(core::wide(buf)));
            },
            [this] { return side_pane_->bounds.r; });
        grip->name = "SideGrip";
        grip->dock = Dock::Left;
        grip->width = 5;
    }
    view_area_ = body_->emplace<Panel>();
    view_area_->dock = Dock::Fill;
    list_ = view_area_->emplace<ListView>();
    list_->name = "FileListView";
    list_->set_provider(&adapter_);
    tiles_ = view_area_->emplace<TileView>();
    tiles_->name = "FileTileView";
    tiles_->set_provider(&adapter_);
    tiles_->set_visible(false);
    empty_ = view_area_->emplace<Overlay>();
    empty_->name = "EmptyMessage";
    empty_->set_visible(false);
}

// What the list and the tiles do with the pointer and the keyboard.
void TabView::wire_list_events() {
    BbApp* app = core_.app();
    list_->on_header_menu = [this](Point p) { open_context(p, true); };
    list_->on_drag_start = [this](MouseButton b) {
        if (FileDrag* d = main_.file_drag()) d->start(tab_, b);
    };
    list_->on_context = [this](Point p, size_t row) { open_context(p, false, row == static_cast<size_t>(-1)); };
    tiles_->on_activate = [this](size_t row) { open_row(row); };
    // The tiles are the same rows as the list, and answer the same gestures the same way.
    tiles_->on_context = [this](Point p, size_t row) { open_context(p, false, row == TileView::npos); };
    tiles_->on_drag_start = [this](MouseButton b) {
        if (FileDrag* d = main_.file_drag()) d->start(tab_, b);
    };

    list_->on_sort = [this, app](int column, bool) {
        if (column < 0 || static_cast<size_t>(column) >= columns_.size()) return;
        bb_tab_set_sort(app, tab_, core::s(columns_[static_cast<size_t>(column)].id));
    };
    // A header drag or a gripper is the person arranging this tab's columns: the order and the widths go to
    // the core, which keeps them for the tab (and the session) and marks the tab as arranged by hand.
    // Spec: DirectoryTabView (ColumnsChanged -> FileList.ColumnLayout).
    list_->on_columns_changed = [this, app] {
        std::wstring order;
        std::vector<std::pair<std::wstring, float>> widths;
        for (const Column& c : list_->columns()) {
            if (c.id < 0 || static_cast<size_t>(c.id) >= columns_.size()) continue;
            const core::ColumnInfo& info = columns_[static_cast<size_t>(c.id)];
            order += (order.empty() ? L"" : L",") + info.id;
            if (std::fabs(c.width - info.width) >= 0.5f) widths.push_back({info.id, c.width});
        }
        bb_columns_do(app, tab_, core::s(L"order"), core::s(order));
        for (const auto& [id, w] : widths) bb_columns_do(app, tab_, core::s(L"width"), core::s(id + L" " + std::to_wstring(static_cast<int>(std::lround(w)))));
    };
    list_->on_activate = [this](size_t row) { open_row(row); };
    // A middle-clicked folder opens in a pane of its own to the right; a file is left alone.
    // Spec: DirectoryTabView.FileList_PreviewMouseDown (middle).
    list_->on_middle = [this, app](size_t row) {
        const auto listing = core_.listing(tab_);
        BbRow r{};
        if (!listing || !listing->row(static_cast<uint32_t>(row), r) || !(r.flags & BB_ROW_DIRECTORY)) return;
        uint64_t pane = 0;
        bb_pane_split(app, 0, BB_VERTICAL, core::s(core::w(r.full_path)), &pane);
    };
    list_->on_typeahead = [this](const std::wstring& typed) { type_ahead(typed); };
    tiles_->on_typeahead = [this](const std::wstring& typed) { type_ahead(typed); };
    tiles_->on_middle = list_->on_middle;
}

// A double click or Enter. Explorer's convention: with Ctrl+Shift held, opening a file runs it as
// administrator - the row clicked, not the selection, which Ctrl+Shift has just range-extended.
// Spec: DirectoryTabView.FileList_DoubleClick (RunAsAdminHeld).
void TabView::open_row(size_t row) {
    const bool ctrl = (GetKeyState(VK_CONTROL) & 0x8000) != 0, shift = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
    const bool alt = (GetKeyState(VK_MENU) & 0x8000) != 0;
    if (ctrl && shift && !alt) {
        const auto listing = core_.listing(tab_);
        BbRow r{};
        if (listing && listing->row(static_cast<uint32_t>(row), r) && !(r.flags & BB_ROW_DIRECTORY)) {
            core_.select(tab_, BB_SELECT_REPLACE, {static_cast<uint32_t>(row)});
            adapter_.reload_selection();
            main_.execute_command(L"file.run-as-admin");
            return;
        }
    }
    bb_tab_open_item(core_.app(), tab_, static_cast<uint32_t>(row));
}

void TabView::type_ahead(const std::wstring& typed) {
    if (typed.empty()) return;
    const std::wstring key(1, typed.back());
    const size_t cursor = tiles_on_ ? tiles_->cursor() : list_->cursor();
    // Extending the prefix: keep the row if it still matches, else search on from it.
    if (typed.size() > 1 && !typeahead_prefix_.empty() &&
        select_by_prefix(typeahead_prefix_ + key, cursor == ListView::npos ? 0 : cursor)) {
        typeahead_prefix_ += key;
        return;
    }
    // A fresh letter, or a broken prefix: a one-letter jump past the current row, so repeating it cycles.
    if (select_by_prefix(key, cursor == ListView::npos ? 0 : cursor + 1)) typeahead_prefix_ = key;
}

bool TabView::select_by_prefix(const std::wstring& prefix, size_t start) {
    const auto& l = adapter_.listing();
    if (!l || l->size() == 0) return false;
    const size_t n = l->size();
    for (size_t k = 0; k < n; ++k) {
        const size_t i = (start + k) % n;
        if (core::iequals(l->name(i).substr(0, prefix.size()), prefix)) {
            if (tiles_on_) tiles_->select_row(i);
            else list_->select_row(i);
            return true;
        }
    }
    return false;
}

void TabView::sync_columns() {
    const auto cols = core_.columns(tab_);
    std::vector<int> cells;
    int match = -1, size = -1;
    for (size_t i = 0; i < cols.size(); ++i) {
        cells.push_back(cols[i].cell);
        if (cols[i].id == L"Match") match = static_cast<int>(i);
        if (cols[i].id == L"Size") size = static_cast<int>(i);
    }
    adapter_.set_cells(cells);
    adapter_.set_match_column(match);
    adapter_.set_size_column(size);
    bool same = cols.size() == columns_.size();
    for (size_t i = 0; same && i < cols.size(); ++i) same = cols[i].id == columns_[i].id;
    columns_ = cols;
    if (same) return;
    std::vector<Column> out;
    for (size_t i = 0; i < cols.size(); ++i) {
        Column c;
        c.id = static_cast<int>(i);
        c.title = cols[i].header;
        c.width = cols[i].width;
        c.right_align = cols[i].right;
        c.sortable = cols[i].sortable;
        // Where a result lives and what a comparison says in words are second to the name. Spec: Styles.xaml
        // (Column.Cell.RelativePath at 0.7, Column.Cell.CompareStatus in the secondary colour).
        if (cols[i].id == L"RelativePath" || cols[i].id == L"CompareStatus") c.fg = tok::TextSecondary;
        out.push_back(std::move(c));
    }
    list_->set_columns(std::move(out));
}

TabView::~TabView() {
    thumbs_.reset();
    // Gone before its pane: the pane's worker posts here, and a full-screen video lent to the window comes home.
    if (preview_) preview_->set_active(false);
}

// The file list keeps at least a tenth of the pane; the remembered width is untouched, so a wider window gives it back.
void TabView::arrange(Rect r) {
    if (side_pane_) {
        const float total = r.width();
        side_pane_->width = std::min(side_width_, std::max(180.0f, total * 0.9f));
    }
    DockPanel::arrange(r);
}

Edit* TabView::global_search_box() const { return main_.title_search(); }

void TabView::reload_columns() {
    sync_columns();
    list_->invalidate();
}

std::vector<std::wstring> TabView::column_ids() const {
    std::vector<std::wstring> out;
    for (const auto& c : columns_) out.push_back(c.id);
    return out;
}

std::wstring TabView::cell_by_id(size_t row, const std::wstring& column_id) const {
    for (size_t i = 0; i < columns_.size(); ++i) {
        if (core::iequals(columns_[i].id, column_id)) return adapter_.cell_text(row, static_cast<int>(i));
    }
    return {};
}

size_t TabView::realized_rows() const {
    if (tiles_on_) {
        const auto r = tiles_->realized();
        return r.second - r.first;
    }
    const auto v = list_->visible_range();
    return v.second - v.first;
}

void TabView::scroll_to_offset(float y) {
    if (tiles_on_) tiles_->set_scroll_y(y);
    else list_->set_scroll_y(y);
}

void TabView::reveal_row(size_t row) {
    core_.select(tab_, BB_SELECT_REPLACE, {static_cast<uint32_t>(row)});
    adapter_.reload_selection();
    if (tiles_on_) tiles_->scroll_to_row(row);
    else list_->scroll_to_row(row);
    list_->invalidate();
    tiles_->invalidate();
}

void TabView::set_thumbnail_source(ThumbSource s) {
    thumb_source_ = s;
    thumbs_.reset();
    tiles_->set_thumbnails(nullptr);
}

void TabView::drain_thumbnails() {
    if (thumbs_) thumbs_->drain();
}

Widget* TabView::active_view() const { return tiles_on_ ? static_cast<Widget*>(tiles_) : static_cast<Widget*>(list_); }


}  // namespace bb::views
