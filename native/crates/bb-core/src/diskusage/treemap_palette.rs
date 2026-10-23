//! The treemap's fill colours, derived from whatever theme is loaded rather than authored.
//!
//! Deriving rather than adding tokens is a deliberate trade. A ramp of eight fills would be eight
//! new `ThemeToken`s that every root theme must define and every built-in must pass the contrast
//! suite with: a great deal of hand-picked colour for something a rule can produce, and a tax on
//! every future theme. Rotating hue from the theme's own accent gives a palette that follows all
//! the built-ins *and* any theme a user writes, for free.
//!
//! The rotation is the golden angle, which is what keeps successive hues far apart no matter how
//! many are asked for; stepping by a fixed fraction of the circle instead makes the first and last
//! entries collide as soon as the count divides evenly into 360.
//! Spec: src/BertBrowser.Core/Services/DiskUsage/TreemapPalette.cs.

use crate::theming::{ResolvedTheme, ThemeColor, ThemeContrast, ThemeToken};

/// About 137.507 degrees, the angle that spreads any number of samples most evenly.
const GOLDEN_ANGLE: f64 = 137.50776405003785;

pub struct TreemapPalette;

impl TreemapPalette {
    /// `count` fills for `theme`, each legible under the same text colour an accent-coloured
    /// surface uses.
    pub fn ramp(theme: &ResolvedTheme, count: usize) -> Vec<ThemeColor> {
        let accent = theme[ThemeToken::AccentBackground];
        let label = theme[ThemeToken::TextOnAccent];
        let (hue, mut saturation, value) = accent.to_hsv();

        // A near-grey accent has no meaningful hue to rotate, so the ramp would come out as one
        // colour repeated. Give it enough saturation to be distinguishable.
        if saturation < 0.15 {
            saturation = 0.45;
        }

        (0..count)
            .map(|i| {
                let h = (hue + GOLDEN_ANGLE * i as f64) % 360.0;
                legible(ThemeColor::from_hsv(h, saturation, value), label, value)
            })
            .collect()
    }
}

/// Darkens or lightens a fill until the label on it clears AA.
///
/// Hue rotation alone is not enough: at a fixed saturation and value, yellows and greens are far
/// lighter than blues and purples, so a ramp built from a light accent produces entries that white
/// text vanishes on. Which direction to walk is decided by the label rather than assumed: the
/// light-accent themes (Ayu Mirage, Cobalt2) deliberately use a *dark* `TextOnAccent`, and
/// lightening is what helps there.
fn legible(fill: ThemeColor, label: ThemeColor, start_value: f64) -> ThemeColor {
    if ThemeContrast::ratio(label, fill) >= ThemeContrast::AA_NORMAL_TEXT {
        return fill;
    }

    // Move away from the label: a light label wants a darker fill, and the reverse.
    let towards_dark = label.relative_luminance() > 0.5;
    let (h, start_saturation, _) = fill.to_hsv();

    const STEPS: i32 = 24;
    let mut last = fill;

    for step in 1..=STEPS {
        let t = f64::from(step) / f64::from(STEPS);

        // Going lighter, saturation comes down with value. Raising value alone tops out at a vivid
        // colour that is still too dark for a dark label on several hues, and pushing further just
        // saturates to white, which is what made every light-accent theme's ramp collapse to a row
        // of identical white tiles. A pastel gets light while keeping the hue that tells one tile
        // from its neighbour.
        let v = if towards_dark {
            start_value * (1.0 - t * 0.9)
        } else {
            start_value + (1.0 - start_value) * t
        };
        let s = if towards_dark {
            start_saturation
        } else {
            start_saturation * (1.0 - t * 0.7)
        };

        last = ThemeColor::from_hsv(h, s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
        if ThemeContrast::ratio(label, last) >= ThemeContrast::AA_NORMAL_TEXT {
            return last;
        }
    }

    // Nothing on this hue cleared it. Return the far end of the walk rather than plain black or
    // white: it is the most legible this hue gets, and it still differs from its neighbours, which
    // pure white would not.
    last
}
