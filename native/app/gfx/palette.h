// The theme as brushes indexed by a token id. Widgets hold token ids and never a brush: a theme change
// is `set_colors`, a device loss is `rebuild`, and nothing that draws has to hear about either.
//
// The size and order of the array come from the Rust theme later (126 tokens); the constants in `tok`
// are the ones the foundation widgets need and give the first slots. The FFI side supplies the real
// order and these become aliases into it.
#pragma once
#include "gfx/device.h"

#include <vector>

namespace bb::gfx {

using Token = uint16_t;
constexpr Token kNoToken = 0xFFFF;

namespace tok {
enum : Token {
    WindowBg,
    Text,
    TextMuted,
    Border,
    ControlBg,
    ControlHover,
    ControlPressed,
    Accent,
    AccentText,
    SelectionBg,
    SelectionText,
    SelectionInactiveBg,
    Caret,
    Composition,
    Placeholder,
    HeaderBg,
    HeaderText,
    RowAlt,
    FocusRing,
    ScrollTrack,
    ScrollThumb,
    ScrollThumbHot,
    Marquee,
    SplitterBar,
    Count
};
}  // namespace tok

// A dark default so a bare tree is legible before the theme arrives. ARGB, 0xAARRGGBB.
std::vector<uint32_t> default_dark_argb();

class Palette {
public:
    explicit Palette(GfxDevice& dev);

    // Replace the colours (a theme change). Brushes are recreated lazily on next use.
    void set_colors(const uint32_t* argb, size_t count);
    void set_colors(const std::vector<uint32_t>& argb) { set_colors(argb.data(), argb.size()); }
    // Drop every brush and make them again (a device loss, or an explicit rebuild).
    void rebuild();

    size_t size() const { return colors_.size(); }
    uint32_t argb(Token t) const { return t < colors_.size() ? colors_[t] : 0u; }
    D2D1_COLOR_F color(Token t) const;
    // Null for kNoToken or an out-of-range token.
    ID2D1SolidColorBrush* brush(Token t);
    // How many brushes have been created since construction: the tests watch this for rebuilds.
    uint32_t brushes_made() const { return made_; }

private:
    void check_generation();

    GfxDevice& dev_;
    uint32_t generation_ = 0;
    uint32_t made_ = 0;
    ComPtr<ID2D1DeviceContext> ctx_;
    std::vector<uint32_t> colors_;
    std::vector<ComPtr<ID2D1SolidColorBrush>> brushes_;
};

}  // namespace bb::gfx
