#include "ui/controls/tabs.h"

#include <algorithm>
#include <cmath>

namespace bb::ui {

int TabStrip::add(Tab t, bool sel) {
    tabs_.push_back(std::move(t));
    icon_geo_.emplace_back();
    icon_gen_.push_back(0);
    const int i = static_cast<int>(tabs_.size()) - 1;
    if (sel || selected_ < 0) selected_ = i;
    invalidate();
    return i;
}

void TabStrip::remove(int index) {
    if (index < 0 || index >= count()) return;
    tabs_.erase(tabs_.begin() + index);
    icon_geo_.erase(icon_geo_.begin() + index);
    icon_gen_.erase(icon_gen_.begin() + index);
    if (tabs_.empty()) {
        selected_ = -1;
    } else if (selected_ == index) {
        selected_ = std::min(index, count() - 1);
    } else if (selected_ > index) {
        --selected_;
    }
    hover_tab_ = -1;
    invalidate();
}

void TabStrip::move(int from, int to) {
    if (from < 0 || from >= count() || to < 0 || to >= count() || from == to) return;
    auto rotate = [&](auto& v) {
        auto item = std::move(v[static_cast<size_t>(from)]);
        v.erase(v.begin() + from);
        v.insert(v.begin() + to, std::move(item));
    };
    rotate(tabs_);
    rotate(icon_geo_);
    rotate(icon_gen_);
    if (selected_ == from) {
        selected_ = to;
    } else if (from < selected_ && selected_ <= to) {
        --selected_;
    } else if (to <= selected_ && selected_ < from) {
        ++selected_;
    }
    invalidate();
}

void TabStrip::select(int index) {
    if (index < -1 || index >= count() || index == selected_) return;
    selected_ = index;
    invalidate();
}

void TabStrip::set_title(int index, std::wstring title) {
    if (index < 0 || index >= count()) return;
    tabs_[static_cast<size_t>(index)].title = std::move(title);
    invalidate();
}

int TabStrip::index_of_id(uint64_t id) const {
    for (int i = 0; i < count(); ++i) {
        if (tabs_[static_cast<size_t>(i)].id == id) return i;
    }
    return -1;
}

Size TabStrip::measure(Size) { return fixed_or({0.0f, kHeight}); }

float TabStrip::tab_width() const {
    const float avail = bounds.width() - (show_new_button ? kNewWidth : 0.0f);
    if (tabs_.empty()) return kMaxTab;
    return std::clamp(avail / static_cast<float>(tabs_.size()), kMinTab, kMaxTab);
}

Rect TabStrip::tab_rect(int i) const {
    const float w = tab_width();
    return Rect::xywh(bounds.l + static_cast<float>(i) * w, bounds.t, w, bounds.height());
}

Rect TabStrip::close_rect(int i) const {
    const Rect r = tab_rect(i);
    const float cy = (r.t + r.b) / 2;
    return Rect::xywh(r.r - 8 - kCloseSize, cy - kCloseSize / 2, kCloseSize, kCloseSize);
}

Rect TabStrip::new_rect() const {
    const float avail = bounds.width() - kNewWidth;
    const float used = std::min(avail, static_cast<float>(tabs_.size()) * tab_width());
    return Rect::xywh(bounds.l + used, bounds.t, kNewWidth, bounds.height());
}

int TabStrip::tab_at(Point p) const {
    if (!bounds.contains(p) || tabs_.empty()) return -1;
    if (show_new_button && new_rect().contains(p)) return -1;
    const int i = static_cast<int>(std::floor((p.x - bounds.l) / tab_width()));
    return i >= 0 && i < count() ? i : -1;
}

void TabStrip::paint(PaintContext& pc) {
    pc.fill(bounds, tok::TabStripBackground);
    pc.clip(bounds);
    for (int i = 0; i < count(); ++i) {
        const Rect r = tab_rect(i);
        if (!r.intersects(pc.dirty)) continue;
        const Tab& t = tabs_[static_cast<size_t>(i)];
        const bool active = i == selected_;
        const bool hot = i == hover_tab_;
        pc.fill(r, active ? tok::TabActiveBackground : (hot ? tok::TabHoverBackground : tok::TabInactiveBackground));
        if (active) pc.fill(Rect{r.l, r.t, r.r, r.t + 2}, tok::TabActiveIndicator);
        pc.line({r.r - 0.5f, r.t + 7}, {r.r - 0.5f, r.b - 7}, tok::TabBorder);
        const Token fg = active ? tok::TabActiveForeground : tok::TabInactiveForeground;
        float x = r.l + 12;
        if (t.icon) {
            Icon::draw(pc, *t.icon, Rect::xywh(x, (r.t + r.b) / 2 - 8, 16, 16), fg, icon_geo_[static_cast<size_t>(i)],
                       icon_gen_[static_cast<size_t>(i)]);
            x += 22;
        }
        const bool show_close = t.closable && (active || hot);
        pc.text(t.title, Rect{x, r.t, r.r - (t.closable ? kCloseSize + 12 : 8.0f), r.b}, style.font, fg);
        if (show_close) {
            const Rect c = close_rect(i);
            if (hot && hover_close_) pc.fill(c, tok::ToolbarHoverBackground);
            const float cx = (c.l + c.r) / 2, cy = (c.t + c.b) / 2;
            pc.line({cx - 4, cy - 4}, {cx + 4, cy + 4}, fg, 1.3f);
            pc.line({cx + 4, cy - 4}, {cx - 4, cy + 4}, fg, 1.3f);
        }
    }
    if (show_new_button) {
        const Rect n = new_rect();
        if (hover_new_) pc.fill(n.deflate(4, 6, 4, 6), tok::ToolbarHoverBackground);
        const float cx = (n.l + n.r) / 2, cy = (n.t + n.b) / 2;
        pc.line({cx - 5, cy}, {cx + 5, cy}, tok::TabInactiveForeground, 1.3f);
        pc.line({cx, cy - 5}, {cx, cy + 5}, tok::TabInactiveForeground, 1.3f);
    }
    pc.line({bounds.l, bounds.b - 0.5f}, {bounds.r, bounds.b - 0.5f}, tok::TabBorder);
    pc.unclip();
}

bool TabStrip::on_mouse_down(const MouseEvent& e) {
    const int i = tab_at(e.pt);
    if (e.button == MouseButton::Middle) {
        if (i >= 0 && tabs_[static_cast<size_t>(i)].closable && on_close) on_close(i);
        return i >= 0;
    }
    if (e.button != MouseButton::Left) return false;
    if (show_new_button && new_rect().contains(e.pt)) {
        press_new_ = true;
        root()->capture(this);
        return true;
    }
    if (i < 0) return false;
    root()->capture(this);
    press_tab_ = i;
    press_x_ = e.pt.x;
    drag_from_ = i;
    const Tab& t = tabs_[static_cast<size_t>(i)];
    if (t.closable && (i == selected_ || i == hover_tab_) && close_rect(i).contains(e.pt)) {
        press_close_ = true;
        return true;
    }
    if (i != selected_) {
        selected_ = i;
        invalidate();
        if (on_select) on_select(i);
    }
    return true;
}

bool TabStrip::on_mouse_move(const MouseEvent& e) {
    const int i = tab_at(e.pt);
    const bool close = i >= 0 && tabs_[static_cast<size_t>(i)].closable && close_rect(i).contains(e.pt);
    const bool nw = show_new_button && new_rect().contains(e.pt);
    if (i != hover_tab_ || close != hover_close_ || nw != hover_new_) {
        hover_tab_ = i;
        hover_close_ = close;
        hover_new_ = nw;
        invalidate();
    }
    if (press_tab_ >= 0 && !press_close_) {
        if (!reordering_ && std::fabs(e.pt.x - press_x_) > kDragThreshold) reordering_ = true;
        if (reordering_) {
            const int target = std::clamp(static_cast<int>(std::floor((e.pt.x - bounds.l) / tab_width())), 0, count() - 1);
            if (target != press_tab_) {
                move(press_tab_, target);
                press_tab_ = target;
            }
        }
        return true;
    }
    return false;
}

bool TabStrip::on_mouse_up(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    const int tab = press_tab_;
    const bool was_close = press_close_, was_new = press_new_, was_reorder = reordering_;
    press_tab_ = -1;
    press_close_ = press_new_ = reordering_ = false;
    if (was_reorder) {
        if (tab != drag_from_ && on_reorder) on_reorder(drag_from_, tab);
    } else if (was_close && tab >= 0) {
        if (close_rect(tab).contains(e.pt) && on_close) on_close(tab);
    } else if (was_new) {
        if (new_rect().contains(e.pt) && on_new) on_new();
    }
    invalidate();
    return true;
}

void TabStrip::on_mouse_leave() {
    if (hover_tab_ >= 0 || hover_new_) {
        hover_tab_ = -1;
        hover_close_ = hover_new_ = false;
        invalidate();
    }
}

bool TabStrip::on_key_down(const KeyEvent& e) {
    if (tabs_.empty() || e.mods.ctrl || e.mods.alt) return false;
    int to = selected_;
    switch (e.vk) {
        case VK_LEFT: to = std::max(0, selected_ - 1); break;
        case VK_RIGHT: to = std::min(count() - 1, selected_ + 1); break;
        case VK_HOME: to = 0; break;
        case VK_END: to = count() - 1; break;
        default: return false;
    }
    if (to != selected_) {
        selected_ = to;
        invalidate();
        if (on_select) on_select(to);
    }
    return true;
}

}  // namespace bb::ui
