#include "views/tool_dialogs.h"
#include "views/tool_state.h"
#include "views/main_view.h"

#include "gfx/icons.gen.h"
#include "views/folder_tree.h"
#include "views/media_pane.h"
#include "views/property_host.h"
#include "views/shell_props.h"

#include <dwmapi.h>

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

// ---- ListAdapter -------------------------------------------------------------------------------------

void ListAdapter::reset(std::shared_ptr<core::Listing> l) {
    listing_ = std::move(l);
    reload_selection();
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
    style.bg = tok::ListBackground;
    BbApp* app = core.app();

    auto nav = std::make_unique<StackPanel>(Orientation::Horizontal, 6.0f);
    nav->pad = 6;
    nav->height = 42;
    nav->dock = Dock::Top;
    nav->name = "NavBar";
    nav->style.bg = tok::ToolbarBackground;
    nav_ = nav.get();
    auto button = [&](const wchar_t* glyph, std::function<void()> fn) {
        auto b = std::make_unique<Button>(glyph);
        b->width = 34;
        b->on_click = std::move(fn);
        return nav_->add(std::move(b));
    };
    back_ = button(L"←", [this, app] { bb_tab_back(app, tab_); });
    forward_ = button(L"→", [this, app] { bb_tab_forward(app, tab_); });
    up_ = button(L"↑", [this, app] { bb_tab_up(app, tab_); });
    refresh_ = button(L"↻", [this, app] { bb_tab_refresh(app, tab_); });
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

    search_ = nav_->emplace<Edit>();
    search_->name = "SearchBox";
    search_->width = 170;
    search_->placeholder = L"Search";
    search_->on_changed = [this](const std::wstring&) { on_search_changed(false); };
    gsearch_ = nav_->emplace<Edit>();
    gsearch_->name = "GlobalSearchBox";
    gsearch_->width = 170;
    gsearch_->placeholder = L"Search this PC";
    gsearch_->on_changed = [this](const std::wstring&) { on_search_changed(true); };
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

    auto banner = std::make_unique<Panel>();
    banner->pad = 4;
    banner->height = 28;
    banner->dock = Dock::Top;
    banner->name = "ErrorBanner";
    banner->style.bg = tok::ErrorBackground;
    banner_ = banner->emplace<TextBlock>();
    banner_->style.fg = tok::ErrorForeground;
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
    list_->on_context = [this](Point p, size_t) { open_context(p, false); };
    tiles_->on_activate = [this, app](size_t row) { bb_tab_open_item(app, tab_, static_cast<uint32_t>(row)); };

    list_->on_sort = [this, app](int column, bool) {
        if (column < 0 || static_cast<size_t>(column) >= columns_.size()) return;
        bb_tab_set_sort(app, tab_, core::s(columns_[static_cast<size_t>(column)].id));
    };
    list_->on_activate = [this, app](size_t row) { bb_tab_open_item(app, tab_, static_cast<uint32_t>(row)); };
    list_->on_typeahead = [this](const std::wstring& prefix) {
        const auto& l = adapter_.listing();
        if (!l) return;
        for (size_t i = 0; i < l->size(); ++i) {
            if (core::iequals(l->name(i).substr(0, prefix.size()), prefix)) {
                list_->select_row(i);
                return;
            }
        }
    };
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

void TabView::arrange(Rect r) {
    // A narrow pane gives up the whole-PC search box first, so the path never shrinks to nothing.
    const bool narrow = r.width() < 700.0f;
    if (gsearch_->flags.visible == narrow) gsearch_->set_visible(!narrow);
    DockPanel::arrange(r);
}

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

void TabView::refresh_tooltips() {
    static const wchar_t* ids[3] = {L"view.side-none", L"view.preview", L"view.metadata"};
    static const wchar_t* names[3] = {L"Hide the side pane", L"Preview pane", L"Metadata pane"};
    for (int i = 0; i < 3; ++i) {
        BbOwnedStr o{};
        std::wstring gesture;
        if (bb_keymap_gesture(core_.app(), core::s(ids[i]), &o) == BB_OK) gesture = core::take(o);
        side_buttons_[i]->tooltip = names[i] + (gesture.empty() ? std::wstring() : L" (" + gesture + L")");
    }
}

void TabView::open_context(Point p, bool header) {
    main_.show_context_menu(header ? build_column_menu() : build_files_menu(false), p);
}

static BarRow command_row(core::Core& core, const wchar_t* id) {
    BarRow r;
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_command_state(core.app(), core::s(std::wstring(id)), o); });
    if (st.is_null()) return r;
    r.kind = "command";
    r.id = st["id"].str();
    r.icon = st["icon"].str();
    r.name = core::wide(st["name"].str());
    r.caption = escape_caption(core::wide(st["caption"].is_null() ? st["name"].str() : st["caption"].str()));
    r.gesture = core::wide(st["gesture"].str());
    r.enabled = st["whyNot"].is_null();
    r.reason = core::wide(st["whyNot"].str());
    r.checked = st["checked"].boolean();
    return r;
}

