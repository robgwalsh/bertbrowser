#include "views/tool_dialogs.h"
#include "views/tool_state.h"
#include "views/main_view.h"

#include "gfx/icons.gen.h"
#include "views/folder_tree.h"
#include "views/media_pane.h"
#include "views/ops.h"
#include "views/ops_card.h"
#include "views/ops_menus.h"
#include "views/property_host.h"
#include "views/shell_props.h"
#include "views/tool_widgets.h"

#include <dwmapi.h>
#include <shobjidl.h>

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace bb::ui;

namespace {

// A split whose first child gets `fraction` of the space the first time it is laid out; after that the
// user's drag is the truth.
class WeightedSplitter : public Splitter {
public:
    WeightedSplitter(Orientation o, float fraction) : Splitter(o), fraction_(fraction) {}
    void arrange(Rect r) override {
        if (!placed_ && !r.empty()) {
            const float total = orientation == Orientation::Horizontal ? r.width() : r.height();
            set_position(fraction_ * (total - thickness));
            placed_ = true;
        }
        Splitter::arrange(r);
    }

private:
    float fraction_;
    bool placed_ = false;
};

std::wstring key_name(unsigned vk) {
    if ((vk >= 'A' && vk <= 'Z') || (vk >= '0' && vk <= '9')) return std::wstring(1, static_cast<wchar_t>(vk));
    if (vk >= VK_F1 && vk <= VK_F24) return L"F" + std::to_wstring(vk - VK_F1 + 1);
    if (vk >= VK_NUMPAD0 && vk <= VK_NUMPAD9) return L"Num" + std::to_wstring(vk - VK_NUMPAD0);
    switch (vk) {
        case VK_BACK: return L"Backspace";
        case VK_DELETE: return L"Del";
        case VK_RETURN: return L"Enter";
        case VK_ESCAPE: return L"Esc";
        case VK_TAB: return L"Tab";
        case VK_SPACE: return L"Space";
        case VK_LEFT: return L"Left";
        case VK_RIGHT: return L"Right";
        case VK_UP: return L"Up";
        case VK_DOWN: return L"Down";
        case VK_HOME: return L"Home";
        case VK_END: return L"End";
        case VK_PRIOR: return L"PageUp";
        case VK_NEXT: return L"PageDown";
        case VK_INSERT: return L"Insert";
        case VK_ADD: return L"Num+";
        case VK_SUBTRACT: return L"Num-";
        case VK_MULTIPLY: return L"Num*";
        case VK_DIVIDE: return L"Num/";
        case VK_DECIMAL: return L"Num.";
        case VK_OEM_COMMA: return L",";
        case VK_OEM_PERIOD: return L".";
        case VK_OEM_2: return L"/";
        case VK_OEM_1: return L";";
        case VK_OEM_7: return L"'";
        case VK_OEM_4: return L"[";
        case VK_OEM_6: return L"]";
        case VK_OEM_5: return L"\\";
        case VK_OEM_MINUS: return L"-";
        case VK_OEM_PLUS: return L"=";
        case VK_OEM_3: return L"`";
        default: return L"";
    }
}

const json::Value& pane_state(const json::Value& panes, uint64_t id) {
    static const json::Value none;
    for (size_t i = 0; i < panes.size(); ++i) {
        if (static_cast<uint64_t>(panes[i]["id"].i64()) == id) return panes[i];
    }
    return none;
}

std::string signature(const json::Value& node, const json::Value& panes) {
    if (node["kind"].str() == "pane") {
        const uint64_t id = static_cast<uint64_t>(node["pane"].i64());
        const json::Value& p = pane_state(panes, id);
        std::string s = "P" + std::to_string(id) + "[";
        for (size_t i = 0; i < p["tabs"].size(); ++i) s += std::to_string(p["tabs"][i].i64()) + ",";
        return s + "|" + std::to_string(p["activeTab"].i64(-1)) + "]";
    }
    std::string s = "S" + node["orientation"].str() + "(";
    const json::Value& kids = node["children"];
    for (size_t i = 0; i < kids.size(); ++i) s += signature(kids[i], panes) + ";";
    return s + ")";
}

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

std::wstring chord_key_name(unsigned vk) { return key_name(vk); }

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
    BbApp* app = core.app();

    auto nav = std::make_unique<StackPanel>(Orientation::Horizontal, 6.0f);
    nav->pad = 6;
    nav->height = 42;
    nav->dock = Dock::Top;
    nav->name = "NavBar";
    nav->style.bg = tok::ToolbarBackground;
    nav_ = nav.get();
    // Drawn outlines, as the other toolbar buttons are; the tooltips name the key. Spec: DirectoryTabView.xaml.
    auto button = [&](const IconData& icon, std::function<void()> fn) {
        auto b = std::make_unique<Button>();
        b->icon = &icon;
        b->width = 34;
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
        const std::wstring target = path_->text();
        end_path_edit();
        try {
            core_.navigate(tab_, target);
        } catch (const core::CoreError&) {
        }
    };
    path_->on_cancel = [this] { end_path_edit(); };
    path_->on_blur = [this] { end_path_edit(); };

    auto save = [this] { main_.execute_command(L"search.save"); };
    search_ = nav_->emplace<SearchField>([this] { clear_search(false); }, save);
    search_->name = "SearchBox";
    search_->width = 170;
    search_->placeholder = L"Search this folder";
    search_->on_changed = [this](const std::wstring&) { on_search_changed(false); };
    search_->on_cancel = [this] { search_escape(false); };
    search_->clear_button()->name = "SearchClearButton";
    search_->save_button()->name = "SaveSearchButton";
    // Flat view, lit while it is on. It comes before the side pane's buttons because it is about the list.
    // Spec: DirectoryTabView.xaml (FlatViewToggle).
    flat_button_ = nav_->emplace<Button>();
    flat_button_->name = "FlatViewToggle";
    flat_button_->width = 34;
    flat_button_->icon = &icons::FlatView;
    flat_button_->on_click = [this] { main_.execute_command(L"view.flat"); };
    {
        // Three buttons choose what the right-hand pane shows: nothing, the preview, or the metadata.
        const char* names[3] = {"SidePaneNone", "SidePanePreview", "SidePaneMetadata"};
        const char* icons_[3] = {"SidePaneNone", "PreviewPane", "Metadata"};
        for (int i = 0; i < 3; ++i) {
            auto* b = nav_->emplace<Button>();
            b->name = names[i];
            b->width = 34;
            b->icon = icons::find(icons_[i]);
            b->on_click = [this, app, i] { bb_tab_set_side_pane(app, tab_, static_cast<uint32_t>(i)); };
            side_buttons_[i] = b;
        }
    }
    add(std::move(nav));

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
    banner->pad = 4;
    banner->height = 28;
    banner->dock = Dock::Top;
    banner->name = "ErrorBanner";
    // A locked archive is a listing state, so Unlock rides in this banner rather than a dialog of its
    // own. Spec: DirectoryTabView.xaml (the error banner).
    banner->style.bg = tok::WarningBackground;
    banner_ = banner->emplace<TextBlock>();
    banner_->style.fg = tok::WarningForeground;
    banner_->vcenter = true;
    banner_->flex = 1;
    unlock_ = banner->emplace<Button>(L"Unlock…");
    unlock_->name = "UnlockArchive";
    unlock_->width = 90;
    unlock_->set_visible(false);
    unlock_->on_click = [this] { main_.open_dialog(L"archive-password"); };
    banner->set_visible(false);
    banner_panel_ = add(std::move(banner));

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
                side_width_ = std::clamp(wanted, 180.0f, std::max(180.0f, body_->bounds.width() - 140.0f));
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
    for (const auto& c : cols) cells.push_back(c.cell);
    adapter_.set_cells(cells);
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
        out.push_back(std::move(c));
    }
    list_->set_columns(std::move(out));
}

TabView::~TabView() {
    thumbs_.reset();
    // Gone before its pane: the pane's worker posts here, and a full-screen video lent to the window comes home.
    if (preview_) preview_->set_active(false);
}

void TabView::arrange(Rect r) { DockPanel::arrange(r); }

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

void TabView::sync_view_mode(const BbTabState& st) {
    const bool want = st.thumbnail_scale > 0.0;
    const float image = 64.0f + static_cast<float>(st.thumbnail_scale) * 160.0f;
    if (want && !thumbs_) {
        thumbs_ = std::make_unique<ThumbnailService>(main_.window().hwnd(), thumb_source_, [this](size_t row) { return adapter_.row_path(row); });
        tiles_->set_thumbnails(thumbs_.get());
        tiles_->dpi_scale = static_cast<float>(main_.window().dpi()) / 96.0f;
    }
    if (want && std::fabs(tiles_->metrics.image - image) > 0.5f) {
        tiles_->metrics.image = image;
        tiles_->refresh();
    }
    if (want == tiles_on_) return;
    tiles_on_ = want;
    list_->set_visible(!want);
    tiles_->set_visible(want);
    if (want) tiles_->refresh();
    else list_->refresh();
    view_area_->request_layout();
}

PreviewPane* TabView::ensure_preview() {
    if (preview_) return preview_;
    preview_ = side_pane_->emplace<PreviewPane>(core_, tab_, main_.window().hwnd());
    preview_->on_fit_width = [this] { fit_side_width(); };
    preview_->on_next_media = [this] { select_next_media(); };
    return preview_;
}

MetadataPane* TabView::ensure_metadata() {
    if (metadata_) return metadata_;
    metadata_ = side_pane_->emplace<MetadataPane>(core_, tab_);
    return metadata_;
}

