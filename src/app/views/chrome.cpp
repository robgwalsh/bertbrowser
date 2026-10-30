#include "views/chrome.h"

#include "gfx/icons.gen.h"

#include <windows.h>

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace bb::ui;

namespace {
constexpr size_t kNone = static_cast<size_t>(-1);
}

// ---- title bar ---------------------------------------------------------------------------------------

CaptionButton::CaptionButton(Kind k) : kind(k) {
    width = 46.0f;
    nc_hit = k == Kind::Minimize ? HTMINBUTTON : k == Kind::Maximize ? HTMAXBUTTON : HTCLOSE;
    tooltip = k == Kind::Minimize ? L"Minimise" : k == Kind::Maximize ? L"Maximise" : L"Close";  // Spec: Window.xaml
}

static bool window_maximized(const Widget& w) { return w.root() && w.root()->window_maximized; }
static bool window_active(const Widget& w) { return !w.root() || w.root()->window_active; }

void CaptionButton::paint(PaintContext& pc) {
    const bool close = kind == Kind::Close;
    const bool hot = hovered();
    if (kind == Kind::Maximize) tooltip = window_maximized(*this) ? L"Restore" : L"Maximise";
    if (hot) pc.fill(bounds, close ? tok::TitleBarCloseHoverBackground : tok::TitleBarButtonHoverBackground);
    const Token fg = hot && close ? tok::TitleBarCloseHoverForeground
                     : window_active(*this) ? tok::TitleBarForeground
                                            : tok::TitleBarInactiveForeground;
    const float cx = std::floor((bounds.l + bounds.r) / 2) + 0.5f, cy = std::floor((bounds.t + bounds.b) / 2) + 0.5f;
    switch (kind) {
        case Kind::Minimize: pc.line({cx - 5, cy}, {cx + 5, cy}, fg); break;
        case Kind::Maximize:
            if (window_maximized(*this)) {
                // Restore: two overlapped squares, the back one showing only its top and right edges.
                pc.stroke(Rect{cx - 5, cy - 3, cx + 3, cy + 5}, fg);
                pc.line({cx - 3, cy - 3}, {cx - 3, cy - 5}, fg);
                pc.line({cx - 3, cy - 5}, {cx + 5, cy - 5}, fg);
                pc.line({cx + 5, cy - 5}, {cx + 5, cy + 3}, fg);
                pc.line({cx + 5, cy + 3}, {cx + 3, cy + 3}, fg);
            } else {
                pc.stroke(Rect{cx - 5, cy - 5, cx + 5, cy + 5}, fg);
            }
            break;
        case Kind::Close:
            pc.line({cx - 5, cy - 5}, {cx + 5, cy + 5}, fg);
            pc.line({cx + 5, cy - 5}, {cx - 5, cy + 5}, fg);
            break;
    }
}

TitleBar::TitleBar() {
    height = kHeight;
    nc_hit = HTCAPTION;
    style.bg = tok::TitleBarBackground;
    emplace<CaptionButton>(CaptionButton::Kind::Minimize);
    emplace<CaptionButton>(CaptionButton::Kind::Maximize);
    emplace<CaptionButton>(CaptionButton::Kind::Close);
}

void TitleBar::set_title(std::wstring t) {
    if (t == title) return;
    title = std::move(t);
    invalidate();
}

