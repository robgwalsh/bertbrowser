#include "gfx/palette.h"

namespace bb::gfx {

std::vector<uint32_t> default_dark_argb() {
    std::vector<uint32_t> c(tok::Count, 0xFFFF00FFu);
    c[tok::WindowBg] = 0xFF1E1E1E;
    c[tok::Text] = 0xFFE6E6E6;
    c[tok::TextMuted] = 0xFF9A9A9A;
    c[tok::Border] = 0xFF4A4A4A;
    c[tok::ControlBg] = 0xFF2D2D2D;
    c[tok::ControlHover] = 0xFF3A3A3A;
    c[tok::ControlPressed] = 0xFF4A4A4A;
    c[tok::Accent] = 0xFF3A7BD5;
    c[tok::AccentText] = 0xFFFFFFFF;
    c[tok::SelectionBg] = 0xFF2F5C9E;
    c[tok::SelectionText] = 0xFFFFFFFF;
    c[tok::SelectionInactiveBg] = 0xFF3A3A3A;
    c[tok::Caret] = 0xFFFFFFFF;
    c[tok::Composition] = 0xFFF2BF4D;
    c[tok::Placeholder] = 0xFF8C8C8C;
    c[tok::HeaderBg] = 0xFF2A2A2A;
    c[tok::HeaderText] = 0xFFCFCFCF;
    c[tok::RowAlt] = 0xFF232323;
    c[tok::FocusRing] = 0xFF6AA0F0;
    c[tok::ScrollTrack] = 0xFF262626;
    c[tok::ScrollThumb] = 0xFF5A5A5A;
    c[tok::ScrollThumbHot] = 0xFF7A7A7A;
    c[tok::Marquee] = 0x553A7BD5;
    c[tok::SplitterBar] = 0xFF333333;
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