void TabView::sync_side_pane(const BbTabState& st) {
    side_mode_ = static_cast<int>(st.side_pane);
    side_pane_->set_visible(side_mode_ != 0);
    for (int i = 0; i < 3; ++i) side_buttons_[i]->primary = i == side_mode_;
    flat_button_->primary = st.is_flat_browse;

    // Built the first time it is shown, not declared with the tab: most tabs never show one.
    if (side_mode_ == 1) ensure_preview();
    if (side_mode_ == 2) ensure_metadata();
    if (preview_) preview_->set_visible(side_mode_ == 1);
    if (metadata_) metadata_->set_visible(side_mode_ == 2);
    if (preview_) {
        preview_->set_active(side_mode_ == 1);
        if (side_mode_ == 1) {
            preview_->apply_view_state(static_cast<int>(st.preview_mode), st.preview_wrap, st.preview_fit);
            preview_->show();
        }
    }
    if (metadata_) {
        metadata_->set_active(side_mode_ == 2);
        if (side_mode_ == 2) metadata_->follow_selection(false);
    }
    // What the commands that act on a playing video ask before they say yes.
    bb_ui_set_flag(core_.app(), core::s(L"previewHasMedia"), preview_ && side_mode_ == 1 && preview_->state_name() == "media");
}

void TabView::fit_side_width() {
    if (!preview_) return;
    const float wanted = preview_->desired_width() + 16.0f;
    side_width_ = std::clamp(wanted, 180.0f, std::max(180.0f, body_->bounds.width() - 140.0f));
    side_pane_->width = side_width_;
    body_->request_layout();
}

void TabView::select_next_media() {
    int64_t row = -1;
    if (bb_preview_next_media(core_.app(), tab_, &row) != BB_OK || row < 0) return;
    const auto& listing = adapter_.listing();
    BbRow r{};
    if (!listing || !listing->row(static_cast<size_t>(row), r)) return;
    // The file that arrives is the one that plays: a path, not a flag, so a selection the user makes in the
    // meantime is never auto-played.
    if (preview_) preview_->play_when_loaded(core::w(r.full_path));
    reveal_row(static_cast<size_t>(row));
}

// "Back (Alt+Left)": what the button does and, when it has one, the key. Spec: Cmd.Tip.
static std::wstring gesture_tip(BbApp* app, const std::wstring& text, const wchar_t* command, const std::wstring& after = {}) {
    BbOwnedStr o{};
    const std::wstring g = bb_keymap_gesture(app, core::s(command), &o) == BB_OK ? core::take(o) : std::wstring();
    return text + (g.empty() ? std::wstring() : L" (" + g + L")") + after;
}

void TabView::refresh_tooltips() {
    BbApp* app = core_.app();
    back_->tooltip = gesture_tip(app, L"Back", L"nav.back");
    forward_->tooltip = gesture_tip(app, L"Forward", L"nav.forward");
    up_->tooltip = gesture_tip(app, L"Up", L"nav.up");
    refresh_->tooltip = gesture_tip(app, L"Refresh", L"nav.refresh");
    search_->tooltip = gesture_tip(app, L"Search this folder and everything under it", L"search.folder",
                                   L". Filters: ext: size: dm: path: is: re: in: content:, plus OR and !");
    static const wchar_t* ids[3] = {L"view.side-none", L"view.preview", L"view.metadata"};
    static const wchar_t* names[3] = {L"No side pane", L"Preview pane", L"Metadata pane"};
    for (int i = 0; i < 3; ++i) {
        BbOwnedStr o{};
        std::wstring gesture;
        if (bb_keymap_gesture(core_.app(), core::s(ids[i]), &o) == BB_OK) gesture = core::take(o);
        side_buttons_[i]->tooltip = names[i] + (gesture.empty() ? std::wstring() : L" (" + gesture + L")");
    }
    BbOwnedStr o{};
    const std::wstring flat = bb_keymap_gesture(core_.app(), core::s(L"view.flat"), &o) == BB_OK ? core::take(o) : std::wstring();
    flat_button_->tooltip = L"Flat view — everything under this folder" + (flat.empty() ? std::wstring() : L" (" + flat + L")");
}

void TabView::open_context(Point p, bool header, bool background) {
    main_.show_context_menu(header ? build_column_menu() : build_files_menu(background), p);
}

MenuItems TabView::build_files_menu(bool background) {
    // Composed per opening: the app's entries, the user's commands and each extension's, in the saved order.
    ops::ComposedMenu composed = ops::compose_context_menu(main_, background ? ops::MenuKind::Background : ops::MenuKind::Files);
    main_.keep_menu_session(std::move(composed.keep));
    return std::move(composed.items);
}

// The header strip's checklist: the built-ins in catalogue order, the curated properties by group, anything
// else showing, then More columns, Set as default for new tabs and Reset. Spec: ColumnHeaderMenu.
MenuItems TabView::build_column_menu() {
    BbApp* app = core_.app();
    const uint64_t tab = tab_;
    const json::Value m = try_model([&](BbOwnedStr* o) { return bb_columns_header_menu(app, tab, o); });
    auto entry = [app, tab](const json::Value& c) {
        MenuItem item = MenuItem::make(escape_caption(core::wide(c["header"].str())));
        item.checked = c["on"].boolean();
        // Name cannot be turned off; disabled rather than absent, so the reason is visible.
        item.enabled = !c["locked"].boolean();
        const std::wstring id = core::wide(c["id"].str());
        item.on_invoke = [app, tab, id, on = !item.checked] { bb_columns_do(app, tab, core::s(on ? L"add" : L"remove"), core::s(id)); };
        return item;
    };
    MenuItems out;
    for (size_t i = 0; i < m["builtIns"].size(); ++i) out.push_back(entry(m["builtIns"][i]));
    out.push_back(MenuItem::separator());
    for (size_t g = 0; g < m["groups"].size(); ++g) {
        MenuItem group = MenuItem::make(escape_caption(core::wide(m["groups"][g]["title"].str())));
        const json::Value& items = m["groups"][g]["items"];
        for (size_t i = 0; i < items.size(); ++i) group.sub.push_back(entry(items[i]));
        out.push_back(std::move(group));
    }
    if (m["extra"].size() > 0) {
        out.push_back(MenuItem::separator());
        for (size_t i = 0; i < m["extra"].size(); ++i) out.push_back(entry(m["extra"][i]));
    }
    out.push_back(MenuItem::separator());
    out.push_back(MenuItem::make(L"More columns\u2026", [this] { main_.open_dialog(L"columns"); }));
    out.push_back(MenuItem::make(L"Set as default for new tabs", [app, tab] { bb_columns_do(app, tab, core::s(L"default"), core::s(L"")); }));
    out.push_back(MenuItem::make(L"Reset to default", [app, tab] { bb_columns_do(app, tab, core::s(L"reset"), core::s(L"")); }));
    return out;
}

void TabView::reload_listing() {
    adapter_.reset(core_.listing(tab_));
    sync_columns();
    list_->refresh();
    tiles_->refresh();
    const bool none = adapter_.row_count() == 0;
    std::wstring message = none ? core_.tab_text(tab_, BB_TEXT_EMPTY) : std::wstring();
    empty_->set_message(std::move(message));
    try {
        apply_reveal(core_.tab_state(tab_));
    } catch (const core::CoreError&) {
    }
}

// Something the core selected for the person (a renamed or new item landing) is brought into view with the
// cursor on it, once - kept until the listing it is a row of has reached this view, since the two events can
// arrive either way round. Spec: DirectoryTabView (ScrollIntoView after SelectPaths).
void TabView::apply_reveal(const BbTabState& st) {
    if (st.reveal_seq == reveal_seq_) return;
    const auto& listing = adapter_.listing();
    if (st.reveal_row < 0) {
        reveal_seq_ = st.reveal_seq;
    } else if (listing && listing->version() == st.version && static_cast<size_t>(st.reveal_row) < listing->size()) {
        reveal_seq_ = st.reveal_seq;
        const size_t row = static_cast<size_t>(st.reveal_row);
        if (tiles_on_) tiles_->reveal_cursor(row);
        else list_->reveal_cursor(row);
    }
}

void TabView::reload_state() {
    BbTabState st{};
    try {
        st = core_.tab_state(tab_);
    } catch (const core::CoreError&) {
        return;
    }
    back_->set_enabled(st.can_back);
    forward_->set_enabled(st.can_forward);
    up_->set_enabled(st.can_up);

    const std::wstring path = core_.tab_text(tab_, BB_TEXT_PATH);
    if (path != path_text_) {
        path_text_ = path;
        crumbs_->set_crumbs(core_.breadcrumb(tab_));
        if (!path_->flags.visible) path_->set_text(path);
        list_->set_provider(&adapter_);  // a different place: back to the top, no cursor
        tiles_->set_provider(&adapter_);
        main_.tab_navigated(tab_, path);
    }

    // The box that holds the active search shows it; typing in the other one is left alone. The whole-PC
    // box is the title bar's and belongs to whichever tab is active.
    SearchField* gbox = main_.active_tab() == tab_ ? main_.title_search() : nullptr;
    if (!search_->focused() && !(gbox && gbox->focused())) {
        const std::wstring text = core_.tab_text(tab_, BB_TEXT_SEARCH);
        suppress_ = true;
        const std::wstring local = st.is_global_search ? std::wstring() : text;
        if (search_->text() != local) search_->set_text(local);
        suppress_ = false;
        if (gbox) main_.set_title_search_text(st.is_global_search ? text : std::wstring());
    }
    search_->sync_buttons();

    const std::wstring error = core_.tab_text(tab_, BB_TEXT_ERROR);
    banner_->set_text(error);
    banner_panel_->set_visible(!error.empty());
    const std::wstring notice = core_.tab_text(tab_, BB_TEXT_NOTICE);
    if (notice_->text() != notice) notice_->set_text(notice);
    notice_panel_->set_visible(!notice.empty());
    // Indeterminate only while shown: its animation timer runs for as long as it is.
    const bool busy = st.is_loading || st.is_search_running;
    activity_->set_indeterminate(busy);
    activity_->set_visible(busy);
    unlock_->set_visible(!error.empty() && st.is_archive_locked);

    adapter_.reload_selection();
    const std::wstring sort = core_.tab_text(tab_, BB_TEXT_SORT_BY);
    for (size_t i = 0; i < columns_.size(); ++i) {
        if (columns_[i].id == sort) list_->set_sort(static_cast<int>(i), !st.sort_descending);
    }
    sync_view_mode(st);
    sync_side_pane(st);
    refresh_tooltips();
    apply_reveal(st);
    list_->invalidate();
    tiles_->invalidate();
}

