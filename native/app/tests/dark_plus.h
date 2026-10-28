// The Dark+ built-in theme (bb-core/src/theming/catalog.rs), as ARGB in token order. Tests and the widget
// photographs use it so a PNG shows the colours the app has, not the magenta of an unset token.
#pragma once
#include "gfx/tokens.h"

#include <cstdint>
#include <vector>

namespace bb::gfx {

inline std::vector<uint32_t> dark_plus_argb() {
    std::vector<uint32_t> c(tok::Count, 0xFFFF00FFu);
    c[tok::WindowBackground] = 0xFF1E1E1E;
    c[tok::SurfaceBackground] = 0xFF252526;
    c[tok::SurfaceRaised] = 0xFF333333;
    c[tok::OverlayBackground] = 0xFF252526;
    c[tok::OverlayBorder] = 0xFF454545;
    c[tok::ShadowColor] = 0x99000000;
    c[tok::Scrim] = 0x80000000;
    c[tok::TextPrimary] = 0xFFCCCCCC;
    c[tok::TextSecondary] = 0xFF9D9D9D;
    c[tok::TextMuted] = 0xFF7A7A7A;
    c[tok::TextDisabled] = 0xFF6E6E6E;
    c[tok::TextOnAccent] = 0xFFFFFFFF;
    c[tok::TextLink] = 0xFF3794FF;
    c[tok::TextPlaceholder] = 0xFFA6A6A6;
    c[tok::BorderDefault] = 0xFF3C3C3C;
    c[tok::BorderSubtle] = 0xFF2B2B2B;
    c[tok::BorderFocus] = 0xFF007FD4;
    c[tok::BorderActive] = 0xFF007ACC;
    c[tok::AccentBackground] = 0xFF0E639C;
    c[tok::AccentHoverBackground] = 0xFF1177BB;
    c[tok::AccentPressedBackground] = 0xFF0D5A8E;
    c[tok::AccentForeground] = 0xFFFFFFFF;
    c[tok::AccentMuted] = 0xFF04395E;
    c[tok::ListBackground] = 0xFF1E1E1E;
    c[tok::ListHoverBackground] = 0xFF2A2D2E;
    c[tok::ListSelectedBackground] = 0xFF04395E;
    c[tok::ListSelectedForeground] = 0xFFFFFFFF;
    c[tok::ListSelectedHoverBackground] = 0xFF0A4A75;
    c[tok::ListSelectedInactiveBackground] = 0xFF37373D;
    c[tok::ListSelectedInactiveForeground] = 0xFFCCCCCC;
    c[tok::ListDropBackground] = 0xFF062F4A;
    c[tok::ListDropBorder] = 0xFF007ACC;
    c[tok::ListHeaderBackground] = 0xFF252526;
    c[tok::ListHeaderForeground] = 0xFFCCCCCC;
    c[tok::ListHeaderHoverBackground] = 0xFF2A2D2E;
    c[tok::ListHeaderBorder] = 0xFF3C3C3C;
    c[tok::CompareOnlyHere] = 0x262E9E7A;
    c[tok::CompareNewer] = 0x282F7FD8;
    c[tok::CompareOlder] = 0x2AB8860B;
    c[tok::CompareDiffers] = 0x28A06AA8;
    c[tok::CompareUnknown] = 0x24808080;
    c[tok::ChangeCreated] = 0x262E9E7A;
    c[tok::ChangeModified] = 0x282F7FD8;
    c[tok::ChangeDeleted] = 0x2AD9534F;
    c[tok::ChangeRenamed] = 0x2AB8860B;
    c[tok::TreeChevronForeground] = 0xFFC5C5C5;
    c[tok::TreeChevronHoverForeground] = 0xFFE7E7E7;
    c[tok::TabStripBackground] = 0xFF252526;
    c[tok::TabActiveBackground] = 0xFF1E1E1E;
    c[tok::TabActiveForeground] = 0xFFFFFFFF;
    c[tok::TabInactiveBackground] = 0xFF2D2D2D;
    c[tok::TabInactiveForeground] = 0xFF9D9D9D;
    c[tok::TabHoverBackground] = 0xFF333333;
    c[tok::TabBorder] = 0xFF252526;
    c[tok::TabActiveIndicator] = 0xFF007ACC;
    c[tok::TitleBarBackground] = 0xFF3C3C3C;
    c[tok::TitleBarInactiveBackground] = 0xFF3C3C3C;
    c[tok::TitleBarForeground] = 0xFFCCCCCC;
    c[tok::TitleBarInactiveForeground] = 0xFF8A8A8A;
    c[tok::TitleBarButtonHoverBackground] = 0xFF4D4D4D;
    c[tok::TitleBarButtonPressedBackground] = 0xFF5A5A5A;
    c[tok::TitleBarCloseHoverBackground] = 0xFFC42B1C;
    c[tok::TitleBarCloseHoverForeground] = 0xFFFFFFFF;
    c[tok::TitleBarBorder] = 0xFF2B2B2B;
    c[tok::ToolbarBackground] = 0xFF333333;
    c[tok::ToolbarForeground] = 0xFFCCCCCC;
    c[tok::ToolbarHoverBackground] = 0xFF4F5254;
    c[tok::ToolbarPressedBackground] = 0xFF5A5D5E;
    c[tok::ToolbarCheckedForeground] = 0xFF4EC9F5;
    c[tok::InputBackground] = 0xFF3C3C3C;
    c[tok::InputForeground] = 0xFFCCCCCC;
    c[tok::InputBorder] = 0xFF3C3C3C;
    c[tok::InputSelectionBackground] = 0xFF264F78;
    c[tok::InputSelectionForeground] = 0xFFFFFFFF;
    c[tok::InputCaret] = 0xFFAEAFAD;
    c[tok::ButtonSecondaryBackground] = 0xFF3A3D41;
    c[tok::ButtonSecondaryHoverBackground] = 0xFF45494E;
    c[tok::ButtonSecondaryPressedBackground] = 0xFF4F5459;
    c[tok::ButtonSecondaryForeground] = 0xFFCCCCCC;
    c[tok::ButtonDisabledBackground] = 0xFF2D2D2D;
    c[tok::ButtonDisabledForeground] = 0xFF6E6E6E;
    c[tok::MenuBackground] = 0xFF252526;
    c[tok::MenuForeground] = 0xFFCCCCCC;
    c[tok::MenuBorder] = 0xFF454545;
    c[tok::MenuHoverBackground] = 0xFF0F6CBD;
    c[tok::MenuHoverForeground] = 0xFFFFFFFF;
    c[tok::MenuSeparator] = 0xFF454545;
    c[tok::MenuIconForeground] = 0xFFC5C5C5;
    c[tok::MenuGestureForeground] = 0xFF8A8A8A;
    c[tok::MenuDisabledForeground] = 0xFF6E6E6E;
    c[tok::ScrollBarBackground] = 0x00000000;
    c[tok::ScrollBarThumb] = 0x66797979;
    c[tok::ScrollBarThumbHover] = 0xB3646464;
    c[tok::ScrollBarThumbActive] = 0x66BFBFBF;
    c[tok::StatusBarBackground] = 0xFF0F6CBD;
    c[tok::StatusBarForeground] = 0xFFFFFFFF;
    c[tok::StatusBarMutedForeground] = 0xCCFFFFFF;
    c[tok::StatusBarHoverBackground] = 0x1FFFFFFF;
    c[tok::ErrorForeground] = 0xFFF48771;
    c[tok::ErrorBackground] = 0xFF5A1D1D;
    c[tok::ErrorBorder] = 0xFFBE1100;
    c[tok::WarningForeground] = 0xFFCCA700;
    c[tok::WarningBackground] = 0xFF352A05;
    c[tok::WarningBorder] = 0xFFB89500;
    c[tok::ProgressIndicator] = 0xFF0E70C0;
    c[tok::ProgressTrack] = 0xFF2D2D2D;
    c[tok::SliderTrack] = 0xFF4D4D4D;
    c[tok::SliderThumb] = 0xFFCCCCCC;
    c[tok::SliderThumbHover] = 0xFFFFFFFF;
    c[tok::CheckBoxBackground] = 0xFF3C3C3C;
    c[tok::CheckBoxBorder] = 0xFF6E6E6E;
    c[tok::CheckBoxGlyph] = 0xFFFFFFFF;
    c[tok::CheckBoxCheckedBackground] = 0xFF0E639C;
    c[tok::GroupBoxHeaderForeground] = 0xFFCCCCCC;
    c[tok::SplitterBackground] = 0x00000000;
    c[tok::SplitterHoverBackground] = 0xFF007ACC;
    c[tok::MarqueeFill] = 0x33007ACC;
    c[tok::MarqueeBorder] = 0xFF007ACC;
    c[tok::ThumbnailTileBackground] = 0xFF2A2A2A;
    c[tok::PreviewCheckerLight] = 0xFF3A3A3A;
    c[tok::PreviewCheckerDark] = 0xFF2A2A2A;
    c[tok::SyntaxKeyword] = 0xFF6FB0E4;
    c[tok::SyntaxString] = 0xFFCE9178;
    c[tok::SyntaxComment] = 0xFF6A9955;
    c[tok::SyntaxNumber] = 0xFFB5CEA8;
    c[tok::SyntaxPunctuation] = 0xFFD4D4D4;
    return c;
}

}  // namespace bb::gfx
