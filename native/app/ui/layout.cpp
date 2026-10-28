#include "ui/layout.h"

#include <algorithm>

namespace bb::ui {

Size Panel::measure(Size avail) {
    Size m;
    for (auto& c : children()) {
        if (!c->flags.visible) continue;
        const Size s = c->measure_tree({std::max(0.0f, avail.w - 2 * pad), std::max(0.0f, avail.h - 2 * pad)});
        m.w = std::max(m.w, s.w);
        m.h = std::max(m.h, s.h);
    }
    return fixed_or({m.w + 2 * pad, m.h + 2 * pad});
}

void Panel::arrange(Rect r) {
    bounds = r;
    const Rect inner = r.deflate(pad, pad, pad, pad);
    for (auto& c : children()) {
        if (c->flags.visible) c->arrange(inner);
    }
}

Size StackPanel::measure(Size avail) {
    const bool vert = orientation == Orientation::Vertical;
    const Size inner{std::max(0.0f, avail.w - 2 * pad), std::max(0.0f, avail.h - 2 * pad)};
    float main = 0, cross = 0;
    int n = 0;
    for (auto& c : children()) {
        if (!c->flags.visible) continue;
        const Size s = c->measure_tree(vert ? Size{inner.w, kInf} : Size{kInf, inner.h});
        main += vert ? s.h : s.w;
        cross = std::max(cross, vert ? s.w : s.h);
        ++n;
    }
    if (n > 1) main += spacing * static_cast<float>(n - 1);
    return fixed_or(vert ? Size{cross + 2 * pad, main + 2 * pad} : Size{main + 2 * pad, cross + 2 * pad});
}

void StackPanel::arrange(Rect r) {
    bounds = r;
    const bool vert = orientation == Orientation::Vertical;
    const Rect inner = r.deflate(pad, pad, pad, pad);
    const float total = vert ? inner.height() : inner.width();
    float fixed = 0, flex_sum = 0;
    int n = 0;
    for (auto& c : children()) {
        if (!c->flags.visible) continue;
        ++n;
        if (c->flex > 0) {
            flex_sum += c->flex;
        } else {
            fixed += vert ? c->desired().h : c->desired().w;
        }
    }
    const float leftover = std::max(0.0f, total - fixed - (n > 1 ? spacing * static_cast<float>(n - 1) : 0.0f));
    float pos = vert ? inner.t : inner.l;
    for (auto& c : children()) {
        if (!c->flags.visible) continue;
        const float main = c->flex > 0 ? leftover * c->flex / flex_sum : (vert ? c->desired().h : c->desired().w);
        const float cross_fixed = vert ? c->width : c->height;
        const float cross = cross_fixed >= 0 ? cross_fixed : (vert ? inner.width() : inner.height());
        if (vert) {
            c->arrange(Rect::xywh(inner.l, pos, cross, main));
        } else {
            c->arrange(Rect::xywh(pos, inner.t, main, cross));
        }
        pos += main + spacing;
    }
}

Size DockPanel::measure(Size avail) {
    float used_w = 0, used_h = 0, max_w = 0, max_h = 0;
    for (auto& c : children()) {
        if (!c->flags.visible) continue;
        const Size s = c->measure_tree({std::max(0.0f, avail.w - used_w), std::max(0.0f, avail.h - used_h)});
        switch (c->dock) {
            case Dock::Left:
            case Dock::Right:
                used_w += s.w;
                max_h = std::max(max_h, used_h + s.h);
                break;
            case Dock::Top:
            case Dock::Bottom:
                used_h += s.h;
                max_w = std::max(max_w, used_w + s.w);
                break;
            case Dock::Fill:
                max_w = std::max(max_w, used_w + s.w);
                max_h = std::max(max_h, used_h + s.h);
                break;
        }
    }
    return fixed_or({std::max(max_w, used_w) + 2 * pad, std::max(max_h, used_h) + 2 * pad});
}

void DockPanel::arrange(Rect r) {
    bounds = r;
    Rect rem = r.deflate(pad, pad, pad, pad);
    for (auto& c : children()) {
        if (!c->flags.visible) continue;
        const Size d = c->desired();
        switch (c->dock) {
            case Dock::Left: {
                const float w = std::min(d.w, rem.width());
                c->arrange({rem.l, rem.t, rem.l + w, rem.b});
                rem.l += w;
                break;
            }
            case Dock::Right: {
                const float w = std::min(d.w, rem.width());
                c->arrange({rem.r - w, rem.t, rem.r, rem.b});
                rem.r -= w;
                break;
            }
            case Dock::Top: {
                const float h = std::min(d.h, rem.height());
                c->arrange({rem.l, rem.t, rem.r, rem.t + h});
                rem.t += h;
                break;
            }
            case Dock::Bottom: {
                const float h = std::min(d.h, rem.height());
                c->arrange({rem.l, rem.b - h, rem.r, rem.b});
                rem.b -= h;
                break;
            }
            case Dock::Fill:
                c->arrange(rem);
                break;
        }
    }
}

}  // namespace bb::ui