void TabView::on_search_changed(bool global) {
    if (global) return;  // the title bar's box, which MainView drives
    search_->sync_buttons();
    if (suppress_) return;
    bb_tab_set_search(core_.app(), tab_, core::s(search_->text()), false);
}

void TabView::search_escape(bool global) {
    if (core_.tab_state(tab_).is_search_running) {
        main_.execute_command(L"search.stop");
        return;
    }
    clear_search(global);
}

void TabView::clear_search(bool global) {
    try {
        type_search(L"", global);
    } catch (const core::CoreError&) {
    }
    main_.focus_file_list();
}

SearchField::SearchField(std::function<void()> clear, std::function<void()> save) : Edit(false) {
    auto make = [this](const IconData& icon, const wchar_t* tip, std::function<void()> click) {
        auto* b = emplace<Button>();
        b->icon = &icon;
        b->tooltip = tip;
        b->style.bg = tok::InputBackground;
        b->style.border = tok::InputBackground;
        b->style.fg = tok::TextSecondary;
        b->flags.tab_stop = false;
        b->flags.focusable = false;
        b->on_click = std::move(click);
        b->set_visible(false);
        return b;
    };
    save_ = make(icons::Save, L"Save this search\u2026", std::move(save));
    clear_ = make(icons::Dismiss, L"Clear search (Esc)", std::move(clear));
}

void SearchField::sync_buttons() {
    const bool shown = !text().empty();
    if (clear_->flags.visible == shown) return;
    clear_->set_visible(shown);
    save_->set_visible(shown);
    inset_right = shown ? 44.0f : 0.0f;
    arrange(bounds);
    invalidate();
}

void SearchField::arrange(Rect r) {
    Edit::arrange(r);
    const float size = std::min(22.0f, r.height() - 4);
    const float t = r.t + (r.height() - size) / 2;
    clear_->arrange(Rect::xywh(r.r - 2 - size, t, size, size));
    save_->arrange(Rect::xywh(r.r - 2 - 2 * size, t, size, size));
}

void TabView::type_search(const std::wstring& text, bool global) {
    suppress_ = true;
    if (global) {
        if (!text.empty()) search_->set_text(L"");
    } else {
        search_->set_text(text);
    }
    suppress_ = false;
    search_->sync_buttons();
    if (main_.active_tab() == tab_ && (global || !text.empty())) main_.set_title_search_text(global ? text : std::wstring());
    core::check(bb_tab_set_search(core_.app(), tab_, core::s(text), global), "search");
}

void TabView::begin_path_edit() {
    crumbs_->set_visible(false);
    path_->set_visible(true);
    path_->set_text(path_text_);
    root()->set_focus(path_);
    path_->select(0, path_text_.size());
    path_bar_->request_layout();
}

void TabView::end_path_edit() {
    if (!path_->flags.visible) return;
    path_->set_visible(false);
    crumbs_->set_visible(true);
    path_bar_->request_layout();
}

// ---- PaneView ----------------------------------------------------------------------------------------

