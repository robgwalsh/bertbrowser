//! WCAG 2.1 contrast, used two ways: the test suite asserts every theme we ship is readable, and
//! the theme editor badges a token the user has just made illegible.
//! Spec: src/BertBrowser.Core/Theming/ThemeContrast.cs.

use super::{ResolvedTheme, ThemeColor, ThemeToken};

pub struct ThemeContrast;

impl ThemeContrast {
    /// WCAG AA for body text.
    pub const AA_NORMAL_TEXT: f64 = 4.5;

    /// WCAG AA for large or incidental text.
    pub const AA_LARGE_TEXT: f64 = 3.0;

    /// Contrast ratio between 1 and 21. A translucent `foreground` is composited over the
    /// background first; otherwise a token like the scrollbar thumb scores against an alpha it is
    /// never actually seen at.
    pub fn ratio(foreground: ThemeColor, background: ThemeColor) -> f64 {
        let fg = foreground.composite_over(background).relative_luminance();
        let bg = background.relative_luminance();
        let (lighter, darker) = if fg >= bg { (fg, bg) } else { (bg, fg) };
        (lighter + 0.05) / (darker + 0.05)
    }

    pub fn ratio_of(theme: &ResolvedTheme, foreground: ThemeToken, background: ThemeToken) -> f64 {
        ThemeContrast::ratio(theme[foreground], theme[background])
    }
}
