#include "ui/controls/inputs.h"

#include <algorithm>
#include <cmath>
#include <cwctype>

namespace bb::ui {

// ---- ProgressBar ------------------------------------------------------------------------------------

void ProgressBar::set_value(double v) {
    v = std::clamp(v, 0.0, 1.0);
    if (v == value_) return;
    value_ = v;
    invalidate();
}

void ProgressBar::set_indeterminate(bool on) {
    if (on == indeterminate_) return;
    indeterminate_ = on;
    if (!on && timer_on_ && root()) {
        root()->stop_timer(this, kTimer);
        timer_on_ = false;
    }
    invalidate();
}

Rect ProgressBar::filled() const {
    if (indeterminate_) {
        // A segment 30% of the track long that sweeps in from the left and out at the right.
        const double phase = root() ? sweep_phase(root()->now_ms()) : 0.0;
        const float w = bounds.width() * 0.3f;
        const float x = bounds.l - w + static_cast<float>(phase) * (bounds.width() + w);
        return Rect{std::max(bounds.l, x), bounds.t, std::min(bounds.r, x + w), bounds.b};
    }
    return Rect{bounds.l, bounds.t, bounds.l + bounds.width() * static_cast<float>(value_), bounds.b};
}

void ProgressBar::paint(PaintContext& pc) {
    if (indeterminate_ && !timer_on_ && root()) {
        root()->start_timer(this, kTimer, 33);
        timer_on_ = true;
    }
    pc.fill(bounds, tok::ProgressTrack);
    const Rect f = filled();
    if (!f.empty()) pc.fill(f, tok::ProgressIndicator);
}

// ---- Slider -----------------------------------------------------------------------------------------

Slider::Slider(Orientation o) : orientation(o) {
    flags.focusable = true;
    flags.tab_stop = true;
}

Size Slider::measure(Size) {
    return orientation == Orientation::Horizontal ? fixed_or({160.0f, 24.0f}) : fixed_or({24.0f, 160.0f});
}

Rect Slider::track() const {
    const float h = kThumb / 2;
    if (orientation == Orientation::Horizontal) {
        const float cy = (bounds.t + bounds.b) / 2;
        return Rect{bounds.l + h, cy - 2, bounds.r - h, cy + 2};
    }
    const float cx = (bounds.l + bounds.r) / 2;
    return Rect{cx - 2, bounds.t + h, cx + 2, bounds.b - h};
}

double Slider::snap(double v) const {
    v = std::clamp(v, min, max);
    if (step > 0) v = min + std::round((v - min) / step) * step;
    return std::clamp(v, min, max);
}

void Slider::set_value(double v) {
    v = snap(v);
    if (v == value_) return;
    value_ = v;
    invalidate();
}

Rect Slider::thumb() const {
    const Rect t = track();
    const double frac = max > min ? (value_ - min) / (max - min) : 0.0;
    if (orientation == Orientation::Horizontal) {
        const float cx = t.l + static_cast<float>(frac) * t.width();
        const float cy = (t.t + t.b) / 2;
        return Rect::xywh(cx - kThumb / 2, cy - kThumb / 2, kThumb, kThumb);
    }
    const float cy = t.b - static_cast<float>(frac) * t.height();
    const float cx = (t.l + t.r) / 2;
    return Rect::xywh(cx - kThumb / 2, cy - kThumb / 2, kThumb, kThumb);
}

double Slider::value_at(Point p) const {
    const Rect t = track();
    double frac;
    if (orientation == Orientation::Horizontal) {
        frac = t.width() > 0 ? (p.x - t.l) / t.width() : 0.0;
    } else {
        frac = t.height() > 0 ? (t.b - p.y) / t.height() : 0.0;
    }
    return snap(min + std::clamp(frac, 0.0, 1.0) * (max - min));
}

void Slider::apply(double v, bool dragging) {
    const double before = value_;
    value_ = snap(v);
    invalidate();
    if ((value_ != before || !dragging) && on_change) on_change(value_, dragging);
}

void Slider::paint(PaintContext& pc) {
    Widget::paint(pc);
    const Rect t = track();
    pc.fill(t, tok::SliderTrack);
    const Rect th = thumb();
    const float cx = (th.l + th.r) / 2, cy = (th.t + th.b) / 2;
    Rect done = t;
    if (orientation == Orientation::Horizontal) {
        done.r = cx;
    } else {
        done.t = cy;
    }
    if (!done.empty()) pc.fill(done, tok::AccentBackground);
    const bool hot = hovered() || dragging_;
    if (auto* b = pc.brush(hot ? tok::SliderThumbHover : tok::SliderThumb)) {
        pc.dc->FillEllipse(D2D1::Ellipse(D2D1::Point2F(cx, cy), kThumb / 2, kThumb / 2), b);
    }
    if (focused()) {
        if (auto* b = pc.brush(tok::BorderFocus)) {
            pc.dc->DrawEllipse(D2D1::Ellipse(D2D1::Point2F(cx, cy), kThumb / 2 + 1.5f, kThumb / 2 + 1.5f), b, 1.5f);
        }
    }
}

bool Slider::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    dragging_ = true;
    root()->capture(this);
    apply(value_at(e.pt), true);  // anywhere on the track, not just on the thumb
    return true;
}