void TitleBar::arrange(Rect r) {
    bounds = r;
    float right = r.r;
    for (auto it = children().rbegin(); it != children().rend(); ++it) {
        if (it->get() == left || it->get() == middle || it->get() == after || it->get() == center || it->get() == app_icon) continue;
        const float w = (*it)->width > 0 ? (*it)->width : 46.0f;
        (*it)->arrange(Rect{right - w, r.t, right, r.b});
        right -= w;
    }
    // A group of fixed-height controls is laid out from the top of its rect, so give it a rect of its own height
    // in the middle of the bar rather than the whole bar's.
    auto centred_rows = [&r](Widget* w, float l, float rr) {
        const float h = std::min(r.height(), w->measure_tree({kInf, r.height()}).h);
        const float t = r.t + std::floor((r.height() - h) / 2);
        return Rect{l, t, rr, t + h};
    };
    float x = r.l + 8;
    if (app_icon && app_icon->flags.visible) {
        constexpr float kIcon = 16.0f;
        const float t = r.t + (r.height() - kIcon) / 2;
        app_icon->arrange(Rect{x, t, x + kIcon, t + kIcon});
        x += kIcon + 6;
    }
    if (left) {
        const float w = left->measure_tree({kInf, r.height()}).w;
        left->arrange(Rect{x, r.t, x + w, r.b});
        x += w + 4;
    }
    const float after_w = after && after->flags.visible ? after->measure_tree({kInf, r.height()}).w : 0.0f;
    const float after_part = after_w > 0 ? after_w + 6 : 0.0f;
    const float mid_natural = middle ? middle->measure_tree({kInf, r.height()}).w : 0.0f;
    // `center` sits in the middle of the whole bar when that leaves the toolbar its room; otherwise it moves
    // right, and only once it is down to its narrowest does the toolbar start to give way (into its overflow).
    float limit = right - 4;  // where the toolbar and the switchers must end
    if (center && center->flags.visible) {
        constexpr float kNarrowest = 160.0f;
        const float natural = center->measure_tree({kInf, r.height()}).w;
        float l = std::max((r.l + r.r) / 2 - natural / 2, x + mid_natural + after_part + 8);
        if (right - 8 - l < kNarrowest) l = std::max(x + 8, right - 8 - std::min(natural, kNarrowest));
        const float cr = std::min(l + natural, right - 8);
        center->arrange(centred_rows(center, l, std::max(l, cr)));
        limit = l - 8;
    }
    // `after` sits right after what `middle` needs, and `middle` gives way first when the bar is short.
    const float room = std::max(0.0f, limit - x - after_part);
    const float mid_w = after_w > 0 || (center && center->flags.visible) ? std::min(room, mid_natural) : room;
    if (middle) middle->arrange(Rect{x, r.t, x + mid_w, r.b});
    if (after && after->flags.visible) {
        const float ax = x + mid_w + 6;
        after->arrange(centred_rows(after, ax, std::min(ax + after_w, limit)));
    }
}

void TitleBar::paint(PaintContext& pc) {
    // Dimmed while another window is active. Spec: Window.xaml (IsActive false -> TitleBar.Inactive*).
    const bool active = window_active(*this);
    style.bg = active ? tok::TitleBarBackground : tok::TitleBarInactiveBackground;
    Widget::paint(pc);
    gfx::FontSpec font;
    font.size = 13.0f;
    if (show_title) {
        pc.text(title, Rect{bounds.l + 12, bounds.t, bounds.r - 150, bounds.b}, font,
                active ? tok::TitleBarForeground : tok::TitleBarInactiveForeground);
    }
    pc.line({bounds.l, bounds.b - 0.5f}, {bounds.r, bounds.b - 0.5f}, tok::TitleBarBorder);
}

// ---- status bar --------------------------------------------------------------------------------------

namespace {
gfx::FontSpec status_font() {
    gfx::FontSpec font;
    font.size = 12.5f;
    return font;
}
}  // namespace

StatusBar::StatusBar() {
    height = kHeight;
    slider_ = emplace<Slider>();
    slider_->name = "ThumbnailSlider";
    slider_->min = 0;
    slider_->max = 1;
    slider_->step = 0.01;
    slider_->large_step = 0.1;
    slider_->flags.tab_stop = false;
    slider_->tooltip = L"Thumbnail size \u2014 drag left for the details list";
    slider_->on_change = [this](double v, bool) {
        if (on_scale) on_scale(v);
    };
}

void StatusBar::set_text(std::wstring left, std::wstring right, std::wstring action) {
    if (left == left_ && right == right_ && action == action_) return;
    left_ = std::move(left);
    right_ = std::move(right);
    action_ = std::move(action);
    invalidate();
}

