#include "views/chrome.h"

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
        if (it->get() == left || it->get() == middle) continue;
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
    if (middle) middle->arrange(Rect{x, r.t, std::max(x, right - 4), r.b});
}

void TitleBar::paint(PaintContext& pc) {
    Widget::paint(pc);
    gfx::FontSpec font;
    font.size = 13.0f;
    if (show_title) pc.text(title, Rect{bounds.l + 12, bounds.t, bounds.r - 150, bounds.b}, font, tok::TitleBarForeground);
    pc.line({bounds.l, bounds.b - 0.5f}, {bounds.r, bounds.b - 0.5f}, tok::TitleBarBorder);
}

// ---- status bar --------------------------------------------------------------------------------------

void StatusBar::set_text(std::wstring left, std::wstring right) {
    if (left == left_ && right == right_) return;
    left_ = std::move(left);
    right_ = std::move(right);
    invalidate();
}

void StatusBar::paint(PaintContext& pc) {
    pc.fill(bounds, tok::StatusBarBackground);
    gfx::FontSpec font;
    font.size = 12.5f;
    const float split = right_split();
    pc.text(left_, Rect{bounds.l + 12, bounds.t, right_.empty() ? bounds.r - 12 : split - 8, bounds.b}, font,
            tok::StatusBarForeground);
    if (!right_.empty()) {
        pc.text(right_, Rect{split, bounds.t, bounds.r - 12, bounds.b}, font, tok::StatusBarMutedForeground,
                gfx::TextAlign::Trailing);
    }
}

float StatusBar::right_split() const {
    if (right_.empty()) return bounds.r;
    gfx::FontSpec font;
    font.size = 12.5f;
    const float w = std::ceil(res().text.measure(right_, font).width) + 12.0f;
    return std::max(bounds.l + 12 + 200.0f, bounds.r - 12 - w);
}

bool StatusBar::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left || right_.empty() || !on_right_click || e.pt.x < right_split()) return false;
    on_right_click();
    return true;
}

// ---- tab strip ---------------------------------------------------------------------------------------

void TabStrip::paint(PaintContext& pc) {
    paint_tabs(pc);
    if (drag_gap_ < 0) return;
    const size_t gap = static_cast<size_t>(drag_gap_);
    const float x = gap == 0 ? bounds.l + 2 : gap >= tabs_.size() ? tab_rect(tabs_.size() - 1).r + 1 : tab_rect(gap).l - 1;
    pc.fill(Rect{x - 1, bounds.t + 4, x + 1, bounds.b - 2}, tok::AccentBackground);
}

void TabStrip::set_tabs(std::vector<TabEntry> tabs, bool pane_active) {
    tabs_ = std::move(tabs);
    pane_active_ = pane_active;
    hot_ = kNone;
    drag_gap_ = -1;
    invalidate();
}

float TabStrip::tab_width(size_t i) const {
    const float text = res().text.measure(tabs_[i].title, gfx::FontSpec{}).width;
    return std::clamp(std::ceil(text) + 44.0f, 96.0f, 220.0f);
}

Rect TabStrip::tab_rect(size_t i) const {
    float x = bounds.l + 4;
    for (size_t k = 0; k < i && k < tabs_.size(); ++k) x += tab_width(k) + 2;
    return Rect{x, bounds.t + 3, x + (i < tabs_.size() ? tab_width(i) : 0), bounds.b};
}

size_t TabStrip::index_at(Point p) const {
    for (size_t i = 0; i < tabs_.size(); ++i) {
        if (tab_rect(i).contains(p)) return i;
    }
    return kNone;
}

void TabStrip::paint_tabs(PaintContext& pc) {
    pc.fill(bounds, tok::TabStripBackground);
    for (size_t i = 0; i < tabs_.size(); ++i) {
        const Rect r = tab_rect(i);
        const TabEntry& t = tabs_[i];
        const bool hot = i == hot_;
        pc.fill(r, t.active ? tok::TabActiveBackground : hot ? tok::TabHoverBackground : tok::TabInactiveBackground);
        const Token fg = t.active ? tok::TabActiveForeground : tok::TabInactiveForeground;
        pc.text(t.title, Rect{r.l + 10, r.t, r.r - 28, r.b}, gfx::FontSpec{}, fg);
        if (t.active || hot) {
            const float cx = r.r - 15.5f, cy = std::floor((r.t + r.b) / 2) + 0.5f;
            pc.line({cx - 4, cy - 4}, {cx + 4, cy + 4}, fg);
            pc.line({cx + 4, cy - 4}, {cx - 4, cy + 4}, fg);
        }
        if (t.active) {
            pc.fill(Rect{r.l, r.t, r.r, r.t + 2}, pane_active_ ? tok::TabActiveIndicator : tok::BorderDefault);
        }
    }
    pc.line({bounds.l, bounds.b - 0.5f}, {bounds.r, bounds.b - 0.5f}, tok::TabBorder);
}

bool TabStrip::on_mouse_down(const MouseEvent& e) {
    const size_t i = index_at(e.pt);
    if (i == kNone) return false;
    const Rect r = tab_rect(i);
    if (e.button == MouseButton::Middle || (e.button == MouseButton::Left && e.pt.x >= r.r - 26)) {
        if (on_close) on_close(tabs_[i].id);
    } else if (e.button == MouseButton::Left) {
        if (on_activate) on_activate(tabs_[i].id);
    }
    return true;
}

bool TabStrip::on_mouse_move(const MouseEvent& e) {
    const size_t i = index_at(e.pt);
    if (i != hot_) {
        hot_ = i;
        invalidate();
    }
    return false;
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
    // Keep as many trailing crumbs as fit; the folder you are in matters most.
    const float room = bounds.width() - 8;
    float used = 0;
    size_t first = crumbs_.size();
    while (first > 0) {
        const float need = widths[first - 1] + (first < crumbs_.size() ? sep : 0);
        if (used + need > room && first < crumbs_.size()) break;
        used += need;
        --first;
    }
    float x = bounds.l + 4;
    for (size_t i = first; i < crumbs_.size(); ++i) {
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

void BreadcrumbBar::on_mouse_leave() {
    if (hot_ != kNone) {
        hot_ = kNone;
        invalidate();
    }
}

Cursor BreadcrumbBar::cursor_at(Point p) const { return seg_at(p) != kNone ? Cursor::Hand : Cursor::IBeam; }

}  // namespace bb::views