bool PaneView::on_mouse_down(const MouseEvent&) {
    if (pane_ != main_.active_pane()) main_.activate_pane(pane_);
    return false;
}

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
    title_ = root->emplace<TitleBar>();
    title_->name = "TitleBarRoot";
    title_->dock = Dock::Top;
    title_->show_title = false;
    menu_bar_ = title_->emplace<MenuBar>();
    menu_bar_->name = "MainMenu";
    menu_bar_->nc_hit = HTCLIENT;
    title_->left = menu_bar_;
    title_toolbar_ = title_->emplace<CommandToolbar>("title");
    title_toolbar_->name = "TitleToolbar";
    title_toolbar_->style.bg = gfx::kNoToken;
    title_toolbar_->nc_hit = HTCLIENT;
    title_->middle = title_toolbar_;
    {
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
    {
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
    browser_ = root->emplace<Panel>();
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
    {
        // What a copy or move is doing: the headline, a bar, and the numbers. Above the status line.
        auto* strip = browser_dock->emplace<StackPanel>(Orientation::Horizontal, 14.0f);
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
        transfer_pause_ = strip->emplace<Button>(L"Pause");
        transfer_pause_->name = "TransferPause";
        transfer_pause_->width = 80;
        transfer_pause_->on_click = [this] {
            const bool paused = queue_["paused"].boolean();
            BbOwnedStr o{};
            bb_ops_do(core_.app(), core::s(paused ? L"queue-resume" : L"queue-pause"), core::s(L""), &o);
            bb_str_free(o);
        };
        transfer_cancel_ = strip->emplace<Button>(L"Cancel");
        transfer_cancel_->name = "TransferCancel";
        transfer_cancel_->width = 80;
        transfer_cancel_->on_click = [this] {
            BbOwnedStr o{};
            bb_ops_do(core_.app(), core::s(L"cancel-running"), core::s(L""), &o);
            bb_str_free(o);
        };
        auto* details = strip->emplace<Button>(L"Details");
        details->name = "TransferDetails";
        details->width = 90;
        details->on_click = [this] { open_dialog(L"transfer"); };
    }
    {
        // The offer to start the search index helper, above the status bar and the transfer strip:
        // shown until answered, Dismiss lasts the session. A neutral surface rather than a warning,
        // on purpose: nothing is wrong, search falls back to a crawl, and this is an offer. Spec:
        // MainWindow.xaml (IndexerBanner).
        auto* banner = browser_dock->emplace<StackPanel>(Orientation::Vertical, 0.0f);
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
    main_split_ = browser_dock->emplace<Splitter>(Orientation::Horizontal);
    main_split_->dock = Dock::Fill;
    main_split_->name = "MainSplitter";
    main_split_->set_position(240);
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
    // The panes, with the compare banner above them and not above the sidebar: a comparison is one
    // thing spanning two panes. Spec: MainWindow.xaml (PaneHostSite).
    auto* pane_column = main_split_->emplace<DockPanel>();
    pane_column->name = "PaneColumn";
    pane_column->add(make_compare_banner(*this));
    pane_host_ = pane_column->emplace<Panel>();
    pane_host_->name = "PaneHostSite";
    pane_host_->dock = Dock::Fill;

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
    tree_->on_context = [this](uint64_t id, Point at) {
        const std::wstring path = tree_provider_->path_of(id);
        if (!path.empty()) open_tree_menu(path, at);
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
        if (dialog_open_ && dialog_kind_ == L"search-syntax") defer([this] { close_dialog(); });
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
    Widget* content = window_.root().set_content(std::move(outer));
    popup_ = PopupLayer::install(*content);

    menu_bar_->hook(window_.root());
    menu_bar_->allows_mode = [this] { return (focus_flags() & (BB_FOCUS_PALETTE_OPEN | BB_FOCUS_CONTENT_BLOCKED | BB_FOCUS_RECORDING)) == 0; };
    title_toolbar_->on_activate = [this](const BarRow& r, Rect anchor) {
        if (r.kind == "list" || r.kind == "placeholder") open_toolbar_list(r, anchor);
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

void MainView::sink(void* ctx, BbEvent* evt) {
    // A Rust thread: enqueue and return. The window drains it on the UI thread.
    auto* self = static_cast<MainView*>(ctx);
    HWND hwnd = self->hwnd_.load();
    if (!hwnd || !PostMessageW(hwnd, WM_BB_EVENT, 0, reinterpret_cast<LPARAM>(evt))) {
        bb_event_free(evt);
    }
}

bool MainView::on_window_message(UINT msg, WPARAM wp, LPARAM lp) {
    if (msg == WM_BB_BARS) {
        bars_pending_ = false;
        try {
            refresh_bars();
        } catch (const std::exception&) {
        }
        return true;
    }
    if (msg == WM_BB_DEFER) {
        auto run = std::move(deferred_);
        deferred_.clear();
        for (auto& fn : run) fn();
        return true;
    }
    if (msg == WM_BB_ICONS) {
        icons_->drain();
        return true;
    }
    if (msg == WM_BB_TREE) {
        if (tree_provider_ && tree_provider_->drain() && tree_) tree_->refresh();
        return true;
    }
    if (msg == WM_BB_THUMBS) {
        for (auto& kv : by_tab_) kv.second->drain_thumbnails();
        return true;
    }
    if (msg == WM_BB_PREVIEW) {
        deliver_preview(static_cast<uint64_t>(wp), reinterpret_cast<PreviewResult*>(lp));
        return true;
    }
    if (msg != WM_BB_EVENT) return false;
    auto* evt = reinterpret_cast<BbEvent*>(lp);
    try {
        handle(*evt);
    } catch (const std::exception&) {
        // A view that cannot read the core right now repaints on the next event.
    }
    ++handled_;
    bb_event_free(evt);
    return true;
}

void MainView::deliver_preview(uint64_t tab, PreviewResult* raw) {
    std::unique_ptr<PreviewResult> result(raw);
    TabView* tv = tab_view(tab);
    if (!tv || !tv->preview()) {
        // The tab went away (or is parked) while its answer was on the way.
        if (result->gif) bb_gif_close(core_.app(), static_cast<uint64_t>(result->gif));
        return;
    }
    tv->preview()->apply(std::move(result));
}

void MainView::handle(const BbEvent& e) {
    switch (e.kind) {
        case BB_EVT_LISTING_CHANGED: {
            TabView* tv = tab_view(e.id);
            if (!tv) {
                sync_layout();
                tv = tab_view(e.id);
            }
            if (tv) tv->reload_listing();
            refresh_status();
            break;
        }
        case BB_EVT_TAB_CHANGED: {
            TabView* tv = tab_view(e.id);
            if (!tv) {
                sync_layout();
            } else {
                tv->reload_state();
                sync_layout();
            }
            refresh_status();
            break;
        }
        case BB_EVT_LAYOUT_CHANGED:
        case BB_EVT_PANE_CHANGED:
            sync_layout();
            break;
        case BB_EVT_SETTINGS_CHANGED: {
            const std::wstring key = core::w(e.text);
            if (core::icontains(key, L"theme")) apply_theme();
            if (key == L"showHiddenItems") sync_tree_hidden();
            // The page redraws only what moved (its signature), so this is cheap; it is how an import that
            // finished off the UI thread, or a problem with a list just typed, reaches the page.
            if (settings_ && settings_->is_open()) settings_->sync();
            break;
        }
        case BB_EVT_OP_FINISHED:
            // A cut is one-shot once something has moved, even if not everything did; a paste that moved
            // nothing leaves it to try elsewhere. Spec: ShellViewModel (Completed.Count > 0).
            if (cut_paste_op_ && e.id == cut_paste_op_) {
                const bool moved = e.a != 0 || std::any_of(cut_paste_paths_.begin(), cut_paste_paths_.end(), [](const std::wstring& p) {
                                       return GetFileAttributesW(p.c_str()) == INVALID_FILE_ATTRIBUTES;
                                   });
                if (moved && clipboard_seq() == cut_paste_seq_) {
                    if (headless_dialogs_) {
                        run_clipboard_.reset();
                        ++run_clipboard_seq_;
                    } else {
                        file_clipboard::clear();
                    }
                    push_clipboard_flag();
                }
                cut_paste_op_ = 0;
            }
            // An operation may have made, renamed, moved or deleted a folder: the open branches of the tree read
            // their sub-folders again. Spec: ShellViewModel.RefreshAfterCreateAsync (the tree's half).
            if (tree_ && tree_provider_ && tree_provider_->reload_where([this](uint64_t id) { return tree_->is_expanded(id); })) {
                tree_->refresh();
            }
            duplicates_settle_prune(*this);
            [[fallthrough]];
        case BB_EVT_UNDO_CHANGED:
        case BB_EVT_BUSY_CHANGED:
            if (e.kind != BB_EVT_BUSY_CHANGED && on_undo_changed) on_undo_changed();
            refresh_status();
            break;
        case BB_EVT_PROPERTIES_NEEDED:
            supply_properties(e.id);
            break;
        case BB_EVT_MODEL_CHANGED:
            if (e.a == 2) apply_theme();
            if (e.a == 10) {
                for (auto& kv : by_tab_) kv.second->reload_columns();
            }
            if (e.a == 4) refresh_tooltips();
            if (e.a == 11) compare_banner_sync(browser_->find("CompareBanner"));
            if (e.a == 14) sync_transfer();
            if (e.a == 15) sync_questions(L"conflicts");
            if (e.a == 16) sync_questions(L"elevation");
            if (e.a == 17) sync_questions(L"confirm");
            if (e.a == 18) sync_questions(L"notice");
            if (e.a == 19 && on_delete_survey) on_delete_survey();
            if (e.a == 20) sync_duplicates();
            if (e.a == 9) refresh_index_windows();
            if (e.a == 21) refresh_tool(L"checksum-show");
            if (e.a == 22) refresh_tool(L"disk-usage-show");
            if (e.a == 23) refresh_tool(L"compare-files-show");
            if (e.a == 24) sync_devices(false);
            if (e.a == 13) {
                for (auto& kv : by_tab_) {
                    if (kv.second->metadata()) kv.second->metadata()->refresh();
                }
            }
            if (e.a == 1 || e.a == 2 || e.a == 4 || e.a == 5 || e.a == 6 || e.a == 7 || e.a == 8 || e.a == 9 || e.a == 10) sync_overlays();
            break;
        default:
            break;
    }
    // Whatever moved may have changed what a command can do now: the bars catch up once, at idle.
    if (!bars_pending_ && hwnd_.load()) {
        bars_pending_ = true;
        PostMessageW(hwnd_.load(), WM_BB_BARS, 0, 0);
    }
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

void MainView::start(const std::wstring& path) {
    bb_ui_set_flag(core_.app(), core::s(L"propertyProvider"), true);
    bb_set_property_host(core_.app(), make_property_host());
    if (follow_windows_) {
        push_system_appearance();
        window_.on_system_settings_changed = [this] { push_system_appearance(); };
    }
    apply_theme();
    uint64_t tab = 0;
    core::check(bb_app_initialize(core_.app(), core::s(path), &tab), "initialize");
    sync_layout();
    refresh_bars();
    sync_overlays();
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

// The strip's buttons and its menu act on the pane they sit in, so that pane is made active first.
void MainView::wire_tab_strip(TabStrip& strip, uint64_t pane) {
    BbApp* app = core_.app();
    strip.on_move = [app](uint64_t tab, uint32_t index) { bb_tab_move(app, tab, index); };
    strip.on_new_tab = [this, pane] {
        activate_pane(pane);
        execute_command(L"tab.new");
    };
    strip.on_split = [this, pane](bool right) {
        activate_pane(pane);
        execute_command(right ? L"pane.split-right" : L"pane.split-below");
    };
    strip.on_context = [this](uint64_t tab, Point at) { show_context_menu(tab_menu(tab), at); };
    auto tip = [app](const wchar_t* text, const wchar_t* command) {
        BbOwnedStr o{};
        const std::wstring g = bb_keymap_gesture(app, core::s(command), &o) == BB_OK ? core::take(o) : std::wstring();
        return g.empty() ? std::wstring(text) : std::wstring(text) + L" (" + g + L")";
    };
    strip.new_tab_button()->tooltip = tip(L"New tab here", L"tab.new");
    strip.split_right_button()->tooltip = tip(L"Split pane to the right", L"pane.split-right");
    strip.split_below_button()->tooltip = tip(L"Split pane below", L"pane.split-below");
}

MenuItems MainView::tab_menu(uint64_t tab) {
    // The commands act on the active tab, so the one right-clicked becomes it.
    activate_tab(tab);
    std::vector<BarRow> rows;
    const std::pair<const char*, const wchar_t*> items[] = {{"tab.duplicate", L"&Duplicate tab"},
                                                            {"tab.close-others", L"Close &other tabs"},
                                                            {"tab.move-to-new-pane", L"&Move to new pane"},
                                                            {"-", nullptr},
                                                            {"tab.close", L"&Close tab"}};
    for (const auto& [id, caption] : items) {
        BarRow r;
        if (std::string(id) == "-") {
            r.kind = "separator";
            rows.push_back(std::move(r));
            continue;
        }
        const json::Value st = try_model([&](BbOwnedStr* o) { return bb_command_state(core_.app(), core::s(core::wide(id)), o); });
        if (st.is_null()) continue;
        r.kind = "command";
        r.id = id;
        r.icon = st["icon"].str();
        r.name = core::wide(st["name"].str());
        r.caption = caption;
        r.gesture = core::wide(st["gesture"].str());
        r.enabled = st["whyNot"].is_null();
        r.reason = core::wide(st["whyNot"].str());
        rows.push_back(std::move(r));
    }
    return build_menu_items(menu_env(), rows);
}

BbFocusKind MainView::focus_kind() const {
    Widget* f = window_.root().focused();
    if (const auto* edit = dynamic_cast<Edit*>(f)) return edit->read_only ? BB_FOCUS_TEXT_READ_ONLY : BB_FOCUS_TEXT_EDITABLE;
    if (!f || dynamic_cast<ListView*>(f) || dynamic_cast<TileView*>(f)) return BB_FOCUS_FILE_LIST;
    return BB_FOCUS_OTHER;
}

uint32_t MainView::focus_flags() {
    uint32_t flags = 0;
    if (try_model([&](BbOwnedStr* o) { return bb_settings_page_state(core_.app(), o); })["open"].boolean()) {
        flags |= BB_FOCUS_SETTINGS_OPEN;
        if (try_model([&](BbOwnedStr* o) { return bb_keys_page(core_.app(), o); })["recording"].boolean()) flags |= BB_FOCUS_RECORDING;
    }
    if (try_model([&](BbOwnedStr* o) { return bb_palette_state(core_.app(), o); })["open"].boolean()) flags |= BB_FOCUS_PALETTE_OPEN;
    if (dialog_open_) flags |= BB_FOCUS_CONTENT_BLOCKED;
    if (popup_ && popup_->menu_open()) flags |= BB_FOCUS_IN_POPUP;
    return flags;
}

namespace {

Button* find_button(Widget& w, bool Button::*flag) {
    if (auto* b = dynamic_cast<Button*>(&w); b && b->*flag && b->flags.enabled && b->shown()) return b;
    for (const auto& c : w.children()) {
        if (Button* found = find_button(*c, flag)) return found;
    }
    return nullptr;
}

// The card's default button: the one marked default, else the accent-filled one, when it can be pressed.
Button* primary_button(Widget& w) {
    if (Button* b = find_button(w, &Button::is_default)) return b;
    return find_button(w, &Button::primary);
}

// What has the keyboard when a card opens: its first field (with whatever selection the card gave it),
// else its default button. Spec: each dialog's Loaded handler.
Widget* first_focus(Widget& w) {
    if (!w.shown() || !w.flags.enabled) return nullptr;
    if (auto* e = dynamic_cast<Edit*>(&w); e && e->flags.focusable && !e->read_only) return e;
    for (const auto& c : w.children()) {
        if (Widget* found = first_focus(*c)) return found;
    }
    return nullptr;
}

}  // namespace

// A dialog's keys, as a WPF dialog's IsCancel and IsDefault buttons had them: Esc closes it (closing is
// "no" wherever a question is waiting), Enter presses the default button unless the keyboard is in
// something that takes Enter itself. The search syntax card closes on any key. Spec: ThemedWindow.
bool MainView::dialog_key(const KeyEvent& e) {
    if (!dialog_open_ || !dialogs_ || (popup_ && popup_->menu_open())) return false;
    const bool plain = !e.mods.ctrl && !e.mods.alt && !e.mods.shift;
    const bool modifier = e.vk == VK_SHIFT || e.vk == VK_CONTROL || e.vk == VK_MENU || e.vk == VK_LWIN || e.vk == VK_RWIN;
    if (dialog_kind_ == L"search-syntax" && !modifier) {
        close_dialog();
        return true;
    }
    if (e.vk == VK_ESCAPE && plain) {
        close_dialog();
        return true;
    }
    if (e.vk == VK_RETURN && plain) {
        Widget* focused = window_.root().focused();
        if (auto* edit = dynamic_cast<Edit*>(focused); edit && (edit->multiline || edit->on_commit)) return false;
        if (dynamic_cast<Button*>(focused)) return false;
        if (focused && dynamic_cast<forms::ItemList*>(focused) && static_cast<forms::ItemList*>(focused)->on_activate) return false;
        if (Button* b = primary_button(*dialogs_)) {
            if (b->on_click) b->on_click();
            return true;
        }
    }
    return false;
}

bool MainView::on_preview_key(const KeyEvent& e) {
    if (dialog_key(e)) return true;
    if (menu_bar_ && menu_bar_->preview_key(e)) return true;
    if (palette_ && palette_->preview_key(e)) return true;
    const std::wstring key = key_name(e.vk);
    if (key.empty()) return false;
    const uint32_t mods = (e.mods.ctrl ? 1u : 0u) | (e.mods.alt ? 2u : 0u) | (e.mods.shift ? 4u : 0u);
    const json::Value r = try_model([&](BbOwnedStr* o) {
        return bb_key_press(core_.app(), BbKeyChord{mods, core::s(key)}, BbFocusState{focus_kind(), focus_flags()}, false, o);
    });
    if (!r["claimed"].boolean()) return false;
    // A claimed chord is the command's even when it cannot run now.
    if (r["result"].str() == "ui") host_command(core::wide(r["command"].str()));
    if (r["result"].str() == "ran") after_command(core::wide(r["command"].str()));
    return true;
}

bool MainView::execute_command(const std::wstring& id, std::wstring* reason, bool* ui) {
    bool needs_ui = false;
    const BbStatus st = bb_command_execute(core_.app(), core::s(id), &needs_ui);
    if (st != BB_OK) {
        if (reason) *reason = core::last_error();
        return false;
    }
    if (needs_ui) {
        const bool done = host_command(id);
        if (ui) *ui = !done;  // true: it wanted a person (a dialog) and nothing was done
    } else {
        after_command(id);
    }
    return true;
}

// A command that changed which pane or tab is active, or made a new one, leaves the keyboard on that
// tab's list, as the original's PaneFocusRequested did - otherwise a rebuilt layout leaves it nowhere and
// the next arrow key goes unheard until a click. Deferred, so it lands after the layout the command
// changed has been rebuilt from the events already posted.
void MainView::after_command(const std::wstring& id) {
    const bool moves_focus = (id.rfind(L"tab.", 0) == 0 || id.rfind(L"pane.", 0) == 0 || id == L"file.open-pane-right" ||
                              id == L"file.open-pane-below") &&
                             id != L"pane.focus-tree" && id != L"pane.open-here-in-other" && id != L"pane.copy-to-other" &&
                             id != L"pane.move-to-other";
    if (!moves_focus) return;
    defer([this] {
        if (!dialog_open_) focus_file_list();
    });
}

bool MainView::host_command(const std::wstring& id) {
    if (id == L"nav.address-bar") {
        if (TabView* tv = active_view()) tv->begin_path_edit();
        return true;
    }
    // The side pane's own: the core only decides whether they may run (a pane is open, something plays).
    if (id == L"preview.fit-width" || id == L"preview.play" || id == L"preview.play-next" || id == L"preview.fullscreen") {
        TabView* tv = active_view();
        if (!tv || !tv->preview()) return false;
        if (id == L"preview.fit-width") tv->fit_side_width();
        else if (id == L"preview.play-next") tv->select_next_media();
        else if (id == L"preview.play") tv->preview()->media()->toggle_play();
        else tv->preview()->media()->set_full_screen(!tv->preview()->media()->full_screen());
        return true;
    }
    // The operations the core runs but only the host can start: they need the selection or a confirmation.
    if (id == L"edit.clear-undo-history") {
        if (!shows_dialogs()) return false;
        open_dialog(L"clear-undo");
        return true;
    }
    if (id == L"tools.transfer-cancel") {
        ops::run(core_, "cancel-running");
        return true;
    }
    if (id == L"file.create-shortcut") {
        std::vector<std::wstring> paths;
        for (const auto& r : selected_rows()) paths.push_back(r.path);
        if (paths.empty()) return false;
        ops::run(core_, "shortcut", "{\"sources\":" + ops::qa(paths) + ",\"directory\":" + ops::q(core_.tab_text(active_tab_, BB_TEXT_PATH)) + "}");
        return true;
    }
    // Commands that start another program for the user: the core decides what from the selection and
    // starts it through the one launcher (the harness's refuses and records).
    if (id == L"file.run-as-admin" || id == L"file.open-terminal" || id == L"file.open-vscode" || id.rfind(L"custom:", 0) == 0) {
        return bb_launch_command(core_.app(), core::s(id)) == BB_OK;
    }
    // Commands that put a dialog up. A run with no one to answer reports them instead.
    static const std::pair<const wchar_t*, const wchar_t*> dialogs[] = {
        {L"file.new-folder", L"new-folder"}, {L"file.new-file", L"new-file"},       {L"file.rename", L"rename"},
        {L"file.delete", L"delete"},         {L"file.delete-permanently", L"delete-permanently"},
        {L"file.properties", L"properties"}, {L"file.compress", L"compress"},
        {L"file.extract-to", L"extract"},    {L"file.unlock-archive", L"archive-password"},
        {L"app.about", L"about"},            {L"search.syntax", L"search-syntax"},  {L"search.save", L"saved-search"},
        {L"select.pattern", L"select-pattern"}, {L"select.unpattern", L"deselect-pattern"},
        {L"view.add-column", L"columns"},    {L"tools.transfer-details", L"transfer"}, {L"edit.undo-history", L"undo-history"},
        {L"app.customise-theme", L"theme-editor"},
    };
    for (const auto& [command, kind] : dialogs) {
        if (id != command) continue;
        if (!shows_dialogs()) return false;
        open_dialog(kind);
        return true;
    }
    // Extract here asks nothing: it goes where ExtractRules says, keeping both on a clash. Spec: ExtractAsync(askWhere: false).
    if (id == L"file.extract-here") {
        const ops::ExtractSubject subject = ops::extract_subject(*this);
        if (subject.archive.empty()) return false;
        ops::run_extract(*this, subject, ops::extract_destination(*this, subject.archive), false);
        return true;
    }
    if (id == L"tools.duplicates" && shows_dialogs()) {
        open_duplicates(core_.tab_text(active_tab_, BB_TEXT_PATH));
        return true;
    }
    if (id == L"edit.copy" || id == L"edit.cut") {
        std::vector<std::wstring> paths;
        for (const auto& r : selected_rows()) paths.push_back(r.path);
        if (paths.empty()) return false;
        const bool cut = id == L"edit.cut";
        const std::wstring said = clipboard_put(paths, cut)
                                      ? std::to_wstring(paths.size()) + L" item(s) " + (cut ? L"cut" : L"copied")
                                      : L"Could not copy - the clipboard is in use by another program.";
        bb_status_set(core_.app(), core::s(said));
        return true;
    }
    if (id == L"edit.paste" || id == L"edit.paste-shortcut") return paste(id == L"edit.paste-shortcut");
    if (id == L"file.copy-to" || id == L"file.move-to") return transfer_to_chosen(id == L"file.move-to");
    if (id == L"app.exit") {
        // A run has no window of the person's to close.
        if (headless_dialogs_) return false;
        PostMessageW(window_.hwnd(), WM_CLOSE, 0, 0);
        return true;
    }
    if (id == L"pane.focus-tree") {
        window_.root().set_focus(tree_);
        return true;
    }
    if (id == L"pane.focus-list") return focus_file_list();
    if (id == L"search.folder" || id == L"search.pc") {
        TabView* tv = active_view();
        if (!tv) return false;
        Widget* target = id == L"search.folder" ? static_cast<Widget*>(tv->search_box()) : static_cast<Widget*>(tv->global_search_box());
        if (!target) return false;
        window_.root().set_focus(target);
        // A search box takes focus with its text selected, so typing replaces it.
        if (auto* edit = dynamic_cast<Edit*>(target)) edit->select(0, edit->text().size());
        return true;
    }
    if (id == L"app.save-workspace" || id == L"app.save-command-bars") {
        if (!shows_dialogs()) return false;
        open_dialog(id == L"app.save-workspace" ? L"save-workspace" : L"save-command-bars");
        return true;
    }
    if (id == L"edit.copy-path" || id == L"edit.copy-name" || id == L"edit.copy-folder-path") {
        std::vector<std::wstring> paths;
        if (id == L"edit.copy-folder-path") paths.push_back(core_.tab_text(active_tab_, BB_TEXT_PATH));
        else {
            for (const auto& r : selected_rows()) paths.push_back(id == L"edit.copy-name" ? r.name : r.path);
        }
        if (paths.empty() || paths.front().empty()) return false;
        // "Copy as path" quotes each one, as Explorer's does; this folder's own path is copied bare.
        copy_text(paths, id == L"edit.copy-path", id == L"edit.copy-folder-path");
        return true;
    }
    return tool_host_command(*this, id);
}

void MainView::set_headless_dialogs(bool on) {
    headless_dialogs_ = on;
    push_clipboard_flag();
}

uint32_t MainView::clipboard_seq() const { return headless_dialogs_ ? run_clipboard_seq_ : GetClipboardSequenceNumber(); }

bool MainView::clipboard_put(const std::vector<std::wstring>& paths, bool cut) {
    if (headless_dialogs_) {
        run_clipboard_ = ClipboardFiles{paths, cut};
        ++run_clipboard_seq_;
        push_clipboard_flag();
        return true;
    }
    return file_clipboard::put(paths, cut);  // WM_CLIPBOARDUPDATE pushes the flag
}

std::optional<ClipboardFiles> MainView::clipboard_get() { return headless_dialogs_ ? run_clipboard_ : file_clipboard::get(); }

void MainView::push_clipboard_flag() {
    const bool has = headless_dialogs_ ? run_clipboard_.has_value() : file_clipboard::has_files();
    bb_ui_set_flag(core_.app(), core::s(L"clipboardHasFiles"), has);
}

bool MainView::paste(bool shortcut) {
    const std::wstring here = core_.tab_text(active_tab_, BB_TEXT_PATH);
    if (here.empty()) return false;
    const std::optional<ClipboardFiles> clip = clipboard_get();
    if (!clip || clip->paths.empty()) {
        bb_status_set(core_.app(), core::s(L"There are no files on the clipboard."));
        return true;
    }
    // A paste is a drop by another name: the one planner and executor, with progress, conflicts and undo.
    const uint64_t op = transfer_into(core_, clip->paths, here, shortcut ? "shortcut" : clip->cut ? "move" : "copy");
    if (!shortcut && clip->cut && op) {
        cut_paste_op_ = op;
        cut_paste_seq_ = clipboard_seq();
        cut_paste_paths_ = clip->paths;
    }
    return true;
}

bool MainView::transfer_to_chosen(bool move) {
    std::vector<std::wstring> sources;
    for (const auto& r : selected_rows()) sources.push_back(r.path);
    // A run cannot answer Windows' folder picker, so it reports the command instead.
    if (sources.empty() || headless_dialogs_) return false;
    IFileOpenDialog* dialog = nullptr;
    if (FAILED(CoCreateInstance(CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&dialog)))) return false;
    DWORD options = 0;
    dialog->GetOptions(&options);
    dialog->SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST);
    dialog->SetTitle(move ? L"Move to folder" : L"Copy to folder");
    const std::wstring here = core_.tab_text(active_tab_, BB_TEXT_PATH);
    IShellItem* start = nullptr;
    if (!here.empty() && SUCCEEDED(SHCreateItemFromParsingName(here.c_str(), nullptr, IID_PPV_ARGS(&start)))) {
        dialog->SetFolder(start);
        start->Release();
    }
    std::wstring chosen;
    if (SUCCEEDED(dialog->Show(window_.hwnd()))) {
        IShellItem* item = nullptr;
        if (SUCCEEDED(dialog->GetResult(&item))) {
            PWSTR path = nullptr;
            if (SUCCEEDED(item->GetDisplayName(SIGDN_FILESYSPATH, &path))) {
                chosen = path;
                CoTaskMemFree(path);
            }
            item->Release();
        }
    }
    dialog->Release();
    if (!chosen.empty()) transfer_into(core_, sources, chosen, move ? "move" : "copy");
    return true;
}

void MainView::open_duplicates(const std::wstring& folder) {
    // Pointed at the folder (none: this PC) and waiting for Scan: a scan reads real files, so it is asked for.
    const std::string root = folder.empty() ? "null" : json_quote(folder);
    try_tool_call(*this, "duplicates.load", "{\"root\":" + root + "}");
    duplicates_.ticked.clear();
    sync_duplicates();
    open_dialog(L"duplicates");
}

// The index moved (a volume finished, the helper started or stopped): the windows whose answer comes from it
// ask again. Duplicates scans again only when its answer was not believable yet; a finished scan is not
// thrown away for an index that was already enough. Spec: DiskUsageViewModel / ChangeTimelineViewModel /
// DuplicatesViewModel .OnIndexRefreshed and .OnStatusChanged.
void MainView::refresh_index_windows() {
    refresh_tool(L"disk-usage-at");
    if (tool_window(L"changes")) refresh_changes(true);
    if (tool_window(L"duplicates")) {
        const json::Value& m = duplicates_.result;
        if (m["hasScanned"].boolean() && !m["scanning"].boolean() && m["availability"].str() != "Ready") {
            const std::string root = m["root"].is_null() ? "null" : json_quote(core::wide(m["root"].str()));
            try_tool_call(*this, "duplicates.scan", "{\"root\":" + root + "}");
        } else {
            sync_duplicates();
        }
    }
}

// What changed slides with the clock ("in the last hour"), so its window reads the log again every two
// seconds while it is open - and is rebuilt only when what it would show is different, so a list somebody
// is reading does not jump under them. Spec: ChangeTimelineWindow (its DispatcherTimer).
void MainView::refresh_changes(bool force) {
    if (!tool_window(L"changes")) return;
    // The clock tick asks the log's stamp, which is cheap; the query runs when that moved, when recording was
    // switched, or every thirty seconds so "2 min ago" keeps up. Spec: ChangeTimelineViewModel.OnTick.
    {
        const json::Value st = try_tool_call(*this, "changes.stamp");
        const std::string stamp = st["stamp"].str() + (st["recording"].boolean() ? "|on" : "|off");
        if (!force && stamp == changes_stamp_ && ++changes_ticks_ < 15) return;
        changes_stamp_ = stamp;
    }
    changes_ticks_ = 0;
    std::wstring error;
    const json::Value m = try_tool_call(*this, "changes.model", "", &error);
    if (m.is_null()) return;
    std::string sig = m["summary"].str() + "|" + m["markText"].str() + "|" + m["message"].str() + "|" + std::to_string(m["rows"].size());
    for (size_t i = 0; i < m["rows"].size() && i < 64; ++i) sig += "|" + m["rows"][i]["path"].str() + m["rows"][i]["when"].str();
    if (!force && sig == changes_signature_) return;
    changes_signature_ = sig;
    refresh_tool(L"changes-show");
}

void MainView::sync_duplicates() {
    std::wstring error;
    json::Value m = try_tool_call(*this, "duplicates.model", "", &error);
    if (m.is_null()) return;
    const int64_t version = m["version"].i64();
    if (version != duplicates_.version) duplicates_.ticked.clear();
    duplicates_.version = version;
    duplicates_.result = std::move(m);
    duplicates_.has = true;
    refresh_tool(L"duplicates");
}

// "Copy as path" quotes every path, as Explorer's does, one per line; a name is bare text. A scripted run
// leaves the clipboard alone (it is the person's, not the run's) and only says what it would have copied.
void MainView::copy_text(const std::vector<std::wstring>& items, bool paths, bool this_folder) {
    std::wstring text;
    for (const auto& item : items) {
        if (!text.empty()) text += L"\r\n";
        text += paths ? L"\"" + item + L"\"" : item;
    }
    bool copied = headless_dialogs_;
    if (!headless_dialogs_ && OpenClipboard(window_.hwnd())) {
        EmptyClipboard();
        const size_t bytes = (text.size() + 1) * sizeof(wchar_t);
        if (HGLOBAL mem = GlobalAlloc(GMEM_MOVEABLE, bytes)) {
            if (void* p = GlobalLock(mem)) {
                memcpy(p, text.c_str(), bytes);
                GlobalUnlock(mem);
                copied = SetClipboardData(CF_UNICODETEXT, mem) != nullptr;
            }
            if (!copied) GlobalFree(mem);
        }
        CloseClipboard();
    }
    const std::wstring noun = paths ? L"path" : L"name";
    const std::wstring said = !copied ? L"Could not copy - the clipboard is in use by another program."
                              : this_folder ? L"Copied this folder's path"
                                            : L"Copied " + std::to_wstring(items.size()) + L" " + noun + (items.size() == 1 ? L"" : L"s");
    bb_status_set(core_.app(), core::s(said));
}

void MainView::open_tree_menu(const std::wstring& folder, Point at) {
    ops::ComposedMenu composed = ops::compose_context_menu(*this, ops::MenuKind::Tree, folder);
    keep_menu_session(std::move(composed.keep));
    show_context_menu(std::move(composed.items), at);
}

void MainView::tree_verb(const std::wstring& id, const std::wstring& folder) {
    BbApp* app = core_.app();
    // The dialogs that read a folder take this one while they are built.
    auto dialog_on = [&](const wchar_t* kind) {
        dialog_folder_ = folder;
        open_dialog(kind);
        dialog_folder_.clear();
    };
    if (id == L"file.new-folder") dialog_on(L"new-folder");
    else if (id == L"file.new-file") dialog_on(L"new-file");
    else if (id == L"file.properties") dialog_on(L"properties");
    else if (id == L"tools.changes") dialog_on(L"changes");
    else if (id == L"tools.disk-usage") {
        tools_ui(*this).disk_root = folder;
        open_dialog(L"disk-usage-at");
    } else if (id == L"tools.duplicates") {
        if (shows_dialogs()) open_duplicates(folder);
    } else if (id == L"file.open-new-tab") {
        uint64_t tab = 0;
        bb_tab_open(app, 0, core::s(folder), true, &tab);
    } else if (id == L"file.open-pane-right" || id == L"file.open-pane-below") {
        uint64_t pane = 0;
        bb_pane_split(app, 0, id == L"file.open-pane-right" ? BB_VERTICAL : BB_HORIZONTAL, core::s(folder), &pane);
    } else if (id == L"file.open-terminal" || id == L"file.open-vscode") {
        bb_launch_command_at(app, core::s(id), core::s(folder));
    } else if (id == L"file.bookmark") {
        bb_saved_do(app, 0, core::s(L"toggle-bookmark"), core::s(folder));
    } else if (id == L"file.delete") {
        dialog_on(L"delete");
    } else if (id == L"edit.copy-path") {
        copy_text({folder}, true, false);
    }
}

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
                else execute_command(core::wide(r.id));
            };
            tb->on_overflow = [this](const std::vector<BarRow>& rows, Rect anchor) { open_overflow(rows, anchor); };
            tb->on_context = [this](const BarRow& r, Rect anchor) { open_step_menu(r, anchor); };
            row_toolbars_.push_back(tb);
        }
        rows_panel_->height = static_cast<float>(want) * CommandToolbar::kHeight;
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
}

namespace {

// One window per tool: disk usage pointed somewhere else, or checksums verifying instead of hashing, is the
// same window showing something else.
std::wstring tool_key(const std::wstring& kind) {
    if (kind == L"disk-usage-at") return L"disk-usage";
    if (kind == L"checksum-verify" || kind == L"checksum-show") return L"checksum";
    // A window's "-show" kind rebuilds it from the model it already has.
    if (kind.size() > 5 && kind.compare(kind.size() - 5, 5, L"-show") == 0) return kind.substr(0, kind.size() - 5);
    return kind;
}

}  // namespace

bool MainView::is_tool_window(const std::wstring& kind) {
    static const wchar_t* const kinds[] = {L"disk-usage",   L"disk-usage-at", L"disk-usage-show", L"duplicates", L"changes",      L"checksum",
                                           L"checksum-verify", L"checksum-show", L"compare-files", L"compare-files-show",
                                           L"changes-show", L"undo-history", L"transfer",
                                           L"theme-editor"};
    for (const wchar_t* k : kinds) {
        if (kind == k) return true;
    }
    return false;
}

ToolWindow* MainView::tool_window(const std::wstring& kind) {
    const auto it = tools_.find(tool_key(kind));
    return it == tools_.end() ? nullptr : it->second.get();
}

void MainView::close_tool(const std::wstring& kind) {
    const std::wstring key = tool_key(kind);
    if (key == L"changes") {
        KillTimer(window_.hwnd(), kChangesTimer);
        changes_signature_.clear();
    }
    tools_.erase(key);
}

std::vector<std::wstring> MainView::open_tool_windows() const {
    std::vector<std::wstring> out;
    for (const auto& kv : tools_) out.push_back(kv.first);
    return out;
}

void MainView::style_tool_window(ToolWindow& w) {
    const std::vector<uint32_t> argb = core_.palette();
    if (argb.size() != gfx::tok::Count) return;
    auto rgb = [&](gfx::Token t) {
        const uint32_t v = argb[t];
        return RGB((v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF);
    };
    const uint32_t bg = argb[gfx::tok::WindowBackground];
    const int luma = (((bg >> 16) & 0xFF) * 299 + ((bg >> 8) & 0xFF) * 587 + (bg & 0xFF) * 114) / 1000;
    w.apply_theme(luma < 128, rgb(gfx::tok::TitleBarBorder), rgb(gfx::tok::TitleBarBackground), rgb(gfx::tok::TitleBarForeground));
}

// Compare files steps through the differences with F3 / Shift+F3 and Alt+Down / Alt+Up. Spec:
// FileCompareWindow.xaml (InputBindings).
bool MainView::tool_window_key(const std::wstring& key, const KeyEvent& e) {
    if (key != L"compare-files" || e.mods.ctrl) return false;
    int dir = 0;
    if (e.vk == VK_F3 && !e.mods.alt) dir = e.mods.shift ? -1 : 1;
    else if (e.mods.alt && !e.mods.shift && (e.vk == VK_DOWN || e.vk == VK_UP)) dir = e.vk == VK_DOWN ? 1 : -1;
    if (!dir) return false;
    try_tool_call(*this, "filecompare.step", "{\"direction\":" + std::to_string(dir) + "}");
    defer([this] { open_dialog(L"compare-files-show"); });
    return true;
}

void MainView::show_message(std::wstring title, std::wstring text) {
    message_ = {std::move(title), std::move(text)};
    defer([this] { open_dialog(L"message"); });
}

// Rebuilds a tool window that is already open, from something other than the person (progress, the index,
// a timer): the same as opening it again, minus taking the foreground.
void MainView::refresh_tool(const std::wstring& kind) {
    if (!tool_window(tool_key(kind))) return;
    refreshing_tool_ = true;
    open_dialog(kind);
    refreshing_tool_ = false;
}

void MainView::open_dialog(const std::wstring& kind) {
    if (!shows_dialogs() || !dialog_exists(kind)) return;
    if (is_tool_window(kind)) {
        const std::wstring key = tool_key(kind);
        auto card = build_dialog(kind, DialogContext{*this, [this, key] { defer([this, key] { close_tool(key); }); }});
        if (!card) return;
        if (key == L"theme-editor") theme_editor_id_ = try_model([&](BbOwnedStr* o) { return bb_theme_state(core_.app(), o); })["id"].str();
        if (ToolWindow* open = tool_window(key)) {
            open->replace(std::move(card));
            // Brought forward when somebody asked for it; a refresh behind their back never activates it.
            if (!refreshing_tool_) open->focus();
            return;
        }
        auto w = std::make_unique<ToolWindow>(*this, key, std::move(card));
        ToolWindow* raw = w.get();
        tools_[key] = std::move(w);
        raw->show();
        if (key == L"changes") SetTimer(window_.hwnd(), kChangesTimer, 2000, nullptr);
        return;
    }
    // A card rebuilt as itself (a tick in the sync preview, the live run) keeps its lists' places and what
    // had the keyboard; anything else starts fresh.
    auto base = [](std::wstring k) {
        for (const wchar_t* suffix : {L"-edit", L"-live"}) {
            const size_t n = wcslen(suffix);
            if (k.size() > n && k.compare(k.size() - n, n, suffix) == 0) k.resize(k.size() - n);
        }
        return k;
    };
    const bool again = dialog_open_ && base(dialog_kind_) == base(kind);
    tools::ListPlaces places;
    std::string focused_name;
    Widget* kept_focus = focus_before_dialog_;
    if (again) {
        places = tools::save_places(*dialogs_);
        if (Widget* f = window_.root().focused(); f && dialogs_->holds(f)) focused_name = f->name;
    }
    close_dialog();
    auto card = build_dialog(kind, DialogContext{*this, [this] { close_dialog(); }});
    if (!card) return;
    dialog_open_ = true;
    dialog_kind_ = kind;
    bb_ui_set_dialog_open(core_.app(), true);
    Widget* before = window_.root().focused();
    if (again) focus_before_dialog_ = kept_focus;
    else if (!dialogs_->holds(before)) focus_before_dialog_ = before;
    dialogs_->show(std::move(card));
    if (again) tools::restore_places(*dialogs_, places);
    Widget* named = focused_name.empty() ? nullptr : dialogs_->find(focused_name);
    if (named) window_.root().set_focus(named);
    else if (Widget* f = first_focus(*dialogs_)) window_.root().set_focus(f);
    else if (Button* b = primary_button(*dialogs_)) window_.root().set_focus(b);
}

void MainView::open_new_file(const std::wstring& extension, const std::wstring& label, const std::wstring& folder) {
    new_file_template_ = {extension, label};
    dialog_folder_ = folder;
    open_dialog(L"new-file");
    dialog_folder_.clear();
    new_file_template_ = {};
}

void MainView::edit_saved_search(const std::wstring& name) {
    editing_saved_ = name;
    open_dialog(L"saved-search");
    editing_saved_.clear();
}

void MainView::rename_workspace(const std::wstring& name) {
    editing_saved_ = name;
    open_dialog(L"rename-workspace");
    editing_saved_.clear();
}

void MainView::reopen_on(const std::wstring& kind, std::vector<std::wstring> paths) {
    dialog_paths_ = std::move(paths);
    open_dialog(kind);
    dialog_paths_.clear();
}

void MainView::ask_delete(std::vector<std::wstring> paths, std::function<void(bool)> after) {
    dialog_paths_ = std::move(paths);
    after_delete_ = std::move(after);
    open_dialog(L"delete");
    dialog_paths_.clear();
    after_delete_ = nullptr;  // taken by the dialog when it was built
}

Widget* MainView::dialog_widget(std::string_view name) {
    if (dialog_open_ && dialogs_) {
        if (Widget* w = dialogs_->find(name)) return w;
    }
    for (auto& kv : tools_) {
        if (Widget* w = kv.second->find(name)) return w;
    }
    return nullptr;
}

bool MainView::focus_file_list() {
    TabView* tv = active_view();
    if (!tv) return false;
    Widget* target = tv->tiles_mode() ? static_cast<Widget*>(tv->tiles()) : static_cast<Widget*>(tv->list());
    if (!target) return false;
    window_.root().set_focus(target);
    return true;
}

void MainView::close_dialog() {
    if (dialogs_ && dialogs_->is_open()) dialogs_->close();
}

std::unique_ptr<gfx::BitmapSurface> MainView::render_dialog(const std::wstring& kind) {
    auto card = build_dialog(kind, DialogContext{*this, [] {}});
    if (!card) return nullptr;
    return render_dialog_detached(window_.root().resources(), std::move(card));
}

std::vector<SelectedRow> MainView::selected_rows() {
    std::vector<SelectedRow> out;
    if (!active_tab_) return out;
    const auto listing = core_.listing(active_tab_);
    if (!listing) return out;
    for (const uint32_t i : core_.selection(active_tab_)) {
        BbRow r{};
        if (!listing->row(i, r)) continue;
        SelectedRow s;
        s.name = core::w(r.name);
        s.path = core::w(r.full_path);
        s.dir = (r.flags & BB_ROW_DIRECTORY) != 0;
        s.size = r.size;
        s.modified = r.modified;
        s.created = r.created;
        out.push_back(std::move(s));
    }
    return out;
}

void MainView::sync_transfer() {
    queue_ = try_model([&](BbOwnedStr* o) { return bb_ops_query(core_.app(), core::s(L"queue"), core::s(L""), o); });
    const bool active = queue_["active"].boolean();
    const json::Value& v = queue_["view"];
    transfer_strip_->set_visible(active);
    transfer_head_->set_text(core::wide(v["headline"].str()));
    transfer_detail_->set_text(core::wide(v["detail"].str()));
    const bool paused = queue_["paused"].boolean();
    transfer_bar_->set_indeterminate(active && v["indeterminate"].boolean());
    if (!v["indeterminate"].boolean()) transfer_bar_->set_value(v["fraction"].num());
    transfer_pause_->text = paused ? L"Resume" : L"Pause";
    transfer_pause_->set_enabled(queue_["canPause"].boolean() || queue_["canResume"].boolean());
    transfer_cancel_->set_enabled(queue_["canCancel"].boolean());
    browser_->request_layout();
    if (on_transfer_changed) on_transfer_changed();
    if (on_sync_progress) on_sync_progress();
    // The queue window goes when there is nothing left to show in it. Spec: MainWindow (TransferProgress null).
    if (!active && tool_window(L"transfer")) defer([this] { close_tool(L"transfer"); });
}

std::wstring MainView::transfer_text() const {
    if (!queue_["active"].boolean()) return {};
    const json::Value& v = queue_["view"];
    return core::wide(v["headline"].str()) + L"  " + core::wide(v["detail"].str());
}

void MainView::sync_questions(const wchar_t* kind) {
    if (headless_dialogs_) return;
    const json::Value q = try_model([&](BbOwnedStr* o) { return bb_ops_query(core_.app(), core::s(kind), core::s(L""), o); });
    if (q["pending"].is_null() || (dialog_open_ && dialog_kind_ == kind)) return;
    // A question parks a worker until it is answered, so it replaces whatever dialog is up - but never
    // another question, which would be answered "no" by being closed. A notice waits for any dialog, and
    // whatever is left waiting is asked again once that dialog has gone.
    const bool question_up = dialog_open_ && (dialog_kind_ == L"conflicts" || dialog_kind_ == L"elevation" || dialog_kind_ == L"confirm");
    if (question_up || (dialog_open_ && std::wstring(kind) == L"notice")) {
        questions_waiting_ = true;
        return;
    }
    open_dialog(kind);  // closes the one before it
}

void MainView::ensure_machine_properties() {
    if (machine_props_set_) return;
    machine_props_set_ = true;
    const std::string list = machine_property_list();
    bb_columns_set_machine_properties(core_.app(), core::s(core::wide(list)));
}

bool MainView::build_named_menu(const std::wstring& kind, MenuItems* out) {
    TabView* tv = active_view();
    if (!tv) return false;
    if (kind == L"columns") *out = tv->build_column_menu();
    else if (kind == L"files") *out = tv->build_files_menu(false);
    else if (kind == L"background") *out = tv->build_files_menu(true);
    else if (kind == L"tree") {
        std::wstring folder = tree_provider_->path_of(tree_->selected_id());
        if (folder.empty()) folder = core_.tab_text(active_tab_, BB_TEXT_PATH);
        ops::ComposedMenu composed = ops::compose_context_menu(*this, ops::MenuKind::Tree, folder);
        keep_menu_session(std::move(composed.keep));
        *out = std::move(composed.items);
    } else if (!ops::build_step_menu(*this, kind, out)) return false;
    return true;
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
    if (!m["loaded"].boolean()) return;
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

size_t MainView::tree_pending() const { return tree_provider_ ? tree_provider_->pending() : 0; }

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
    std::wstring right = core::wide(index_state_["status"].str());
    if (!right.empty() && index_state_["canRetry"].boolean()) right += L"  —  retry";
    status_->set_text(core_.tab_text(active_tab_, BB_TEXT_STATUS), right);
    // Undo's caption is what it would undo, and there is one only while there is something to undo.
    const json::Value undo = try_model([&](BbOwnedStr* o) { return bb_command_state(core_.app(), core::s(L"edit.undo"), o); });
    status_->set_extras(undo.is_null() ? std::wstring() : core::wide(undo["caption"].str()),
                        core_.tab_text(active_tab_, BB_TEXT_SELECTION));
    status_->set_scale(core_.tab_state(active_tab_).thumbnail_scale);
}

void MainView::sync_layout() {
    const json::Value st = json::parse(core_.state_json());
    active_tab_ = static_cast<uint64_t>(st["activeTab"].i64());
    active_pane_ = static_cast<uint64_t>(st["activePane"].i64());
    const std::string sig = signature(st["layout"], st["panes"]) + "@" + std::to_string(active_pane_) + "/" +
                            std::to_string(active_tab_);
    if (sig != layout_sig_) {
        layout_sig_ = sig;
        rebuild_layout(st);
    }
    // Titles move with navigation, so the strips are refreshed whether or not the shape changed.
    for (PaneView* pv : pane_views_) {
        const json::Value& p = pane_state(st["panes"], pv->pane());
        std::vector<TabEntry> entries;
        for (size_t i = 0; i < p["tabs"].size(); ++i) {
            const uint64_t id = static_cast<uint64_t>(p["tabs"][i].i64());
            entries.push_back({id, core_.tab_text(id, BB_TEXT_TITLE), id == static_cast<uint64_t>(p["activeTab"].i64(-1)),
                               core_.tab_text(id, BB_TEXT_PATH)});
        }
        pv->strip->set_tabs(std::move(entries), pv->pane() == active_pane_);
    }
    if (active_tab_) {
        reveal_in_tree(core_.tab_text(active_tab_, BB_TEXT_PATH));
    }
    sync_title_search();
    refresh_status();
}

void MainView::rebuild_layout(const json::Value& st) {
    // Tab views outlive the layout: park them, drop the old shape, build the new one around them.
    for (PaneView* pv : pane_views_) {
        if (pv->content) {
            const uint64_t tab = pv->content->tab();
            parked_[tab] = std::unique_ptr<TabView>(static_cast<TabView*>(pv->remove_child(pv->content).release()));
            pv->content = nullptr;
        }
    }
    pane_views_.clear();
    if (layout_root_) {
        pane_host_->remove_child(layout_root_);
        layout_root_ = nullptr;
    }
    // A tab the core no longer has goes away with its view.
    std::vector<uint64_t> alive;
    for (size_t i = 0; i < st["tabs"].size(); ++i) alive.push_back(static_cast<uint64_t>(st["tabs"][i]["id"].i64()));
    for (auto it = parked_.begin(); it != parked_.end();) {
        if (std::find(alive.begin(), alive.end(), it->first) == alive.end()) {
            by_tab_.erase(it->first);
            it = parked_.erase(it);
        } else {
            ++it;
        }
    }
    std::unique_ptr<Widget> built = build_node(st["layout"], st["panes"]);
    if (built) layout_root_ = pane_host_->add(std::move(built));
    pane_host_->request_layout();
}

std::unique_ptr<Widget> MainView::build_node(const json::Value& node, const json::Value& panes) {
    if (node["kind"].str() == "pane") {
        const uint64_t id = static_cast<uint64_t>(node["pane"].i64());
        auto pv = std::make_unique<PaneView>(*this, id);
        pv->name = "Pane" + std::to_string(id);
        pv->strip = pv->emplace<TabStrip>();
        pv->strip->name = "TabHost";
        pv->strip->on_activate = [this](uint64_t t) { activate_tab(t); };
        pv->strip->on_close = [this](uint64_t t) { close_tab(t); };
        wire_tab_strip(*pv->strip, id);
        const json::Value& p = pane_state(panes, id);
        if (!p["activeTab"].is_null()) {
            const uint64_t tab = static_cast<uint64_t>(p["activeTab"].i64());
            std::unique_ptr<TabView> view;
            if (auto it = parked_.find(tab); it != parked_.end()) {
                view = std::move(it->second);
                parked_.erase(it);
            } else {
                view = std::make_unique<TabView>(*this, core_, tab);
                view->set_thumbnail_source(thumb_source_);
                by_tab_[tab] = view.get();
                view->reload_listing();
                view->reload_state();
            }
            view->flex = 1;
            pv->content = pv->add(std::move(view));
        }
        pane_views_.push_back(pv.get());
        return pv;
    }
    const json::Value& kids = node["children"];
    if (kids.size() == 0) return nullptr;
    const Orientation o = node["orientation"].str() == "vertical" ? Orientation::Horizontal : Orientation::Vertical;
    // Right-nested: [a, b, c] is a | (b | c), each first child weighted against the rest.
    std::function<std::unique_ptr<Widget>(size_t)> chain = [&](size_t i) -> std::unique_ptr<Widget> {
        std::unique_ptr<Widget> first = build_node(kids[i], panes);
        if (i + 1 >= kids.size()) return first;
        float rest = 0;
        for (size_t k = i + 1; k < kids.size(); ++k) rest += static_cast<float>(kids[k]["weight"].num(1));
        const float mine = static_cast<float>(kids[i]["weight"].num(1));
        auto split = std::make_unique<WeightedSplitter>(o, mine / std::max(0.0001f, mine + rest));
        split->add_child(std::move(first));
        split->add_child(chain(i + 1));
        return split;
    };
    return chain(0);
}

}  // namespace bb::views