MenuItems TabView::build_files_menu(bool background) {
    std::vector<BarRow> rows;
    auto add = [&](const wchar_t* id) {
        BarRow r = command_row(core_, id);
        if (!r.id.empty()) rows.push_back(std::move(r));
    };
    auto sep = [&] {
        BarRow s;
        s.kind = "separator";
        if (!rows.empty() && rows.back().kind != "separator") rows.push_back(s);
    };
    auto submenu = [&](const wchar_t* caption, std::initializer_list<const wchar_t*> ids) {
        BarRow m;
        m.kind = "menu";
        m.caption = caption;
        for (const wchar_t* id : ids) {
            BarRow r = command_row(core_, id);
            if (!r.id.empty()) m.children.push_back(std::move(r));
        }
        if (!m.children.empty()) rows.push_back(std::move(m));
    };
    if (!background) {
        add(L"file.open");
        add(L"file.open-new-tab");
        sep();
        add(L"edit.cut");
        add(L"edit.copy");
        add(L"edit.paste");
        sep();
        add(L"edit.copy-path");
        add(L"file.compress");
        add(L"file.extract-here");
        sep();
        add(L"file.rename");
        add(L"file.delete");
        sep();
    } else {
        add(L"edit.paste");
        sep();
    }
    submenu(L"N&ew", {L"file.new-folder", L"file.new-file"});
    add(L"file.bookmark");
    sep();
    add(L"file.properties");
    while (!rows.empty() && rows.back().kind == "separator") rows.pop_back();
    return build_menu_items(main_.menu_env(), rows);
}