void StatusBar::set_extras(std::wstring undo, std::wstring selection) {
    if (undo == undo_ && selection == selection_) return;
    undo_ = std::move(undo);
    selection_ = std::move(selection);
    invalidate();
}

void StatusBar::set_scale(double scale) {
    if (slider_->dragging() || slider_->value() == scale) return;
    slider_->set_value(scale);
    slider_->invalidate();
}

void StatusBar::arrange(Rect r) {
    bounds = r;
    slider_->arrange(Rect{r.r - 10 - kSlider, r.t, r.r - 10, r.b});
}

float StatusBar::right_edge() const { return bounds.r - 10 - kSlider - kSliderIcon - 10; }

Rect StatusBar::undo_rect() const {
    if (undo_.empty()) return {};
    const float x = bounds.l + 12 + std::ceil(res().text.measure(left_, status_font()).width) + 10;
    const float w = std::ceil(res().text.measure(undo_, status_font()).width);
    return Rect{x, bounds.t, std::min(x + w, right_split() - 8), bounds.b};
}

static gfx::FontSpec action_font() {
    gfx::FontSpec f = status_font();
    f.weight = 600;
    return f;
}

// The index line: a 12px Indexing glyph, the status, and "Try again" in semibold when it can be. Spec:
// MainWindow.xaml (the indexing status, IndexingRetry).
float StatusBar::right_width() const {
    if (right_.empty()) return 0;
    float w = 18.0f + std::ceil(res().text.measure(right_, status_font()).width);
    if (!action_.empty()) w += 8.0f + std::ceil(res().text.measure(action_, action_font()).width);
    return w;
}

Rect StatusBar::right_rect() const {
    if (right_.empty()) return {};
    const float r = right_edge();
    return Rect{std::max(right_split(), r - right_width()), bounds.t, r, bounds.b};
}

void StatusBar::paint(PaintContext& pc) {
    pc.fill(bounds, tok::StatusBarBackground);
    const gfx::FontSpec font = status_font();
    const float split = right_split();
    const float limit = right_.empty() ? right_edge() : split - 8;
    pc.text(left_, Rect{bounds.l + 12, bounds.t, limit, bounds.b}, font, tok::StatusBarForeground);
    float x = bounds.l + 12 + std::ceil(pc.res.text.measure(left_, font).width) + 10;
    if (!undo_.empty() && x < limit) {
        // A link: underlined under the pointer. Spec: Styles.xaml (LinkButton).
        const Rect u = undo_rect();
        pc.text(undo_, u, font, tok::StatusBarForeground);
        if (hot_undo_) pc.line({u.l, u.b - 5.5f}, {u.r, u.b - 5.5f}, tok::StatusBarForeground);
        x = u.r + 10;
    }
    if (!selection_.empty() && x < limit) {
        pc.text(selection_, Rect{x, bounds.t, limit, bounds.b}, font, tok::StatusBarMutedForeground);
    }
    if (!right_.empty()) {
        const Rect rr = right_rect();
        pc.clip(rr);
        float rx = rr.l;
        ui::Icon::draw(pc, icons::Indexing, Rect::xywh(rx, bounds.t + (bounds.height() - 12) / 2, 12, 12), tok::StatusBarMutedForeground,
                       index_geo_, index_gen_);
        rx += 18;
        pc.text(right_, Rect{rx, bounds.t, rr.r, bounds.b}, font, tok::StatusBarMutedForeground);
        if (!action_.empty()) {
            rx += std::ceil(pc.res.text.measure(right_, font).width) + 8;
            pc.text(action_, Rect{rx, bounds.t, rr.r, bounds.b}, action_font(), tok::StatusBarForeground);
        }
        pc.unclip();
    }
    const Rect icon = Rect::xywh(slider_->bounds.l - kSliderIcon, bounds.t + (bounds.height() - 13) / 2, 13, 13);
    ui::Icon::draw(pc, icons::DetailsList, icon, tok::StatusBarMutedForeground, icon_geo_, icon_gen_);
}

