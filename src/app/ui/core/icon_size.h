// How large the toolbar buttons and their icons are: one app-wide choice (Settings > General > Icon size),
// read by the title bar's toolbar and by each folder's header row. Compact is the title bar as it was.
#pragma once

#include <string_view>

namespace bb::ui {

enum class IconSize { Small, Compact, Medium, Large };

struct IconMetrics {
    float icon;    // the picture's side, in DIPs
    float button;  // a button's width; its height is the bar's less the 2 DIPs of air above and below
    float bar;     // a toolbar's height
    float height() const { return bar - 4.0f; }
};

inline IconMetrics icon_metrics(IconSize size) {
    switch (size) {
        case IconSize::Small: return {14.0f, 26.0f, 28.0f};
        case IconSize::Medium: return {20.0f, 38.0f, 38.0f};
        case IconSize::Large: return {24.0f, 44.0f, 44.0f};
        case IconSize::Compact: break;
    }
    return {16.0f, 32.0f, 32.0f};
}

inline IconSize g_icon_size = IconSize::Compact;
inline IconMetrics icon_metrics() { return icon_metrics(g_icon_size); }

// The settings file spells it as the enum does ("Compact"); anything unknown is the default.
inline IconSize parse_icon_size(const char* name) {
    const std::string_view n = name ? name : "";
    if (n == "Small") return IconSize::Small;
    if (n == "Medium") return IconSize::Medium;
    if (n == "Large") return IconSize::Large;
    return IconSize::Compact;
}

}  // namespace bb::ui
