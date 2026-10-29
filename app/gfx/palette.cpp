#include "gfx/palette.h"

namespace bb::gfx {

std::vector<uint32_t> default_dark_argb() {
    // Only until the theme arrives (and in the widget tests): every token magenta, then the ones a bare
    // tree needs set to something legible.
    std::vector<uint32_t> c(tok::Count, 0xFFFF00FFu);
    auto set = [&](Token t, uint32_t v) { c[t] = v; };
    set(tok::WindowBackground, 0xFF1E1E1E);
    set(tok::SurfaceBackground, 0xFF1E1E1E);
    set(tok::SurfaceRaised, 0xFF232323);
    set(tok::TextPrimary, 0xFFE6E6E6);
    set(tok::TextMuted, 0xFF9A9A9A);
    set(tok::TextPlaceholder, 0xFF8C8C8C);
    set(tok::BorderDefault, 0xFF4A4A4A);
    set(tok::BorderFocus, 0xFF6AA0F0);
    set(tok::BorderActive, 0xFFF2BF4D);
    set(tok::AccentBackground, 0xFF3A7BD5);
    set(tok::AccentForeground, 0xFFFFFFFF);
    set(tok::ListBackground, 0xFF1E1E1E);
    set(tok::ListSelectedBackground, 0xFF2F5C9E);
    set(tok::ListSelectedForeground, 0xFFFFFFFF);
    set(tok::ListSelectedInactiveBackground, 0xFF3A3A3A);
    set(tok::ListHeaderBackground, 0xFF2A2A2A);
    set(tok::ListHeaderForeground, 0xFFCFCFCF);
    set(tok::ListHeaderHoverBackground, 0xFF3A3A3A);
    set(tok::ListHeaderBorder, 0xFF4A4A4A);
    set(tok::TreeChevronForeground, 0xFF9A9A9A);
    set(tok::InputBackground, 0xFF2D2D2D);
    set(tok::InputForeground, 0xFFE6E6E6);
    set(tok::InputBorder, 0xFF4A4A4A);
    set(tok::InputSelectionBackground, 0xFF2F5C9E);
    set(tok::InputSelectionForeground, 0xFFFFFFFF);
    set(tok::InputCaret, 0xFFFFFFFF);
    set(tok::ButtonSecondaryBackground, 0xFF2D2D2D);
    set(tok::ButtonSecondaryHoverBackground, 0xFF3A3A3A);
    set(tok::ButtonSecondaryPressedBackground, 0xFF4A4A4A);
    set(tok::ScrollBarBackground, 0xFF262626);
    set(tok::ScrollBarThumb, 0xFF5A5A5A);
    set(tok::ScrollBarThumbHover, 0xFF7A7A7A);
    set(tok::MarqueeFill, 0x553A7BD5);
    set(tok::SplitterBackground, 0xFF333333);
    return c;
}
Palette::Palette(GfxDevice& dev) : dev_(dev), colors_(default_dark_argb()) {
    brushes_.resize(colors_.size());
}

void Palette::set_colors(const uint32_t* argb, size_t count) {
    colors_.assign(argb, argb + count);
    brushes_.assign(count, nullptr);
}

void Palette::rebuild() {
    brushes_.assign(colors_.size(), nullptr);
    ctx_.Reset();
    generation_ = 0;
}

void Palette::check_generation() {
    if (generation_ != dev_.generation() || !ctx_) {
        brushes_.assign(colors_.size(), nullptr);
        ctx_ = dev_.create_context();
        generation_ = dev_.generation();
    }
}

D2D1_COLOR_F Palette::color(Token t) const {
    const uint32_t v = argb(t);
    return D2D1::ColorF(static_cast<float>((v >> 16) & 0xFF) / 255.0f, static_cast<float>((v >> 8) & 0xFF) / 255.0f,
                        static_cast<float>(v & 0xFF) / 255.0f, static_cast<float>(v >> 24) / 255.0f);
}

ID2D1SolidColorBrush* Palette::brush(Token t) {
    if (t >= colors_.size()) {
        return nullptr;
    }
    check_generation();
    if (!brushes_[t]) {
        ctx_->CreateSolidColorBrush(color(t), &brushes_[t]);
        ++made_;
    }
    return brushes_[t].Get();
}

}  // namespace bb::gfx
