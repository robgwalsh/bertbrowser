//! One shell property as a column shows it, and how two of them order.
//! Spec: Core/Services/Columns/ColumnValue.cs.

use std::cmp::Ordering;

use crate::FileTime;

/// The string to render, plus a typed key to order by when the property system gave one.
#[derive(Clone, PartialEq, Debug)]
pub struct ColumnValue {
    /// Always populated — the display-formatted text the cell renders and a person recognises
    /// ("6240 x 4160", "00:03:41", "192kbps").
    pub display: String,
    /// Set for the numeric variant types. At most one of this and `date_utc` is set; both none
    /// means the display string is all there is.
    pub number: Option<f64>,
    /// Set for `VT_FILETIME`. This is the one that has to exist: a formatted date sorts by
    /// day-of-month under any string comparison, and Date taken is the first column anyone adds.
    pub date_utc: Option<FileTime>,
}

impl ColumnValue {
    pub fn words(display: &str) -> ColumnValue {
        ColumnValue {
            display: display.to_string(),
            number: None,
            date_utc: None,
        }
    }

    pub fn number(display: &str, number: f64) -> ColumnValue {
        ColumnValue {
            display: display.to_string(),
            number: Some(number),
            date_utc: None,
        }
    }

    pub fn date(display: &str, date_utc: FileTime) -> ColumnValue {
        ColumnValue {
            display: display.to_string(),
            number: None,
            date_utc: Some(date_utc),
        }
    }
}

/// How two values in the same column order against each other. The text comparer is a parameter
/// rather than something this reaches for, because natural ordering is `StrCmpLogicalW`
/// (`bb_paths::natural_cmp`) and the rule is testable with a plain ordinal one.
pub struct ColumnComparison;

impl ColumnComparison {
    /// Orders two cell values, with anything unknown last.
    ///
    /// **A missing value sorts last in both directions**, which is why it is answered here rather
    /// than left to the caller's descending flip. A metadata column fills in as rows are read, and
    /// a blank is not a small value — it is the absence of one, and floating a screenful of blanks
    /// to the top the moment someone reverses the sort would be indistinguishable from a bug.
    ///
    /// Typed keys beat text, and a column where the two disagree — one file's handler returning a
    /// number where the rest return words — degrades to comparing the display strings rather than
    /// ordering half the list by one rule and half by another.
    pub fn compare(
        a: Option<&ColumnValue>,
        b: Option<&ColumnValue>,
        text: impl Fn(&str, &str) -> Ordering,
    ) -> Ordering {
        let (a, b) = match (a, b) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Greater,
            (Some(_), None) => return Ordering::Less,
            (Some(a), Some(b)) => (a, b),
        };

        if let (Some(left), Some(right)) = (a.date_utc, b.date_utc) {
            return left.cmp(&right);
        }
        if let (Some(x), Some(y)) = (a.number, b.number) {
            return compare_f64(x, y);
        }

        text(&a.display, &b.display)
    }

    /// Where a value sorts relative to a missing one: 0 for something, 1 for nothing. The file
    /// list applies this as a band *above* the ascending/descending flip.
    pub fn known_band(value: Option<&ColumnValue>) -> u8 {
        u8::from(value.is_none())
    }
}

/// A total order over doubles with `NaN` below everything and equal to itself — what a sort key
/// needs from a value a handler should never have produced.
fn compare_f64(x: f64, y: f64) -> Ordering {
    match x.partial_cmp(&y) {
        Some(order) => order,
        None => match (x.is_nan(), y.is_nan()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Less,
            _ => Ordering::Greater,
        },
    }
}
