//! Parses the size literals a `size:` filter accepts: "100mb", "1.5g", "512", "0". Multiples are
//! **1024-based**, because that is what the Size column shows, and a filter disagreeing with the
//! column beside it reads as a bug in the filter. Spec: Core/Services/Search/SizeText.cs.

pub struct SizeText;

/// Unit suffixes, longest first so "KB" is tried before "K".
const UNITS: [(&str, i64); 11] = [
    ("PB", 1 << 50),
    ("TB", 1 << 40),
    ("GB", 1 << 30),
    ("MB", 1 << 20),
    ("KB", 1 << 10),
    ("P", 1 << 50),
    ("T", 1 << 40),
    ("G", 1 << 30),
    ("M", 1 << 20),
    ("K", 1 << 10),
    ("B", 1),
];

impl SizeText {
    /// Parses "100mb" into a byte count. None for anything unusable: a bare unit, a negative number,
    /// a value that overflows, or trailing rubbish.
    pub fn try_parse(text: &str) -> Option<i64> {
        let mut trimmed = text.trim().to_ascii_uppercase();
        let mut multiple = 1i64;
        for (suffix, value) in UNITS {
            if let Some(rest) = trimmed.strip_suffix(suffix) {
                multiple = value;
                trimmed = rest.trim_end().to_string();
                break;
            }
        }
        // A bare unit ("mb") names no quantity.
        if trimmed.is_empty() {
            return None;
        }
        // Digits and one decimal point only: no sign, no exponent, no "inf".
        if !trimmed.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            || trimmed.matches('.').count() > 1
        {
            return None;
        }
        let number: f64 = trimmed.parse().ok()?;
        if number < 0.0 || !number.is_finite() {
            return None;
        }
        let scaled = number * multiple as f64;
        if scaled > i64::MAX as f64 {
            return None;
        }
        Some(scaled as i64)
    }
}
