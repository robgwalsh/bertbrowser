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
}

void CaptionButton::paint(PaintContext& pc) {
    const bool close = kind == Kind::Close;
    const bool hot = hovered();
    if (hot) pc.fill(bounds, close ? tok::TitleBarCloseHoverBackground : tok::TitleBarButtonHoverBackground);
    const Token fg = hot && close ? tok::TitleBarCloseHoverForeground : tok::TitleBarForeground;
    const float cx = std::floor((bounds.l + bounds.r) / 2) + 0.5f, cy = std::floor((bounds.t + bounds.b) / 2) + 0.5f;
    switch (kind) {
        case Kind::Minimize: pc.line({cx - 5, cy}, {cx + 5, cy}, fg); break;
        case Kind::Maximize: pc.stroke(Rect{cx - 5, cy - 5, cx + 5, cy + 5}, fg); break;
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
        if (it->get() == left || it->get() == middle || it->get() == after || it->get() == center) continue;
        const float w = (*it)->width > 0 ? (*it)->width : 46.0f;
        (*it)->arrange(Rect{right - w, r.t, right, r.b});
        right -= w;
    }
    float x = r.l + 8;
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
        center->arrange(Rect{l, r.t, std::max(l, cr), r.b});
        limit = l - 8;
    }
    // `after` sits right after what `middle` needs, and `middle` gives way first when the bar is short.
    const float room = std::max(0.0f, limit - x - after_part);
    const float mid_w = after_w > 0 || (center && center->flags.visible) ? std::min(room, mid_natural) : room;
    if (middle) middle->arrange(Rect{x, r.t, x + mid_w, r.b});
    if (after && after->flags.visible) {
        const float ax = x + mid_w + 6;
        after->arrange(Rect{ax, r.t, std::min(ax + after_w, limit), r.b});
    }
}

void TitleBar::paint(PaintContext& pc) {
    Widget::paint(pc);
    gfx::FontSpec font;
    font.size = 13.0f;
    if (show_title) pc.text(title, Rect{bounds.l + 12, bounds.t, bounds.r - 150, bounds.b}, font, tok::TitleBarForeground);
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

void StatusBar::set_text(std::wstring left, std::wstring right) {
    if (left == left_ && right == right_) return;
    left_ = std::move(left);
    right_ = std::move(right);
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

void StatusBar::paint(PaintContext& pc) {
    pc.fill(bounds, tok::StatusBarBackground);
    const gfx::FontSpec font = status_font();
    const float split = right_split();
    const float limit = right_.empty() ? right_edge() : split - 8;
    pc.text(left_, Rect{bounds.l + 12, bounds.t, limit, bounds.b}, font, tok::StatusBarForeground);
    float x = bounds.l + 12 + std::ceil(pc.res.text.measure(left_, font).width) + 10;
    if (!undo_.empty() && x < limit) {
        const Rect u = undo_rect();
        pc.text(undo_, u, font, tok::StatusBarForeground);
        pc.line({u.l, u.b - 5.5f}, {u.r, u.b - 5.5f}, tok::StatusBarMutedForeground);
        x = u.r + 10;
    }
    if (!selection_.empty() && x < limit) {
        pc.text(selection_, Rect{x, bounds.t, limit, bounds.b}, font, tok::StatusBarMutedForeground);
    }
    if (!right_.empty()) {
        pc.text(right_, Rect{split, bounds.t, right_edge(), bounds.b}, font, tok::StatusBarMutedForeground,
                gfx::TextAlign::Trailing);
    }
    const Rect icon = Rect::xywh(slider_->bounds.l - kSliderIcon, bounds.t + (bounds.height() - 13) / 2, 13, 13);
    ui::Icon::draw(pc, icons::DetailsList, icon, tok::StatusBarMutedForeground, icon_geo_, icon_gen_);
}

float StatusBar::right_split() const {
    if (right_.empty()) return right_edge();
    const float w = std::ceil(res().text.measure(right_, status_font()).width) + 12.0f;
    return std::max(bounds.l + 12 + 200.0f, right_edge() - w);
}

bool StatusBar::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    if (on_undo_click && undo_rect().contains(e.pt)) {
        on_undo_click();
        return true;
    }
    if (right_.empty() || !on_right_click || e.pt.x < right_split() || e.pt.x > right_edge()) return false;
    on_right_click();
    return true;
}

Cursor StatusBar::cursor_at(Point p) const { return undo_rect().contains(p) ? Cursor::Hand : Cursor::Arrow; }

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

void TabStrip::arrange(Rect r) {
    bounds = r;
    clamp_scroll();
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
    return std::clamp(std::ceil(text) + 44.0f, 96.0f, 220.0f);
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
        // The close glyph is faint until its tab is hovered or active, so a row of tabs is not a row of X's.
        const float cx = r.r - 15.5f, cy = std::floor((r.t + r.b) / 2) + 0.5f;
        const Token x = t.active || hot ? fg : tok::TextMuted;
        pc.line({cx - 4, cy - 4}, {cx + 4, cy + 4}, x);
        pc.line({cx + 4, cy - 4}, {cx - 4, cy + 4}, x);
        if (t.active) {
            pc.fill(Rect{r.l, r.t, r.r, r.t + 2}, pane_active_ ? tok::TabActiveIndicator : tok::BorderDefault);
        }
    }
}

bool TabStrip::on_mouse_down(const MouseEvent& e) {
    const size_t i = index_at(e.pt);
    if (i == kNone) return false;
    const Rect r = tab_rect(i);
    if (e.button == MouseButton::Middle || (e.button == MouseButton::Left && e.pt.x >= r.r - 26)) {
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
    if (pressed_ != kNone) {
        if (!dragging_ && std::fabs(e.pt.x - press_at_.x) >= 5) dragging_ = true;
        if (dragging_) {
            const int gap = gap_at(e.pt.x);
            if (gap != drag_gap_) {
                drag_gap_ = gap;
                invalidate();
            }
        }
        return true;
    }
    const size_t i = index_at(e.pt);
    if (i != hot_) {
        hot_ = i;
        if (i != kNone) tooltip = tabs_[i].path;
        invalidate();
    }
    return false;
}

bool TabStrip::on_mouse_up(const MouseEvent& e) {
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

bool TabStrip::on_wheel(Point p, int delta, Mods) {
    if (p.x >= tabs_right()) return false;
    const float before = scroll_;
    scroll_ -= static_cast<float>(delta) / 120.0f * 60.0f;
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

}  // namespace bb::views