bool Slider::on_mouse_move(const MouseEvent& e) {
    if (!dragging_) return false;
    apply(value_at(e.pt), true);
    return true;
}

bool Slider::on_mouse_up(const MouseEvent& e) {
    if (!dragging_) return false;
    dragging_ = false;
    apply(value_at(e.pt), false);
    return true;
}

bool Slider::on_key_down(const KeyEvent& e) {
    double v = value_;
    switch (e.vk) {
        case VK_LEFT:
        case VK_DOWN: v -= step; break;
        case VK_RIGHT:
        case VK_UP: v += step; break;
        case VK_PRIOR: v += large_step; break;
        case VK_NEXT: v -= large_step; break;
        case VK_HOME: v = min; break;
        case VK_END: v = max; break;
        default: return false;
    }
    apply(v, false);
    return true;
}

// ---- RadioGroup -------------------------------------------------------------------------------------

void RadioGroup::set_items(std::vector<std::wstring> items) {
    items_ = std::move(items);
    enabled_.assign(items_.size(), true);
    selected_ = std::min<int>(selected_, static_cast<int>(items_.size()) - 1);
    cursor_ = std::max(0, std::min<int>(cursor_, static_cast<int>(items_.size()) - 1));
    request_layout();
    invalidate();
}

void RadioGroup::set_selected(int i) {
    if (i == selected_) return;
    selected_ = i;
    if (i >= 0) cursor_ = i;
    invalidate();
}

void RadioGroup::set_item_enabled(size_t i, bool enabled) {
    if (i < enabled_.size() && enabled_[i] != enabled) {
        enabled_[i] = enabled;
        invalidate();
    }
}

Size RadioGroup::measure(Size) {
    widths_.clear();
    float total = 0, widest = 0;
    for (const auto& s : items_) {
        const float w = kDot + kGap + std::ceil(res().text.measure(s, style.font).width) + 16.0f;
        widths_.push_back(w);
        total += w;
        widest = std::max(widest, w);
    }
    const float n = static_cast<float>(items_.size());
    return orientation == Orientation::Vertical ? fixed_or({widest, n * kRow}) : fixed_or({total, kRow});
}

Rect RadioGroup::item_rect(int i) const {
    if (i < 0 || static_cast<size_t>(i) >= items_.size()) return {};
    if (orientation == Orientation::Vertical) {
        return Rect::xywh(bounds.l, bounds.t + static_cast<float>(i) * kRow, bounds.width(), kRow);
    }
    float x = bounds.l;
    for (int k = 0; k < i && static_cast<size_t>(k) < widths_.size(); ++k) x += widths_[static_cast<size_t>(k)];
    const float w = static_cast<size_t>(i) < widths_.size() ? widths_[static_cast<size_t>(i)] : 0.0f;
    return Rect::xywh(x, bounds.t, w, kRow);
}

int RadioGroup::item_at(Point p) const {
    for (size_t i = 0; i < items_.size(); ++i) {
        if (item_rect(static_cast<int>(i)).contains(p)) return static_cast<int>(i);
    }
    return -1;
}

void RadioGroup::choose(int i) {
    if (i < 0 || static_cast<size_t>(i) >= items_.size() || !enabled_[static_cast<size_t>(i)]) return;
    cursor_ = i;
    invalidate();
    if (i == selected_) return;
    selected_ = i;
    if (on_change) on_change(i);
}

void RadioGroup::paint(PaintContext& pc) {
    Widget::paint(pc);
    for (size_t i = 0; i < items_.size(); ++i) {
        const Rect r = item_rect(static_cast<int>(i));
        const bool on = enabled_[i];
        const float cy = (r.t + r.b) / 2;
        const D2D1_ELLIPSE ring = D2D1::Ellipse(D2D1::Point2F(r.l + kDot / 2, cy), kDot / 2 - 0.5f, kDot / 2 - 0.5f);
        const bool chosen = static_cast<int>(i) == selected_;
        const bool hot = hovered() && item_at(root()->pointer()) == static_cast<int>(i);
        if (auto* b = pc.brush(chosen ? tok::AccentBackground : (hot ? tok::ButtonSecondaryHoverBackground : tok::CheckBoxBackground))) {
            pc.dc->FillEllipse(ring, b);
        }
        if (auto* b = pc.brush(focused() && static_cast<int>(i) == cursor_ ? tok::BorderFocus : tok::CheckBoxBorder)) {
            pc.dc->DrawEllipse(ring, b, 1.0f);
        }
        if (chosen) {
            if (auto* b = pc.brush(tok::AccentForeground)) {
                pc.dc->FillEllipse(D2D1::Ellipse(D2D1::Point2F(r.l + kDot / 2, cy), 3.5f, 3.5f), b);
            }
        }
        pc.text(items_[i], Rect{r.l + kDot + kGap, r.t, r.r, r.b}, style.font, on ? style.fg : tok::TextDisabled);
    }
}