float StatusBar::right_split() const {
    if (right_.empty()) return right_edge();
    return std::max(bounds.l + 12 + 200.0f, right_edge() - right_width() - 12.0f);
}

bool StatusBar::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    if (on_undo_click && undo_rect().contains(e.pt)) {
        on_undo_click();
        return true;
    }
    if (right_.empty() || !on_right_click || !right_rect().contains(e.pt)) return false;
    on_right_click();
    return true;
}

bool StatusBar::on_mouse_move(const MouseEvent& e) {
    const bool on_undo = undo_rect().contains(e.pt);
    if (on_undo != hot_undo_) {
        hot_undo_ = on_undo;
        invalidate();
    }
    tooltip = on_undo ? undo_tip
              : !action_.empty() && right_rect().contains(e.pt) ? L"Ask for administrator rights again so drives can be indexed"
                                                               : std::wstring();
    return false;
}

void StatusBar::on_mouse_leave() {
    if (!hot_undo_) return;
    hot_undo_ = false;
    invalidate();
}

Cursor StatusBar::cursor_at(Point p) const {
    return undo_rect().contains(p) || (!action_.empty() && right_rect().contains(p)) ? Cursor::Hand : Cursor::Arrow;
}

// ---- tab strip ---------------------------------------------------------------------------------------

TabStrip::TabStrip() {
    height = kHeight;
    auto make = [this](const char* id, const IconData& icon, std::function<void()> click) {
        auto* b = emplace<Button>();
        b->name = id;
        b->icon = &icon;
        b->style.bg = tok::TabStripBackground;
        b->style.border = tok::TabStripBackground;
        b->flags.tab_stop = false;
        b->on_click = std::move(click);
        return b;
    };
    new_tab_ = make("NewTabButton", icons::Add, [this] { if (on_new_tab) on_new_tab(); });
    split_right_ = make("SplitRightButton", icons::SplitRight, [this] { if (on_split) on_split(true); });
    split_below_ = make("SplitBelowButton", icons::SplitBelow, [this] { if (on_split) on_split(false); });
}

void TabStrip::sync_bar() {
    const float room = std::max(0.0f, bounds.r - kButtons - 4 - (bounds.l + 4));
    h_.extent = tabs_width();
    h_.viewport = room;
    h_.area = Rect{bounds.l + 4, bounds.b - 5, bounds.l + 4 + room, bounds.b - 1};
    h_.set_offset(scroll_);
}

void TabStrip::arrange(Rect r) {
    bounds = r;
    clamp_scroll();
    sync_bar();
    const float x = tabs_right() + 3;
    const float t = r.t + (r.height() - 24.0f) / 2 + 1;
    new_tab_->arrange(Rect::xywh(x, t, 24, 24));
    split_right_->arrange(Rect::xywh(x + 35, t, 24, 24));
    split_below_->arrange(Rect::xywh(x + 61, t, 24, 24));
}

void TabStrip::paint(PaintContext& pc) {
    pc.fill(bounds, tok::TabStripBackground);
    pc.clip(Rect{bounds.l, bounds.t, tabs_right(), bounds.b});
    paint_tabs(pc);
    if (drag_gap_ >= 0 && !tabs_.empty()) {
        const size_t gap = static_cast<size_t>(drag_gap_);
        const float x = gap == 0 ? tab_rect(0).l + 1 : gap >= tabs_.size() ? tab_rect(tabs_.size() - 1).r + 1 : tab_rect(gap).l - 1;
        pc.fill(Rect{x - 1, bounds.t + 4, x + 1, bounds.b - 2}, tok::AccentBackground);
    }
    pc.unclip();
    h_.paint(pc);
    const float rule = new_tab_->bounds.r + 5.5f;
    pc.line({rule, bounds.t + 8}, {rule, bounds.b - 8}, tok::BorderSubtle);
    pc.line({bounds.l, bounds.b - 0.5f}, {bounds.r, bounds.b - 0.5f}, tok::TabBorder);
}


