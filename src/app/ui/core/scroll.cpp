#include "ui/core/scroll.h"

#include <windows.h>

namespace bb::ui {

namespace {
double g_wheel_multiplier = 1.0;
}

void set_wheel_multiplier(double multiplier) { g_wheel_multiplier = multiplier > 0 ? multiplier : 1.0; }

float wheel_rows(int delta) {
    UINT lines = 3;
    SystemParametersInfoW(SPI_GETWHEELSCROLLLINES, 0, &lines, 0);
    if (lines == 0 || lines == WHEEL_PAGESCROLL) lines = 3;  // "one page" falls back to the common default
    return static_cast<float>(-static_cast<double>(delta) / 120.0 * lines * g_wheel_multiplier);
}

// ---- ScrollBar -------------------------------------------------------------------------------------

Rect ScrollBar::thumb() const {
    const float len = vertical ? area.height() : area.width();
    if (len <= 0 || extent <= 0) {
        return area;
    }
    const float tl = std::clamp(len * viewport / extent, std::min(24.0f, len), len);
    const float travel = len - tl;
    const float pos = max_offset() > 0 ? travel * offset_ / max_offset() : 0.0f;
    return vertical ? Rect::xywh(area.l, area.t + pos, area.width(), tl) : Rect::xywh(area.l + pos, area.t, tl, area.height());
}

void ScrollBar::paint(PaintContext& pc) const {
    if (!needed()) return;
    pc.fill(area, tok::ScrollBarBackground);
    const Rect t = thumb().inflate(-2.0f);
    pc.fill(t, dragging_ ? tok::ScrollBarThumbHover : tok::ScrollBarThumb);
}

bool ScrollBar::mouse_down(Point p) {
    if (!needed() || !area.contains(p)) {
        return false;
    }
    const Rect t = thumb();
    if (t.contains(p)) {
        dragging_ = true;
        grab_ = vertical ? p.y - t.t : p.x - t.l;
    } else {
        const bool before = vertical ? p.y < t.t : p.x < t.l;
        set_offset(offset_ + (before ? -viewport : viewport));
    }
    return true;
}

bool ScrollBar::mouse_move(Point p) {
    if (!dragging_) {
        return false;
    }
    const Rect t = thumb();
    const float len = vertical ? area.height() : area.width();
    const float tl = vertical ? t.height() : t.width();
    const float travel = len - tl;
    if (travel > 0) {
        const float start = (vertical ? p.y : p.x) - grab_ - (vertical ? area.t : area.l);
        set_offset(start / travel * max_offset());
    }
    return true;
}

// ---- ScrollViewer ----------------------------------------------------------------------------------

Size ScrollViewer::measure(Size) { return fixed_or({0, 0}); }

void ScrollViewer::arrange(Rect r) {
    bounds = r;
    viewport_ = r;
    h_.extent = v_.extent = 0;
    if (children().empty()) {
        return;
    }
    Widget* c = children()[0].get();
    float vw = r.width(), vh = r.height();
    const float th = ScrollBar::kThickness;
    auto measure_content = [&] { return c->measure_tree({horizontal ? kInf : vw, vertical ? kInf : vh}); };
    Size d = measure_content();
    bool vbar = vertical && d.h > vh;
    if (vbar) {
        vw -= th;
        d = measure_content();
    }
    const bool hbar = horizontal && d.w > vw;
    if (hbar) {
        vh -= th;
        if (!vbar && vertical && d.h > vh) {
            vbar = true;
            vw -= th;
            d = measure_content();
        }
    }
    content_ = {horizontal ? std::max(d.w, vw) : vw, vertical ? std::max(d.h, vh) : vh};
    viewport_ = Rect::xywh(r.l, r.t, vw, vh);
    v_.viewport = vh;
    v_.extent = content_.h;
    h_.viewport = vw;
    h_.extent = content_.w;
    v_.area = vbar ? Rect{r.r - th, r.t, r.r, r.t + vh} : Rect{};
    h_.area = hbar ? Rect{r.l, r.b - th, r.l + vw, r.b} : Rect{};
    v_.set_offset(v_.offset());
    h_.set_offset(h_.offset());
    place_content();
}

void ScrollViewer::place_content() {
    if (children().empty()) return;
    children()[0]->arrange(Rect::xywh(viewport_.l - h_.offset(), viewport_.t - v_.offset(), content_.w, content_.h));
}

void ScrollViewer::scroll_to(float x, float y) {
    const float ox = h_.offset(), oy = v_.offset();
    h_.set_offset(x);
    v_.set_offset(y);
    if (ox != h_.offset() || oy != v_.offset()) {
        place_content();
        invalidate();
    }
}

void ScrollViewer::paint_children(PaintContext& pc) {
    pc.clip(viewport_.intersect(pc.dirty));
    Widget::paint_children(pc);
    pc.unclip();
    v_.paint(pc);
    h_.paint(pc);
}

Widget* ScrollViewer::hit_test(Point p) {
    if (!flags.visible || !flags.hit_testable || !bounds.contains(p)) {
        return nullptr;
    }
    if (!viewport_.contains(p)) {
        return this;  // a bar, or the corner
    }
    return Widget::hit_test(p);
}

bool ScrollViewer::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    const float oy = v_.offset(), ox = h_.offset();
    const bool took = v_.mouse_down(e.pt) || h_.mouse_down(e.pt);
    if (took) {
        root()->capture(this);
        if (oy != v_.offset() || ox != h_.offset()) {
            place_content();
            invalidate();
        }
    }
    return took;
}

