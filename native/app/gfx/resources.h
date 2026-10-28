// Everything a widget tree draws with, bundled per device: the palette, the font and layout caches.
#pragma once
#include "gfx/device.h"
#include "gfx/palette.h"
#include "gfx/text_cache.h"

namespace bb::gfx {

struct Resources {
    explicit Resources(GfxDevice& d, size_t layout_capacity = 4096)
        : dev(d), palette(d), text(d.dwrite()), layouts(text, layout_capacity) {}

    // After GfxDevice::recreate(): the DirectWrite factory may be new, device resources certainly are.
    void on_device_recreated() {
        layouts.clear();
        palette.rebuild();
    }

    GfxDevice& dev;
    Palette palette;
    TextCache text;
    LayoutLru layouts;
};

}  // namespace bb::gfx