bool RadioGroup::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    const int i = item_at(e.pt);
    if (i >= 0) choose(i);
    return i >= 0;
}

bool RadioGroup::on_key_down(const KeyEvent& e) {
    int dir = 0;
    switch (e.vk) {
        case VK_UP:
        case VK_LEFT: dir = -1; break;
        case VK_DOWN:
        case VK_RIGHT: dir = 1; break;
        case VK_SPACE: choose(cursor_); return true;
        default: return false;
    }
    const int n = static_cast<int>(items_.size());
    int i = cursor_;
    for (int step = 0; step < n; ++step) {
        i = (i + dir + n) % n;
        if (enabled_[static_cast<size_t>(i)]) {
            choose(i);
            break;
        }
    }
    return true;
}

// ---- ComboBox ---------------------------------------------------------------------------------------

void ComboBox::set_items(std::vector<std::wstring> items) {
    items_ = std::move(items);
    if (selected_ >= static_cast<int>(items_.size())) selected_ = -1;
    request_layout();
    invalidate();
}

void ComboBox::set_selected(int i) {
    if (i == selected_) return;
    selected_ = i;
    invalidate();
}

Size ComboBox::measure(Size) {
    float w = 0;
    for (const auto& s : items_) w = std::max(w, std::ceil(res().text.measure(s, style.font).width));
    return fixed_or({std::max(120.0f, w + 40.0f), 28.0f});
}

int ComboBox::next_starting_with(const std::vector<std::wstring>& items, int from, wchar_t ch) {
    const int n = static_cast<int>(items.size());
    const wchar_t want = static_cast<wchar_t>(std::towlower(ch));
    for (int step = 1; step <= n; ++step) {
        const int i = ((from + step) % n + n) % n;
        if (!items[static_cast<size_t>(i)].empty() && static_cast<wchar_t>(std::towlower(items[static_cast<size_t>(i)][0])) == want) return i;
    }
    return -1;
}

void ComboBox::choose(int i) {
    if (i < 0 || static_cast<size_t>(i) >= items_.size()) return;
    if (i == selected_) return;
    selected_ = i;
    invalidate();
    if (on_change) on_change(i);
}

void ComboBox::open_list() {
    PopupLayer* layer = PopupLayer::of(*this);
    if (!layer || items_.empty() || !flags.enabled) return;
    list_.clear();
    for (size_t i = 0; i < items_.size(); ++i) {
        std::wstring label;
        for (wchar_t c : items_[i]) {
            label += c;
            if (c == L'&') label += L'&';  // an ampersand in an item is not an access key
        }
        MenuItem m = MenuItem::make(std::move(label), [this, i] { choose(static_cast<int>(i)); });
        m.checked = static_cast<int>(i) == selected_;
        list_.push_back(std::move(m));
    }
    MenuOpen o;
    o.anchor = bounds;
    o.min_width = bounds.width();
    o.on_closed = [this](MenuClose) {
        open_ = false;
        invalidate();
    };
    layer->open_menu(list_, std::move(o));
    open_ = true;
    if (selected_ >= 0) layer->menus().select(0, selected_);
    invalidate();
}

void ComboBox::paint(PaintContext& pc) {
    Widget::paint(pc);
    pc.fill(bounds, tok::InputBackground);
    pc.stroke(bounds, (focused() || open_) ? tok::BorderFocus : (hovered() ? tok::BorderActive : tok::InputBorder));
    const bool has = selected_ >= 0 && static_cast<size_t>(selected_) < items_.size();
    const Rect text = bounds.deflate(8, 0, 28, 0);
    pc.text(has ? items_[static_cast<size_t>(selected_)] : placeholder, text, style.font,
            !flags.enabled ? tok::TextDisabled : (has ? tok::InputForeground : tok::TextPlaceholder));
    const float cx = bounds.r - 16, cy = (bounds.t + bounds.b) / 2;
    pc.line({cx - 4, cy - 2}, {cx, cy + 2}, tok::TextSecondary, 1.4f);
    pc.line({cx, cy + 2}, {cx + 4, cy - 2}, tok::TextSecondary, 1.4f);
}

bool ComboBox::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    open_list();
    return true;
}

bool ComboBox::on_key_down(const KeyEvent& e) {
    const int n = static_cast<int>(items_.size());
    switch (e.vk) {
        case VK_F4:
        case VK_SPACE:
        case VK_RETURN: open_list(); return true;
        case VK_DOWN:
            if (e.mods.alt) {
                open_list();
            } else if (n > 0) {
                choose(std::min(n - 1, selected_ + 1));
            }
            return true;
        case VK_UP:
            if (n > 0) choose(std::max(0, selected_ - 1));
            return true;
        case VK_HOME: choose(0); return true;
        case VK_END: choose(n - 1); return true;
        default: return false;
    }
}

bool ComboBox::on_char(wchar_t c) {
    if (c < 0x20) return false;
    const int i = next_starting_with(items_, selected_, c);
    if (i >= 0) choose(i);
    return i >= 0;
}

}  // namespace bb::ui