MenuItems TabView::build_column_menu() {
    BbApp* app = core_.app();
    const uint64_t tab = tab_;
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_columns_state(app, tab, o); });
    const json::Value cands = try_model([&](BbOwnedStr* o) { return bb_columns_candidates(app, tab, core::s(L""), o); });
    std::vector<std::wstring> shown;
    for (size_t i = 0; i < st["columns"].size(); ++i) {
        if (!st["columns"][i]["injected"].boolean()) shown.push_back(core::wide(st["columns"][i]["id"].str()));
    }
    auto is_shown = [&](const std::wstring& id) {
        for (const auto& s : shown) {
            if (core::iequals(s, id)) return true;
        }
        return false;
    };
    MenuItems out;
    auto entry = [&](const std::wstring& id, const std::wstring& header, bool locked) {
        MenuItem m = MenuItem::make(escape_caption(header));
        m.checked = is_shown(id);
        m.enabled = !locked;
        m.on_invoke = [app, tab, id, on = !m.checked] {
            bb_columns_do(app, tab, core::s(on ? L"add" : L"remove"), core::s(id));
        };
        out.push_back(std::move(m));
    };
    entry(L"Name", L"Name", true);
    entry(L"Size", L"Size", false);
    entry(L"Type", L"Type", false);
    entry(L"Modified", L"Modified", false);
    std::vector<std::wstring> listed = {L"Name", L"Size", L"Type", L"Modified"};
    for (size_t g = 0; g < cands["groups"].size(); ++g) {
        const json::Value& items = cands["groups"][g]["items"];
        for (size_t i = 0; i < items.size(); ++i) {
            const std::wstring id = core::wide(items[i]["id"].str());
            entry(id, core::wide(items[i]["header"].str()), false);
            listed.push_back(id);
        }
    }
    // A property added through the picker is showing but is on neither list: without a row here there
    // would be no way to turn it off again.
    bool any_extra = false;
    for (const auto& id : shown) {
        bool known = false;
        for (const auto& l : listed) known = known || core::iequals(l, id);
        if (known) continue;
        if (!any_extra) {
            out.push_back(MenuItem::separator());
            any_extra = true;
        }
        std::wstring header = id;
        for (size_t i = 0; i < st["columns"].size(); ++i) {
            if (core::iequals(core::wide(st["columns"][i]["id"].str()), id)) header = core::wide(st["columns"][i]["header"].str());
        }
        entry(id, header, false);
    }
    out.push_back(MenuItem::separator());
    out.push_back(MenuItem::make(L"More columns…", [this] { main_.open_dialog(L"columns"); }));
    out.push_back(MenuItem::make(L"Reset to default", [app, tab] { bb_columns_do(app, tab, core::s(L"reset"), core::s(L"")); }));
    out.push_back(MenuItem::make(L"Use for new tabs", [app, tab] { bb_columns_do(app, tab, core::s(L"default"), core::s(L"")); }));
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

    // The box that holds the active search shows it; typing in the other one is left alone.
    if (!search_->focused() && !gsearch_->focused()) {
        const std::wstring text = core_.tab_text(tab_, BB_TEXT_SEARCH);
        Edit* active = st.is_global_search ? gsearch_ : search_;
        Edit* other = st.is_global_search ? search_ : gsearch_;
        suppress_ = true;
        if (active->text() != text) active->set_text(text);
        if (!other->text().empty()) other->set_text(L"");
        suppress_ = false;
    }

    const std::wstring error = core_.tab_text(tab_, BB_TEXT_ERROR);
    banner_->set_text(error);
    banner_panel_->set_visible(!error.empty());

    adapter_.reload_selection();
    const std::wstring sort = core_.tab_text(tab_, BB_TEXT_SORT_BY);
    for (size_t i = 0; i < columns_.size(); ++i) {
        if (columns_[i].id == sort) list_->set_sort(static_cast<int>(i), !st.sort_descending);
    }
    sync_view_mode(st);
    sync_side_pane(st);
    refresh_tooltips();
    list_->invalidate();
    tiles_->invalidate();
}

void TabView::on_search_changed(bool global) {
    if (suppress_) return;
    Edit* box = global ? gsearch_ : search_;
    bb_tab_set_search(core_.app(), tab_, core::s(box->text()), global);
}