void TabStrip::set_tabs(std::vector<TabEntry> tabs, bool pane_active) {
    tabs_ = std::move(tabs);
    pane_active_ = pane_active;
    hot_ = kNone;
    if (!dragging_) drag_gap_ = -1;
    std::wstring tip;
    for (const TabEntry& t : tabs_) {
        if (t.active) tip = t.path;
    }
    tooltip = tip;
    reveal_active();
    arrange(bounds);
    invalidate();
}

float TabStrip::tab_width(size_t i) const {
    const float text = res().text.measure(tabs_[i].title, gfx::FontSpec{}).width;
    return std::clamp(std::ceil(text) + 44.0f, 110.0f, 220.0f);  // Spec: FilePaneView.xaml (MinWidth 110)
}

float TabStrip::tabs_width() const {
    float w = 0;
    for (size_t i = 0; i < tabs_.size(); ++i) w += tab_width(i) + 2;
    return w;
}

float TabStrip::tabs_right() const {
    return std::min(bounds.l + 4 + tabs_width() - scroll_, bounds.r - kButtons - 4);
}

void TabStrip::clamp_scroll() {
    const float room = bounds.r - kButtons - 4 - (bounds.l + 4);
    scroll_ = std::clamp(scroll_, 0.0f, std::max(0.0f, tabs_width() - room));
}

void TabStrip::reveal_active() {
    for (size_t i = 0; i < tabs_.size(); ++i) {
        if (!tabs_[i].active) continue;
        const Rect r = tab_rect(i);
        const float right = bounds.r - kButtons - 4;
        if (r.l < bounds.l + 4) scroll_ -= bounds.l + 4 - r.l;
        else if (r.r > right) scroll_ += r.r - right;
    }
    clamp_scroll();
}

Rect TabStrip::tab_rect(size_t i) const {
    float x = bounds.l + 4 - scroll_;
    for (size_t k = 0; k < i && k < tabs_.size(); ++k) x += tab_width(k) + 2;
    return Rect{x, bounds.t + 3, x + (i < tabs_.size() ? tab_width(i) : 0), bounds.b};
}

size_t TabStrip::index_at(Point p) const {
    if (p.x >= tabs_right()) return kNone;
    for (size_t i = 0; i < tabs_.size(); ++i) {
        if (tab_rect(i).contains(p)) return i;
    }
    return kNone;
}

int TabStrip::gap_at(float x) const {
    for (size_t i = 0; i < tabs_.size(); ++i) {
        const Rect r = tab_rect(i);
        if (x < (r.l + r.r) / 2) return static_cast<int>(i);
    }
    return static_cast<int>(tabs_.size());
}

void TabStrip::paint_tabs(PaintContext& pc) {
    for (size_t i = 0; i < tabs_.size(); ++i) {
        const Rect r = tab_rect(i);
        if (r.r < bounds.l || r.l > tabs_right()) continue;
        const TabEntry& t = tabs_[i];
        const bool hot = i == hot_;
        pc.fill(r, t.active ? tok::TabActiveBackground : hot ? tok::TabHoverBackground : tok::TabInactiveBackground);
        const Token fg = t.active ? tok::TabActiveForeground : tok::TabInactiveForeground;
        pc.text(t.title, Rect{r.l + 10, r.t, r.r - 28, r.b}, gfx::FontSpec{}, fg);
        // The close glyph is faint until its tab is hovered or active, so a row of tabs is not a row of X's; the
        // button itself lights under the pointer. Spec: FilePaneView.xaml (the close button, StripButton hover).
        const float cx = r.r - 15.5f, cy = std::floor((r.t + r.b) / 2) + 0.5f;
        if (hot && hot_close_) pc.fill(close_rect(i), tok::ButtonSecondaryHoverBackground);
        const Token x = t.active || hot ? fg : tok::TextMuted;
        pc.line({cx - 4, cy - 4}, {cx + 4, cy + 4}, x);
        pc.line({cx + 4, cy - 4}, {cx - 4, cy + 4}, x);
        if (t.active) {
            pc.fill(Rect{r.l, r.t, r.r, r.t + 2}, pane_active_ ? tok::TabActiveIndicator : tok::BorderDefault);
        }
    }
}

