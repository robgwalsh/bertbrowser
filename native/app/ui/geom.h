// Layout geometry. Everything in the widget tree is in DIPs (1/96 inch); the surface scales.
#pragma once
#include <algorithm>
#include <cmath>

#include <d2d1.h>

namespace bb::ui {

struct Point {
    float x = 0, y = 0;
};

struct Size {
    float w = 0, h = 0;
};

constexpr float kInf = 1.0e9f;

struct Rect {
    float l = 0, t = 0, r = 0, b = 0;

    static Rect xywh(float x, float y, float w, float h) { return {x, y, x + w, y + h}; }
    float width() const { return r - l; }
    float height() const { return b - t; }
    Size size() const { return {width(), height()}; }
    bool empty() const { return r <= l || b <= t; }
    bool contains(Point p) const { return p.x >= l && p.x < r && p.y >= t && p.y < b; }
    bool intersects(const Rect& o) const { return l < o.r && o.l < r && t < o.b && o.t < b; }
    Rect intersect(const Rect& o) const {
        Rect x{std::max(l, o.l), std::max(t, o.t), std::min(r, o.r), std::min(b, o.b)};
        if (x.r < x.l || x.b < x.t) {
            return {};
        }
        return x;
    }
    Rect united(const Rect& o) const {
        if (empty()) return o;
        if (o.empty()) return *this;
        return {std::min(l, o.l), std::min(t, o.t), std::max(r, o.r), std::max(b, o.b)};
    }
    Rect offset(float dx, float dy) const { return {l + dx, t + dy, r + dx, b + dy}; }
    Rect inflate(float d) const { return {l - d, t - d, r + d, b + d}; }
    Rect deflate(float left, float top, float right, float bottom) const {
        return {l + left, t + top, std::max(l + left, r - right), std::max(t + top, b - bottom)};
    }
    bool operator==(const Rect&) const = default;
    D2D1_RECT_F d2d() const { return D2D1::RectF(l, t, r, b); }
};

}  // namespace bb::ui