void TabView::type_search(const std::wstring& text, bool global) {
    Edit* box = global ? gsearch_ : search_;
    Edit* other = global ? search_ : gsearch_;
    suppress_ = true;
    box->set_text(text);
    if (!text.empty()) other->set_text(L"");
    suppress_ = false;
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
    : core_(core), window_(window), tree_provider_(std::make_unique<FolderTreeProvider>()) {
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
    browser_ = root->emplace<Panel>();
    browser_->name = "BrowserRoot";
    browser_->dock = Dock::Fill;
    auto* browser_dock = browser_->emplace<DockPanel>();
    browser_dock->name = "BrowserDock";
    rows_panel_ = browser_dock->emplace<StackPanel>(Orientation::Vertical, 0.0f);
    rows_panel_->name = "ToolbarRows";
    rows_panel_->dock = Dock::Top;
    rows_panel_->set_visible(false);
    {
        // The offer to start the search index helper: shown until answered, "not now" lasts the session.
        auto* banner = browser_dock->emplace<StackPanel>(Orientation::Horizontal, 10.0f);
        banner->name = "IndexerBanner";
        banner->dock = Dock::Top;
        banner->height = 36;
        banner->pad = 6;
        banner->style.bg = tok::WarningBackground;
        banner->set_visible(false);
        indexer_banner_ = banner;
        indexer_text_ = banner->emplace<TextBlock>();
        indexer_text_->style.fg = tok::WarningForeground;
        indexer_text_->flex = 1;
        auto* start = banner->emplace<Button>(L"Start");
        start->name = "IndexerStart";
        start->width = 90;
        start->on_click = [this] { bb_index_do(core_.app(), core::s(L"start")); };
        auto* dismiss = banner->emplace<Button>(L"Not now");
        dismiss->name = "IndexerDismiss";
        dismiss->width = 90;
        dismiss->on_click = [this] { bb_index_do(core_.app(), core::s(L"dismiss-banner")); };
    }
    browser_dock->add(make_compare_banner(*this));
    status_ = browser_dock->emplace<StatusBar>();
    status_->name = "StatusBar";
    status_->dock = Dock::Bottom;
    status_->on_right_click = [this] {
        if (index_state_["canRetry"].boolean()) bb_index_do(core_.app(), core::s(L"retry"));
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
        auto* details = strip->emplace<Button>(L"Details");
        details->name = "TransferDetails";
        details->width = 90;
        details->on_click = [this] { open_dialog(L"transfer"); };
    }
    main_split_ = browser_dock->emplace<Splitter>(Orientation::Horizontal);
    main_split_->dock = Dock::Fill;
    main_split_->name = "MainSplitter";
    main_split_->set_position(240);
    auto* sidebar_column = main_split_->emplace<DockPanel>();
    sidebar_column->name = "SidebarColumn";
    sidebar_ = sidebar_column->emplace<SavedSidebar>(*this, core_);
    sidebar_->dock = Dock::Bottom;
    tree_ = sidebar_column->emplace<TreeView>();
    tree_->dock = Dock::Fill;
    tree_->name = "FolderTree";
    tree_->set_provider(tree_provider_.get());
    pane_host_ = main_split_->emplace<Panel>();
    pane_host_->name = "PaneHostSite";

    tree_->on_select = [this](uint64_t id) {
        if (revealing_ || !active_tab_) return;
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
    dialogs_->on_closed = [this] {
        dialog_open_ = false;
        bb_ui_set_dialog_open(core_.app(), false);
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

    hwnd_ = window_.hwnd();
    window_.on_message = [this](UINT msg, WPARAM wp, LPARAM lp) { return on_window_message(msg, wp, lp); };
    window_.root().on_preview_key = [this](const KeyEvent& e) { return on_preview_key(e); };
    bb_app_set_event_sink(core_.app(), &MainView::sink, this);
}

MainView::~MainView() {
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
            break;
        }
        case BB_EVT_UNDO_CHANGED:
        case BB_EVT_OP_FINISHED:
        case BB_EVT_BUSY_CHANGED:
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
    revealing_ = true;
    for (size_t i = 0; i + 1 < chain.size(); ++i) tree_->expand(chain[i]);
    tree_->select(chain.back());
    revealing_ = false;
}

bool MainView::run_command(const std::wstring& id) {
    return bb_command_run(core_.app(), core::s(id)) == BB_OK;
}

void MainView::activate_tab(uint64_t tab) { bb_tab_activate(core_.app(), tab); }
void MainView::close_tab(uint64_t tab) { bb_tab_close(core_.app(), tab); }
void MainView::activate_pane(uint64_t pane) { bb_pane_activate(core_.app(), pane); }

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

bool MainView::on_preview_key(const KeyEvent& e) {
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
    }
    return true;
}

bool MainView::host_command(const std::wstring& id) {
    if (id == L"nav.address-bar") {
        if (TabView* tv = active_view()) tv->begin_path_edit();
        return true;
    }
    if (id == L"app.settings") {
        bb_settings_page_open(core_.app(), core::s(L""));
        sync_overlays();
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
    // Commands that put a dialog up. A run with no one to answer reports them instead.
    static const std::pair<const wchar_t*, const wchar_t*> dialogs[] = {
        {L"file.new-folder", L"new-folder"}, {L"file.new-file", L"new-file"},       {L"file.rename", L"rename"},
        {L"file.properties", L"properties"}, {L"file.compress", L"compress"},       {L"file.extract-here", L"extract"},
        {L"file.extract-to", L"extract"},    {L"file.unlock-archive", L"archive-password"},
        {L"app.about", L"about"},            {L"search.syntax", L"search-syntax"},  {L"search.save", L"saved-search"},
        {L"select.pattern", L"select-pattern"}, {L"select.unpattern", L"select-pattern"},
        {L"view.add-column", L"columns"},    {L"tools.transfer-details", L"transfer"},
        {L"app.customise-theme", L"theme-editor"},
    };
    for (const auto& [command, kind] : dialogs) {
        if (id != command) continue;
        if (headless_dialogs_) return false;
        open_dialog(kind);
        return true;
    }
    if (id == L"tools.duplicates" && !headless_dialogs_) {
        const std::wstring folder = core_.tab_text(active_tab_, BB_TEXT_PATH);
        BbOwnedStr o{};
        if (bb_duplicates_scan(core_.app(), core::s(folder), 1024 * 1024, &o) == BB_OK) {
            duplicates_.result = json::parse(core::utf8(core::take(o)));
            duplicates_.has = true;
            duplicates_.ticked.clear();
        }
        open_dialog(L"duplicates");
        return true;
    }
    return tool_host_command(*this, id);
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
            core::check(bb_bars_do(app, core::s(L"apply"), core::s(name)), "apply");
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

void MainView::open_toolbar_list(const BarRow& row, Rect anchor) {
    BarRow list = row;
    list.kind = "list";
    floating_menu_ = std::make_unique<MenuItems>(build_menu_items(menu_env(), {list}));
    if (floating_menu_->empty()) return;
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
}

void MainView::open_dialog(const std::wstring& kind) {
    if (headless_dialogs_ || !dialog_exists(kind)) return;
    close_dialog();
    auto card = build_dialog(kind, DialogContext{*this, [this] { close_dialog(); }});
    if (!card) return;
    dialog_open_ = true;
    bb_ui_set_dialog_open(core_.app(), true);
    dialogs_->show(std::move(card));
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

void MainView::set_transfer_pose(TransferPose p) {
    transfer_ = std::move(p);
    transfer_strip_->set_visible(transfer_.active);
    transfer_head_->set_text(transfer_.headline);
    transfer_detail_->set_text(transfer_.detail);
    transfer_bar_->set_indeterminate(transfer_.active && transfer_.indeterminate);
    if (!transfer_.indeterminate) transfer_bar_->set_value(transfer_.fraction);
    browser_->request_layout();
}

std::wstring MainView::transfer_text() const {
    return transfer_.active ? transfer_.headline + L"  " + transfer_.detail : std::wstring();
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
    else return false;
    return true;
}

void MainView::show_context_menu(MenuItems items, Point at) {
    floating_menu_ = std::make_unique<MenuItems>(std::move(items));
    if (floating_menu_->empty()) return;
    MenuOpen o;
    o.anchor = Rect{at.x, at.y, at.x, at.y};
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

void MainView::refresh_tooltips() {
    for (auto& kv : by_tab_) kv.second->refresh_tooltips();
}

void MainView::refresh_status() {
    if (!active_tab_) return;
    std::wstring right = core::wide(index_state_["status"].str());
    if (!right.empty() && index_state_["canRetry"].boolean()) right += L"  —  retry";
    status_->set_text(core_.tab_text(active_tab_, BB_TEXT_STATUS), right);
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
            entries.push_back({id, core_.tab_text(id, BB_TEXT_TITLE), id == static_cast<uint64_t>(p["activeTab"].i64(-1))});
        }
        pv->strip->set_tabs(std::move(entries), pv->pane() == active_pane_);
    }
    if (active_tab_) {
        reveal_in_tree(core_.tab_text(active_tab_, BB_TEXT_PATH));
    }
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