Rect TabStrip::close_rect(size_t i) const {
    const Rect r = tab_rect(i);
    const float cx = r.r - 15.5f, cy = std::floor((r.t + r.b) / 2) + 0.5f;
    return Rect{cx - 8, cy - 8, cx + 8, cy + 8};
}

bool TabStrip::on_mouse_down(const MouseEvent& e) {
    if (e.button == MouseButton::Left && h_.mouse_down(e.pt)) {
        bar_drag_ = true;
        scroll_ = h_.offset();
        root()->capture(this);
        arrange(bounds);
        invalidate();
        return true;
    }
    const size_t i = index_at(e.pt);
    if (i == kNone) return false;
    if (e.button == MouseButton::Middle || (e.button == MouseButton::Left && close_rect(i).contains(e.pt))) {
        if (on_close) on_close(tabs_[i].id);
    } else if (e.button == MouseButton::Right) {
        if (on_context) on_context(tabs_[i].id, e.pt);
    } else if (e.button == MouseButton::Left) {
        pressed_ = i;
        press_at_ = e.pt;
        dragging_ = false;
        root()->capture(this);
    }
    return true;
}

bool TabStrip::on_mouse_move(const MouseEvent& e) {
    if (bar_drag_) {
        h_.mouse_move(e.pt);
        scroll_ = h_.offset();
        arrange(bounds);
        invalidate();
        return true;
    }
    if (pressed_ != kNone) {
        if (!dragging_ && std::fabs(e.pt.x - press_at_.x) >= 5) dragging_ = true;
        if (dragging_) {
            // Near either end of an overflowing strip, it scrolls to show where the tab could go.
            const float right = bounds.r - kButtons - 4;
            const float before = scroll_;
            if (e.pt.x < bounds.l + 24) scroll_ -= 12;
            else if (e.pt.x > right - 24) scroll_ += 12;
            clamp_scroll();
            if (scroll_ != before) arrange(bounds);
            const int gap = gap_at(e.pt.x);
            if (gap != drag_gap_) {
                drag_gap_ = gap;
                invalidate();
            }
        }
        return true;
    }
    const size_t i = index_at(e.pt);
    const bool over_close = i != kNone && close_rect(i).contains(e.pt);
    if (i != hot_ || over_close != hot_close_) {
        hot_ = i;
        hot_close_ = over_close;
        if (i != kNone) tooltip = over_close ? close_tip : tabs_[i].path;
        invalidate();
    }
    return false;
}

bool TabStrip::on_mouse_up(const MouseEvent& e) {
    if (bar_drag_) {
        bar_drag_ = false;
        h_.mouse_up();
        if (root() && root()->captured() == this) root()->release_capture();
        invalidate();
        return true;
    }
    if (pressed_ == kNone || e.button != MouseButton::Left) return false;
    const size_t from = pressed_;
    const int gap = drag_gap_;
    const bool dragged = dragging_;
    end_drag();
    if (from >= tabs_.size()) return true;
    const uint64_t id = tabs_[from].id;
    if (dragged && gap >= 0) {
        // The gap is counted with the dragged tab still in place, the index without it.
        const size_t to = static_cast<size_t>(gap) > from ? static_cast<size_t>(gap) - 1 : static_cast<size_t>(gap);
        if (to != from && on_move) on_move(id, static_cast<uint32_t>(to));
    }
    // Activated on the release, not the press: activating rebuilds the panes, strip and all, which would
    // end a drag that had only just begun. Either call may destroy this strip, so nothing follows them.
    if (on_activate) on_activate(id);
    return true;
}

void TabStrip::end_drag() {
    pressed_ = kNone;
    dragging_ = false;
    drag_gap_ = -1;
    if (root() && root()->captured() == this) root()->release_capture();
    invalidate();
}

void TabStrip::on_modal_cancel() { end_drag(); }