bool ScrollViewer::on_mouse_move(const MouseEvent& e) {
    const float oy = v_.offset(), ox = h_.offset();
    const bool took = v_.mouse_move(e.pt) || h_.mouse_move(e.pt);
    if (took && (oy != v_.offset() || ox != h_.offset())) {
        place_content();
        invalidate();
    }
    return took;
}

bool ScrollViewer::on_mouse_up(const MouseEvent&) {
    const bool was = v_.dragging() || h_.dragging();
    v_.mouse_up();
    h_.mouse_up();
    invalidate();
    return was;
}

bool ScrollViewer::on_wheel(Point, int delta, Mods m) {
    const float step = -static_cast<float>(delta) / 120.0f * line * 3.0f;
    const float ox = h_.offset(), oy = v_.offset();
    if (m.shift || (!vertical && horizontal)) {
        scroll_by(step, 0);
    } else {
        scroll_by(0, step);
    }
    return ox != h_.offset() || oy != v_.offset() || h_.needed() || v_.needed();
}

// ---- Splitter --------------------------------------------------------------------------------------

float Splitter::clamp(float p) const {
    const float total = orientation == Orientation::Horizontal ? bounds.width() : bounds.height();
    if (total <= 0) return p;  // not laid out yet: keep what was asked for
    return std::clamp(p, min_first, std::max(min_first, total - thickness - min_second));
}

void Splitter::set_position(float p) {
    p = clamp(p);
    if (p == position_) return;
    position_ = p;
    request_layout();
    invalidate();
}

Rect Splitter::bar() const {
    return orientation == Orientation::Horizontal ? Rect::xywh(bounds.l + position_, bounds.t, thickness, bounds.height())
                                                  : Rect::xywh(bounds.l, bounds.t + position_, bounds.width(), thickness);
}

void Splitter::arrange(Rect r) {
    bounds = r;
    position_ = clamp(position_);
    const Rect b = bar();
    const bool horiz = orientation == Orientation::Horizontal;
    // Each side is measured at the size it is about to get: measured at the whole split's size, text that wraps
    // in the narrower side would be given one line's height.
    auto place = [](Widget& child, Rect to) {
        child.measure_tree({to.width(), to.height()});
        child.arrange(to);
    };
    if (children().size() > 0) place(*children()[0], horiz ? Rect{r.l, r.t, b.l, r.b} : Rect{r.l, r.t, r.r, b.t});
    if (children().size() > 1) place(*children()[1], horiz ? Rect{b.r, r.t, r.r, r.b} : Rect{r.l, b.b, r.r, r.b});
}

void Splitter::paint_children(PaintContext& pc) {
    Widget::paint_children(pc);
    pc.fill(bar(), (dragging_ || hovered()) ? tok::AccentBackground : tok::SplitterBackground);
}

Widget* Splitter::hit_test(Point p) {
    if (flags.visible && bounds.contains(p) && bar().contains(p)) {
        return this;
    }
    return Widget::hit_test(p);
}

bool Splitter::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left || !bar().contains(e.pt)) return false;
    dragging_ = true;
    grab_ = orientation == Orientation::Horizontal ? e.pt.x - bar().l : e.pt.y - bar().t;
    root()->capture(this);
    invalidate();
    return true;
}

bool Splitter::on_mouse_move(const MouseEvent& e) {
    if (!dragging_) return false;
    const float local = orientation == Orientation::Horizontal ? e.pt.x - bounds.l : e.pt.y - bounds.t;
    const float before = position_;
    set_position(local - grab_);
    if (position_ != before && on_moved) on_moved(position_);
    return true;
}

bool Splitter::on_mouse_up(const MouseEvent&) {
    const bool was = dragging_;
    dragging_ = false;
    invalidate();
    if (was && on_released) on_released();
    return was;
}

Cursor Splitter::cursor_at(Point p) const {
    if (!bar().contains(p) && !dragging_) return Cursor::Arrow;
    return orientation == Orientation::Horizontal ? Cursor::SizeWE : Cursor::SizeNS;
}

}  // namespace bb::ui
