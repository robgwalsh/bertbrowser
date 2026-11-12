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

void TabView::sync_view_mode(const BbTabState& st) {
    const bool want = st.thumbnail_scale > 0.0;
    // 64 to 256 wide, the slider's first 5% all at the smallest so it need not be landed on exactly; the height
    // is the width through the tile aspect ratio. Spec: FileListViewModel (ThumbnailSize, ThumbnailTileHeight).
    constexpr double kMin = 64, kMax = 256, kDeadZone = 0.05;
    const double s = st.thumbnail_scale;
    const float image = static_cast<float>(s < kDeadZone ? kMin : kMin + (s - kDeadZone) / (1 - kDeadZone) * (kMax - kMin));
    float image_h = image;
    if (want) {
        const json::Value ts = try_model([&](BbOwnedStr* o) { return bb_tab_tile_state(core_.app(), tab_, o); });
        const double w = ts["aspect"]["width"].num(1), h = ts["aspect"]["height"].num(1);
        if (w > 0 && h > 0) image_h = static_cast<float>(image * h / w);
    }
    if (want && !thumbs_) {
        thumbs_ = std::make_unique<ThumbnailService>(main_.window().hwnd(), thumb_source_, [this](size_t row) { return adapter_.row_path(row); });
        tiles_->set_thumbnails(thumbs_.get());
        tiles_->dpi_scale = static_cast<float>(main_.window().dpi()) / 96.0f;
    }
    if (want && (std::fabs(tiles_->metrics.image - image) > 0.5f || std::fabs(tiles_->metrics.slot_h() - image_h) > 0.5f)) {
        tiles_->metrics.image = image;
        tiles_->metrics.image_h = image_h;
        tiles_->refresh();
    }
    if (want == tiles_on_) return;
    // The keyboard follows the switch: the view that had it is about to be hidden, and arrows have to act on
    // (and scroll) the one that replaces it, from the same row. Spec: one ListView in C#, so focus never moves.
    const size_t cursor = tiles_on_ ? tiles_->cursor() : list_->cursor();
    Widget* outgoing = tiles_on_ ? static_cast<Widget*>(tiles_) : static_cast<Widget*>(list_);
    const bool had_focus = root() && root()->focused() == outgoing;
    tiles_on_ = want;
    list_->set_visible(!want);
    tiles_->set_visible(want);
    if (want) tiles_->refresh();
    else list_->refresh();
    if (had_focus) {
        root()->set_focus(want ? static_cast<Widget*>(tiles_) : static_cast<Widget*>(list_));
        if (cursor != ListView::npos) {
            if (want) tiles_->reveal_cursor_when_arranged(cursor);
            else list_->reveal_cursor_when_arranged(cursor);
        }
    }
    view_area_->request_layout();
}

PreviewPane* TabView::ensure_preview() {
    if (preview_) return preview_;
    preview_ = side_pane_->emplace<PreviewPane>(core_, tab_, main_.window().hwnd());
    preview_->on_fit_width = [this] { fit_side_width(); };
    preview_->on_next_media = [this] { select_next_media(); };
    preview_->on_context_menu = [this](MenuItems items, Point at) { main_.show_context_menu(std::move(items), at); };
    preview_->on_full_screen = [this](bool on) { main_.window().set_full_screen(on); };
    return preview_;
}

MetadataPane* TabView::ensure_metadata() {
    if (metadata_) return metadata_;
    metadata_ = side_pane_->emplace<MetadataPane>(core_, tab_, &main_.shell_icons());
    return metadata_;
}