// Esc puts a dragged tab back where it was. Spec: ListReorderDrag.OnKey.
bool TabStrip::cancel_capture() {
    if (pressed_ == kNone && !bar_drag_) return false;
    bar_drag_ = false;
    h_.mouse_up();
    end_drag();
    return true;
}

Cursor TabStrip::cursor_at(Point) const { return dragging_ ? Cursor::SizeWE : Cursor::Arrow; }

bool TabStrip::on_wheel(Point p, int delta, Mods) {
    if (p.x >= tabs_right()) return false;
    const float before = scroll_;
    scroll_ -= static_cast<float>(delta) / 120.0f * 120.0f;  // Spec: FilePaneView (120 a notch)
    clamp_scroll();
    if (scroll_ == before) return false;
    arrange(bounds);
    invalidate();
    return true;
}

void TabStrip::on_mouse_leave() {
    if (hot_ != kNone) {
        hot_ = kNone;
        invalidate();
    }
}

// ---- breadcrumb --------------------------------------------------------------------------------------

void BreadcrumbBar::set_crumbs(std::vector<core::Crumb> crumbs) {
    crumbs_ = std::move(crumbs);
    key_crumb_ = crumbs_.empty() ? 0 : crumbs_.size() - 1;
    back_ = 0;  // scrolled to the right end, where the folder you are in is. Spec: DirectoryTabView (ScrollToRightEnd)
    laid_out_ = false;
    hot_ = kNone;
    invalidate();
}

void BreadcrumbBar::arrange(Rect r) {
    bounds = r;
    laid_out_ = false;
}

void BreadcrumbBar::lay_out() {
    segs_.clear();
    laid_out_ = true;
    const float sep = 14.0f, pad = 6.0f;
    std::vector<float> widths(crumbs_.size());
    for (size_t i = 0; i < crumbs_.size(); ++i) {
        widths[i] = std::ceil(res().text.measure(crumbs_[i].name, gfx::FontSpec{}).width) + 2 * pad;
    }
    // Keep as many trailing crumbs as fit, ending `back_` short of the last: the folder you are in matters
    // most. An ellipsis at either end says there is more that way.
    const float dots = 16.0f;
    back_ = std::min(back_, crumbs_.empty() ? size_t{0} : crumbs_.size() - 1);
    const size_t end = crumbs_.size() - back_;
    more_right_ = back_ > 0;
    const float room = bounds.width() - 8 - (more_right_ ? dots : 0);
    float used = 0;
    size_t first = end;
    while (first > 0) {
        const float need = widths[first - 1] + (first < end ? sep : 0);
        if (used + need > room - (first > 1 ? dots : 0) && first < end) break;
        used += need;
        --first;
    }
    more_left_ = first > 0;
    float x = bounds.l + 4 + (more_left_ ? dots : 0);
    for (size_t i = first; i < end; ++i) {
        if (i > first) x += sep;
        segs_.push_back({i, Rect{x, bounds.t + 2, x + widths[i], bounds.b - 2}});
        x += widths[i];
    }
}

size_t BreadcrumbBar::seg_at(Point p) const {
    for (size_t i = 0; i < segs_.size(); ++i) {
        if (segs_[i].rect.contains(p)) return i;
    }
    return kNone;
}

void BreadcrumbBar::paint(PaintContext& pc) {
    if (!laid_out_) lay_out();
    pc.fill(bounds, tok::InputBackground);
    pc.stroke(bounds, tok::InputBorder);
    if (more_left_) pc.text(L"\u2026", Rect{bounds.l + 6, bounds.t, bounds.l + 20, bounds.b}, gfx::FontSpec{}, tok::TextMuted);
    if (more_right_ && !segs_.empty()) {
        const float x = segs_.back().rect.r + 4;
        pc.text(L"\u2026", Rect{x, bounds.t, x + 14, bounds.b}, gfx::FontSpec{}, tok::TextMuted);
    }
    for (size_t i = 0; i < segs_.size(); ++i) {
        const Seg& sg = segs_[i];
        if (i == hot_) pc.fill(sg.rect, tok::ButtonSecondaryHoverBackground);
        if (focused() && sg.crumb == key_crumb_) pc.stroke(sg.rect, tok::BorderFocus);
        pc.text(crumbs_[sg.crumb].name, sg.rect.deflate(6, 0, 6, 0), gfx::FontSpec{},
                sg.crumb + 1 == crumbs_.size() ? tok::InputForeground : tok::TextSecondary);
        if (i + 1 < segs_.size()) {
            const float cx = sg.rect.r + 6.5f, cy = std::floor((bounds.t + bounds.b) / 2) + 0.5f;
            pc.line({cx - 2, cy - 4}, {cx + 2, cy}, tok::TextMuted);
            pc.line({cx + 2, cy}, {cx - 2, cy + 4}, tok::TextMuted);
        }
    }
}

