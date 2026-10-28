#include "ui/controls.h"

#include <cmath>

namespace bb::ui {

// ---- TextBlock -------------------------------------------------------------------------------------

void TextBlock::set_text(std::wstring t) {
    if (t == text_) return;
    text_ = std::move(t);
    request_layout();
    invalidate();
}

Size TextBlock::measure(Size avail) {
    gfx::LayoutOptions opt;
    if (wrap) {
        opt.wrap = true;
        opt.max_width = std::min(avail.w, 1.0e6f);
    }
    auto layout = res().text.make_layout(text_.empty() ? L" " : text_, style.font, opt);
    DWRITE_TEXT_METRICS m{};
    if (layout) layout->GetMetrics(&m);
    return fixed_or({text_.empty() ? 0.0f : std::ceil(m.widthIncludingTrailingWhitespace), std::ceil(m.height)});
}

void TextBlock::paint(PaintContext& pc) {
    Widget::paint(pc);
    if (!wrap) {
        pc.text(text_, bounds, style.font, style.fg, align);
        return;
    }
    gfx::LayoutOptions opt;
    opt.wrap = true;
    opt.max_width = bounds.width();
    opt.max_height = bounds.height();
    opt.align = align;
    auto layout = pc.res.text.make_layout(text_, style.font, opt);
    if (layout) {
        pc.dc->DrawTextLayout(D2D1::Point2F(bounds.l, bounds.t), layout.Get(), pc.brush(style.fg),
                              D2D1_DRAW_TEXT_OPTIONS_CLIP);
    }
}

// ---- Icon ------------------------------------------------------------------------------------------

Size Icon::measure(Size) { return fixed_or({size, size}); }

void Icon::draw(PaintContext& pc, const IconData& d, const Rect& into, Token fg, ComPtr<ID2D1PathGeometry>& cache,
                uint32_t& cache_generation) {
    if (!d.cmds || d.count == 0 || d.viewbox <= 0) {
        return;
    }
    if (!cache || cache_generation != pc.res.dev.generation()) {
        cache.Reset();
        ComPtr<ID2D1PathGeometry> geo;
        if (FAILED(pc.res.dev.d2d_factory()->CreatePathGeometry(&geo))) {
            return;
        }
        ComPtr<ID2D1GeometrySink> sink;
        geo->Open(&sink);
        sink->SetFillMode(D2D1_FILL_MODE_WINDING);
        bool open = false;
        for (size_t i = 0; i < d.count;) {
            const int op = static_cast<int>(d.cmds[i]);
            const float* a = d.cmds + i + 1;
            switch (op) {
                case IconMove:
                    if (open) sink->EndFigure(D2D1_FIGURE_END_OPEN);
                    sink->BeginFigure(D2D1::Point2F(a[0], a[1]), D2D1_FIGURE_BEGIN_FILLED);
                    open = true;
                    i += 3;
                    break;
                case IconLine:
                    sink->AddLine(D2D1::Point2F(a[0], a[1]));
                    i += 3;
                    break;
                case IconCubic:
                    sink->AddBezier(D2D1::BezierSegment(D2D1::Point2F(a[0], a[1]), D2D1::Point2F(a[2], a[3]),
                                                        D2D1::Point2F(a[4], a[5])));
                    i += 7;
                    break;
                default:  // IconClose
                    if (open) sink->EndFigure(D2D1_FIGURE_END_CLOSED);
                    open = false;
                    i += 1;
                    break;
            }
        }
        if (open) sink->EndFigure(D2D1_FIGURE_END_CLOSED);
        sink->Close();
        cache = geo;
        cache_generation = pc.res.dev.generation();
    }
    ID2D1SolidColorBrush* brush = pc.brush(fg);
    if (!brush) {
        return;
    }
    D2D1_MATRIX_3X2_F saved;
    pc.dc->GetTransform(&saved);
    const float s = std::min(into.width(), into.height()) / d.viewbox;
    pc.dc->SetTransform(D2D1::Matrix3x2F::Scale(s, s) * D2D1::Matrix3x2F::Translation(into.l, into.t) * saved);
    pc.dc->FillGeometry(cache.Get(), brush);
    pc.dc->SetTransform(saved);
}

void Icon::paint(PaintContext& pc) {
    Widget::paint(pc);
    if (data) {
        const float s = std::min(bounds.width(), bounds.height());
        const Rect into = Rect::xywh(bounds.l + (bounds.width() - s) / 2, bounds.t + (bounds.height() - s) / 2, s, s);
        draw(pc, *data, into, style.fg, geometry_, generation_);
    }
}

// ---- Button ----------------------------------------------------------------------------------------

Button::Button(std::wstring label) : text(std::move(label)) {
    flags.focusable = true;
    flags.tab_stop = true;
}

void Button::click() {
    if (flags.enabled && on_click) on_click();
}

Size Button::measure(Size) {
    const D2D1_SIZE_F t = res().text.measure(text, style.font);
    float w = text.empty() ? 0.0f : std::ceil(t.width);
    if (icon) w += 16.0f + (text.empty() ? 0.0f : 6.0f);
    return fixed_or({std::max(28.0f, w + 24.0f), 28.0f});
}

void Button::paint(PaintContext& pc) {
    Token bg = primary ? tok::AccentBackground : tok::ButtonSecondaryBackground;
    if (!flags.enabled) {
        bg = tok::ButtonSecondaryBackground;
    } else if (pressed_) {
        bg = tok::ButtonSecondaryPressedBackground;
    } else if (hovered()) {
        bg = primary ? tok::BorderFocus : tok::ButtonSecondaryHoverBackground;
    }
    if (style.bg != kNoToken && !hovered() && !pressed_) bg = style.bg;
    pc.fill(bounds, bg);
    pc.stroke(bounds, focused() ? tok::BorderFocus : (style.border != kNoToken ? style.border : tok::BorderDefault));
    const Token fg = !flags.enabled ? tok::TextMuted : (primary ? tok::AccentForeground : style.fg);
    const D2D1_SIZE_F t = pc.res.text.measure(text, style.font);
    const float tw = text.empty() ? 0.0f : t.width;
    const float iw = icon ? 16.0f : 0.0f;
    const float total = tw + iw + (icon && tw > 0 ? 6.0f : 0.0f);
    float x = bounds.l + std::max(0.0f, (bounds.width() - total) / 2);
    if (icon) {
        const Rect ir = Rect::xywh(x, bounds.t + (bounds.height() - 16.0f) / 2, 16.0f, 16.0f);
        Icon::draw(pc, *icon, ir, fg, icon_geo_, icon_gen_);
        x += iw + 6.0f;
    }
    if (tw > 0) {
        pc.text(text, Rect{x, bounds.t, std::min(bounds.r, x + tw + 2.0f), bounds.b}, style.font, fg);
    }
}

bool Button::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    pressed_ = true;
    root()->capture(this);
    invalidate();
    return true;
}

