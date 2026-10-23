//! A straight ARGB colour, deliberately not a UI type: themes are resolved, validated and
//! contrast-checked here, with no UI dependency, and the UI turns these into brushes at the last
//! moment. Spec: src/BertBrowser.Core/Theming/ThemeColor.cs.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct ThemeColor {
    pub a: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ThemeColor {
    pub const fn new(a: u8, r: u8, g: u8, b: u8) -> ThemeColor {
        ThemeColor { a, r, g, b }
    }

    pub const fn from_rgb(r: u8, g: u8, b: u8) -> ThemeColor {
        ThemeColor::new(0xFF, r, g, b)
    }

    /// Parses `#RGB`, `#RGBA`, `#RRGGBB` or `#AARRGGBB`, with or without the leading `#` and in
    /// either case. Never panics: a theme file is user-editable text.
    pub fn try_parse(text: &str) -> Option<ThemeColor> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        let span = trimmed.strip_prefix('#').unwrap_or(trimmed).as_bytes();

        // Short forms repeat each nibble, so #ABC is #AABBCC: the CSS rule, and what anyone
        // hand-editing a theme file will expect.
        match span.len() {
            3 => nibbles(span, 0xFF),
            4 => nibbles(&span[1..], hex(span[0])? * 17),
            6 => bytes(span, 0xFF),
            8 => bytes(&span[2..], byte(span)?),
            _ => None,
        }
    }

    /// Parse or panic. For built-in theme data, where a bad literal is a bug, not input.
    pub fn parse(text: &str) -> ThemeColor {
        ThemeColor::try_parse(text).unwrap_or_else(|| panic!("'{text}' is not a colour."))
    }

    /// Round-trips through `try_parse`. Opaque colours omit the alpha pair.
    pub fn to_hex(self) -> String {
        if self.a == 0xFF {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.a, self.r, self.g, self.b)
        }
    }

    /// This colour composited over an opaque `background`. Contrast checks need it: a token like
    /// the scrollbar thumb is only ever seen blended with what sits behind it.
    pub fn composite_over(self, background: ThemeColor) -> ThemeColor {
        if self.a == 0xFF {
            return self;
        }
        let alpha = f64::from(self.a) / 255.0;
        // .NET's Math.Round rounds half to even, and the catalogue's contrast suite is pinned to
        // those exact bytes.
        let mix = |fg: u8, bg: u8| {
            (f64::from(fg) * alpha + f64::from(bg) * (1.0 - alpha)).round_ties_even() as u8
        };
        ThemeColor::new(
            0xFF,
            mix(self.r, background.r),
            mix(self.g, background.g),
            mix(self.b, background.b),
        )
    }

    /// WCAG 2.1 relative luminance (0 = black, 1 = white), ignoring alpha.
    pub fn relative_luminance(self) -> f64 {
        fn channel(v: u8) -> f64 {
            let c = f64::from(v) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// Hue in degrees [0, 360), saturation and value in [0, 1]. Alpha is carried separately.
    pub fn to_hsv(self) -> (f64, f64, f64) {
        let r = f64::from(self.r) / 255.0;
        let g = f64::from(self.g) / 255.0;
        let b = f64::from(self.b) / 255.0;
        let max = r.max(g.max(b));
        let min = r.min(g.min(b));
        let delta = max - min;

        let h = if delta == 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / delta + 6.0) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / delta + 2.0)
        } else {
            60.0 * ((r - g) / delta + 4.0)
        };

        (h, if max == 0.0 { 0.0 } else { delta / max }, max)
    }

    pub fn from_hsv(h: f64, s: f64, v: f64) -> ThemeColor {
        ThemeColor::from_hsva(h, s, v, 0xFF)
    }

    pub fn from_hsva(h: f64, s: f64, v: f64, a: u8) -> ThemeColor {
        let h = ((h % 360.0) + 360.0) % 360.0;
        let s = s.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);

        let c = v * s;
        let x = c * (1.0 - (h / 60.0 % 2.0 - 1.0).abs());
        let m = v - c;

        let (r, g, b) = match (h / 60.0) as i32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };

        let scale = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8;
        ThemeColor::new(a, scale(r + m), scale(g + m), scale(b + m))
    }
}

impl fmt::Display for ThemeColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

fn nibbles(span: &[u8], alpha: u8) -> Option<ThemeColor> {
    Some(ThemeColor::new(
        alpha,
        hex(span[0])? * 17,
        hex(span[1])? * 17,
        hex(span[2])? * 17,
    ))
}

fn bytes(span: &[u8], alpha: u8) -> Option<ThemeColor> {
    Some(ThemeColor::new(
        alpha,
        byte(span)?,
        byte(&span[2..])?,
        byte(&span[4..])?,
    ))
}

fn byte(span: &[u8]) -> Option<u8> {
    Some(hex(span[0])? * 16 + hex(span[1])?)
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}