bool BreadcrumbBar::on_mouse_down(const MouseEvent& e) {
    if (e.button == MouseButton::Middle) {
        if (!laid_out_) lay_out();
        const size_t i = seg_at(e.pt);
        if (i == kNone || !on_middle) return false;
        on_middle(crumbs_[segs_[i].crumb].path);
        return true;
    }
    if (e.button != MouseButton::Left) return false;
    if (!laid_out_) lay_out();
    const size_t i = seg_at(e.pt);
    if (i != kNone) {
        if (on_navigate) on_navigate(crumbs_[segs_[i].crumb].path);
    } else if (on_edit) {
        on_edit();
    }
    return true;
}

bool BreadcrumbBar::on_mouse_move(const MouseEvent& e) {
    if (!laid_out_) lay_out();
    const size_t i = seg_at(e.pt);
    if (i != hot_) {
        hot_ = i;
        invalidate();
    }
    return false;
}

bool BreadcrumbBar::on_wheel(Point, int delta, Mods) {
    if (!laid_out_) lay_out();
    if (delta > 0 && more_left_) ++back_;
    else if (delta < 0 && back_ > 0) --back_;
    else return false;
    laid_out_ = false;
    hot_ = kNone;
    invalidate();
    return true;
}

void BreadcrumbBar::on_mouse_leave() {
    if (hot_ != kNone) {
        hot_ = kNone;
        invalidate();
    }
}

Cursor BreadcrumbBar::cursor_at(Point p) const { return seg_at(p) != kNone ? Cursor::Hand : Cursor::IBeam; }

void BreadcrumbBar::on_focus(bool f) {
    // Tabbing in lands on the folder you are in, the last crumb.
    if (f) {
        key_crumb_ = crumbs_.empty() ? 0 : crumbs_.size() - 1;
        show_key_crumb();
    }
    invalidate();
}

void BreadcrumbBar::show_key_crumb() {
    if (!laid_out_) lay_out();
    auto shown = [this] {
        for (const Seg& sg : segs_) {
            if (sg.crumb == key_crumb_) return true;
        }
        return segs_.empty();
    };
    while (!shown()) {
        const bool left = !segs_.empty() && key_crumb_ < segs_.front().crumb;
        if (left && more_left_) ++back_;
        else if (!left && back_ > 0) --back_;
        else break;
        lay_out();
    }
}

bool BreadcrumbBar::on_key_down(const KeyEvent& e) {
    if (crumbs_.empty() || e.mods.ctrl || e.mods.alt) return false;
    const size_t last = crumbs_.size() - 1;
    size_t to = key_crumb_;
    switch (e.vk) {
        case VK_LEFT: to = key_crumb_ > 0 ? key_crumb_ - 1 : 0; break;
        case VK_RIGHT: to = std::min(key_crumb_ + 1, last); break;
        case VK_HOME: to = 0; break;
        case VK_END: to = last; break;
        case VK_RETURN:
        case VK_SPACE:
            if (on_navigate && key_crumb_ <= last) on_navigate(crumbs_[key_crumb_].path);
            return true;
        default: return false;
    }
    key_crumb_ = to;
    hot_ = kNone;
    show_key_crumb();
    invalidate();
    return true;
}

}  // namespace bb::views