void TabView::sync_side_pane(const BbTabState& st) {
    side_mode_ = static_cast<int>(st.side_pane);
    side_pane_->set_visible(side_mode_ != 0);
    for (int i = 0; i < 3; ++i) side_buttons_[i]->primary = i == side_mode_;
    flat_button_->primary = st.is_flat_view;  // the mode, not the listing. Spec: DirectoryTabView.xaml (IsFlatView)

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

// One folder selected in the active tab is shown in the folder tree, without going there; never while a marquee
// is still being drawn. Spec: DirectoryTabView.MirrorSelection, ShellViewModel.RequestTreeReveal.
void TabView::mirror_selection_to_tree() {
    if (main_.active_tab() != tab_ || list_->marquee_active() || tiles_->marquee_active()) return;
    const std::vector<uint32_t> sel = core_.selection(tab_);
    auto listing = core_.listing(tab_);
    BbRow row{};
    if (sel.size() != 1 || !listing || !listing->row(sel[0], row) || !(row.flags & BB_ROW_DIRECTORY)) {
        last_tree_mirror_.clear();  // selected again later, it is shown again
        return;
    }
    const std::wstring path = core::w(row.full_path);
    if (path == last_tree_mirror_) return;
    last_tree_mirror_ = path;
    main_.reveal_in_tree(path);
}

void TabView::fit_side_width() {
    if (!preview_) return;
    const float wanted = preview_->desired_width() + 16.0f;
    side_width_ = std::clamp(wanted, 180.0f, std::max(180.0f, body_->bounds.width() * 0.9f));
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
std::wstring gesture_tip(BbApp* app, const std::wstring& text, const wchar_t* command, const std::wstring& after) {
    BbOwnedStr o{};
    const std::wstring g = bb_keymap_gesture(app, core::s(command), &o) == BB_OK ? core::take(o) : std::wstring();
    return text + (g.empty() ? std::wstring() : L" (" + g + L")") + after;
}

// Every filter the box takes, with what it means: the magnifier's tooltip. Spec: Styles.xaml (SearchSyntaxToolTip).
std::wstring TabView::filters_tip() {
    static const std::wstring text = [] {
        std::wstring out = L"Filters";
        const json::Value sections = try_model([&](BbOwnedStr* o) { return bb_search_syntax_help(o); });
        for (size_t s = 0; s < sections.size(); ++s) {
            for (size_t i = 0; i < sections[s]["entries"].size(); ++i) {
                const json::Value& e = sections[s]["entries"][i];
                out += L"\n" + core::wide(e["example"].str()) + L"   \u2014   " + core::wide(e["meaning"].str());
            }
        }
        return out;
    }();
    return text;
}

void TabView::refresh_tooltips() {
    BbApp* app = core_.app();
    back_->tooltip = gesture_tip(app, L"Back", L"nav.back");
    forward_->tooltip = gesture_tip(app, L"Forward", L"nav.forward");
    up_->tooltip = gesture_tip(app, L"Up", L"nav.up");
    refresh_->tooltip = gesture_tip(app, L"Refresh", L"nav.refresh");
    search_->tooltip = gesture_tip(app, L"Search this folder and everything under it", L"search.folder",
                                   L". Filters: ext: size: dm: path: is: re: in: content:, plus OR and !");
    search_button_->tooltip = search_->tooltip;
    search_->glyph()->tooltip = filters_tip();
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
    if (main_.dialog_up()) return;  // the Apps key reaches a list behind a card; a click never could
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
    out.push_back(MenuItem::make(L"More columns\u2026", [this] { main_.open_dialog_at(L"columns", main_.window().root().pointer()); }));
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
        last_tree_mirror_.clear();
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
        if (local.empty()) collapse_search();
        else reveal_search(false);
        if (gbox) main_.set_title_search_text(st.is_global_search ? text : std::wstring());
    }
    search_->sync_buttons();

    const std::wstring error = core_.tab_text(tab_, BB_TEXT_ERROR);
    banner_->set_text(error);
    banner_panel_->set_visible(!error.empty());
    const std::wstring notice = core_.tab_text(tab_, BB_TEXT_NOTICE);
    if (notice_->text() != notice) notice_->set_text(notice);
    notice_panel_->set_visible(!notice.empty());
    sync_pick();
    // Indeterminate only while shown: its animation timer runs for as long as it is.
    const bool busy = st.is_loading || st.is_search_running;
    activity_->set_indeterminate(busy);
    activity_->set_visible(busy);
    unlock_->set_visible(!error.empty() && st.is_archive_locked);

    adapter_.reload_selection();
    mirror_selection_to_tree();
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

void TabView::reveal_search(bool focus) {
    if (!search_->flags.visible) {
        search_->set_visible(true);
        search_button_->set_visible(false);
        nav_->request_layout();
    }
    if (!focus || !root()) return;
    root()->set_focus(search_);
    search_->select(0, search_->text().size());  // a search box takes focus with its text selected
}

void TabView::collapse_search() {
    if (!search_->flags.visible || !search_->text().empty() || search_->focused()) return;
    search_->set_visible(false);
    search_button_->set_visible(true);
    nav_->request_layout();
}

void TabView::apply_icon_size() {
    const ui::IconMetrics m = ui::icon_metrics();
    nav_->height = m.bar + 4;  // the buttons keep 2 DIPs of air above and below, inside the row's own 4 of padding
    for (Button* b : {back_, forward_, up_, refresh_, search_button_, flat_button_, side_buttons_[0], side_buttons_[1], side_buttons_[2]}) {
        b->width = m.button;
        b->height = m.height();
        b->icon_px = m.icon;
    }
    nav_->request_layout();
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
    // The magnifier at the left; a click on it is a click into the box. Spec: DirectoryTabView.xaml,
    // MainWindow.xaml (the search glyph).
    glyph_ = make(icons::Search, L"", [this] {
        if (root()) root()->set_focus(this);
    });
    glyph_->set_visible(true);
    inset_left = 18.0f;
}

void SearchField::sync_buttons() {
    const bool shown = !text().empty();
    if (clear_->flags.visible == shown) return;
    clear_->set_visible(shown);
    save_->set_visible(shown && can_save);
    inset_right = shown ? (can_save ? 44.0f : 22.0f) : 0.0f;
    arrange(bounds);
    invalidate();
}

void SearchField::arrange(Rect r) {
    Edit::arrange(r);
    const float size = std::min(22.0f, r.height() - 4);
    const float t = r.t + (r.height() - size) / 2;
    glyph_->arrange(Rect::xywh(r.l + 2, t, size, size));
    clear_->arrange(Rect::xywh(r.r - 2 - size, t, size, size));
    save_->arrange(Rect::xywh(r.r - 2 - 2 * size, t, size, size));
}

void TabView::type_search(const std::wstring& text, bool global) {
    suppress_ = true;
    if (global) {
        if (!text.empty()) search_->set_text(L"");
    } else {
        search_->set_text(text);
        if (!text.empty()) reveal_search(false);
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
    // Enter and Esc leave the keyboard on a hidden box; the folders take it back. A blur that came from a
    // click elsewhere finds the focus already moved, and is left alone.
    const bool had_focus = root() && root()->focused() == path_;
    path_->set_visible(false);
    crumbs_->set_visible(true);
    path_bar_->request_layout();
    if (had_focus) root()->set_focus(tiles_on_ ? static_cast<Widget*>(tiles_) : static_cast<Widget*>(list_));
}

// ---- PaneView ----------------------------------------------------------------------------------------

bool PaneView::on_mouse_down(const MouseEvent&) {
    if (pane_ != main_.active_pane()) main_.activate_pane(pane_);
    return false;
}

// A press anywhere in the pane (its list, its search box, its breadcrumb, whatever takes the press) or Tab into
// it makes it the active one. The activation arrives as an event, so nothing under the press is rebuilt now.
// Spec: FilePaneView (PreviewMouseDown with handledEventsToo, IsKeyboardFocusWithinChanged).
void PaneView::on_inner_activity() {
    if (pane_ != main_.active_pane()) main_.activate_pane(pane_);
}

}  // namespace bb::views
