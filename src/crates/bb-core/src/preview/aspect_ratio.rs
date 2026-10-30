//! A width:height ratio, written "4:3": the shape of a thumbnail tile. A parsed value rather than
//! a raw string, because the setting is stored in settings.json where a user can type anything
//! into it. Parsing therefore **never fails loudly**: an unreadable, absurd or missing value
//! resolves to `DEFAULT` and the tiles still draw, the same contract the theme resolver keeps.
//! Spec: Core/Models/AspectRatio.cs.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AspectRatio {
    pub width: i32,
    pub height: i32,
}

impl AspectRatio {
    /// Widest term either side may take. Bounds a hand-edited settings file to shapes that are
    /// merely odd rather than degenerate (a 400:1 tile is a line).
    pub const MAX_TERM: i32 = 100;

    /// 4:3, matching the shape most photos and video thumbnails already are.
    pub const DEFAULT: AspectRatio = AspectRatio::new(4, 3);

    /// What the settings picker offers, in the order it offers them. A value the user typed into
    /// settings.json by hand still works; it is simply not in this list.
    pub const PRESETS: &[AspectRatio] = &[
        AspectRatio::new(1, 1),
        AspectRatio::new(4, 3),
        AspectRatio::new(3, 2),
        AspectRatio::new(16, 10),
        AspectRatio::new(16, 9),
        AspectRatio::new(3, 4),
        AspectRatio::new(2, 3),
    ];

    const fn new(width: i32, height: i32) -> AspectRatio {
        AspectRatio { width, height }
    }

    /// Parses "W:H", tolerating surrounding and inner whitespace. None for anything else,
    /// including zero or negative terms and terms above `MAX_TERM`.
    pub fn try_parse(text: Option<&str>) -> Option<AspectRatio> {
        let text = text?;
        if text.trim().is_empty() {
            return None;
        }

        let (left, right) = text.split_once(':')?;
        if right.contains(':') {
            return None;
        }

        let width: i32 = left.trim().parse().ok()?;
        let height: i32 = right.trim().parse().ok()?;

        if !(1..=Self::MAX_TERM).contains(&width) || !(1..=Self::MAX_TERM).contains(&height) {
            return None;
        }

        Some(AspectRatio::new(width, height))
    }

    /// The parsed ratio, or `DEFAULT` for anything unusable.
    pub fn parse(text: Option<&str>) -> AspectRatio {
        Self::try_parse(text).unwrap_or(Self::DEFAULT)
    }

    /// The height a tile of `width` pixels should be. A zero ratio (reachable, since the fields
    /// are plain) is treated as square rather than dividing by zero and handing the view a NaN.
    pub fn height_for(&self, width: f64) -> f64 {
        if self.width > 0 {
            width * f64::from(self.height) / f64::from(self.width)
        } else {
            width
        }
    }
}

impl Default for AspectRatio {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.width, self.height)
    }
}