bool Button::on_mouse_up(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    const bool was = pressed_;
    pressed_ = false;
    invalidate();
    if (was && bounds.contains(e.pt)) click();
    return true;
}

bool Button::on_key_down(const KeyEvent& e) {
    if (e.vk == VK_SPACE || e.vk == VK_RETURN) {
        click();
        return true;
    }
    return false;
}

// ---- CheckBox --------------------------------------------------------------------------------------

CheckBox::CheckBox(std::wstring label) : text(std::move(label)) {
    flags.focusable = true;
    flags.tab_stop = true;
}

void CheckBox::set_checked(bool v) {
    if (v == checked_) return;
    checked_ = v;
    invalidate();
}

void CheckBox::toggle() {
    checked_ = !checked_;
    invalidate();
    if (on_toggle) on_toggle(checked_);
}

Size CheckBox::measure(Size) {
    const D2D1_SIZE_F t = res().text.measure(text, style.font);
    return fixed_or({kBox + (text.empty() ? 0.0f : kGap + std::ceil(t.width)), std::max(kBox, std::ceil(t.height)) + 4.0f});
}

void CheckBox::paint(PaintContext& pc) {
    Widget::paint(pc);
    const Rect box = Rect::xywh(bounds.l, bounds.t + (bounds.height() - kBox) / 2, kBox, kBox);
    pc.fill(box, checked_ ? tok::AccentBackground : (hovered() ? tok::ButtonSecondaryHoverBackground : tok::ButtonSecondaryBackground));
    pc.stroke(box, focused() ? tok::BorderFocus : tok::BorderDefault);
    if (checked_) {
        pc.line({box.l + 3.5f, box.t + 8.5f}, {box.l + 6.5f, box.t + 11.5f}, tok::AccentForeground, 1.6f);
        pc.line({box.l + 6.5f, box.t + 11.5f}, {box.l + 12.5f, box.t + 4.5f}, tok::AccentForeground, 1.6f);
    }
    pc.text(text, Rect{box.r + kGap, bounds.t, bounds.r, bounds.b}, style.font, flags.enabled ? style.fg : tok::TextMuted);
}

bool CheckBox::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    pressed_ = true;
    root()->capture(this);
    return true;
}

bool CheckBox::on_mouse_up(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    const bool was = pressed_;
    pressed_ = false;
    if (was && bounds.contains(e.pt)) toggle();
    return true;
}

bool CheckBox::on_key_down(const KeyEvent& e) {
    if (e.vk == VK_SPACE) {
        toggle();
        return true;
    }
    return false;
}

}  // namespace bb::ui
