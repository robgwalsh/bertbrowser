// Port of tests/BertBrowser.Core.Tests/ColumnComparisonTests.cs.
use std::cmp::Ordering;

use bb_core::FileTime;
use bb_core::columns::{ColumnComparison, ColumnValue};

/// Stands in for the natural comparer, which is a shlwapi call. Ordinal is enough to show *which
/// arm* was taken, which is the whole of what this rule decides.
fn text(a: &str, b: &str) -> Ordering {
    a.cmp(b)
}

fn words(display: &str) -> ColumnValue {
    ColumnValue::words(display)
}

fn number(display: &str, value: f64) -> ColumnValue {
    ColumnValue::number(display, value)
}

fn date(display: &str, value: FileTime) -> ColumnValue {
    ColumnValue::date(display, value)
}

#[test]
fn a_date_is_compared_as_a_date_and_not_as_its_text() {
    // The case the typed key exists for. As text, "31/08/2026" precedes "01/09/2026" under
    // every string comparison there is, so a Date taken column would sort by day of month.
    let august = date(
        "31/08/2026 14:03",
        FileTime::from_ymd_hms(2026, 8, 31, 14, 3, 0),
    );
    let september = date(
        "01/09/2026 09:00",
        FileTime::from_ymd_hms(2026, 9, 1, 9, 0, 0),
    );

    assert_eq!(
        Ordering::Less,
        ColumnComparison::compare(Some(&august), Some(&september), text)
    );
    assert_eq!(Ordering::Greater, text(&august.display, &september.display));
}

#[test]
fn a_number_is_compared_as_a_number_and_not_as_its_text() {
    // Grouping separators are what break the string comparison here — "1,024 kbps" against
    // "128 kbps" compares '1' with '1' then ',' with '2'.
    let faster = number("1,024 kbps", 1024.0);
    let slower = number("128 kbps", 128.0);

    assert_eq!(
        Ordering::Less,
        ColumnComparison::compare(Some(&slower), Some(&faster), text)
    );
    assert_eq!(Ordering::Greater, text(&slower.display, &faster.display));
}

#[test]
fn anything_else_falls_back_to_the_display_text() {
    assert_eq!(
        Ordering::Less,
        ColumnComparison::compare(Some(&words("Canon")), Some(&words("Nikon")), text)
    );
}

#[test]
fn a_column_whose_handlers_disagree_about_type_compares_as_text() {
    // One file's handler returning a number where the rest return words. Ordering half the list
    // by one rule and half by another would be worse than ordering all of it by text.
    let typed = number("12", 12.0);
    let untyped = words("9");

    assert_eq!(
        text("12", "9"),
        ColumnComparison::compare(Some(&typed), Some(&untyped), text)
    );
}

// --- Unknown sinks last ---

#[test]
fn a_missing_value_sorts_after_one_that_is_there() {
    assert_eq!(
        Ordering::Greater,
        ColumnComparison::compare(None, Some(&words("anything")), text)
    );
    assert_eq!(
        Ordering::Less,
        ColumnComparison::compare(Some(&words("anything")), None, text)
    );
}

#[test]
fn two_missing_values_are_equal() {
    assert_eq!(Ordering::Equal, ColumnComparison::compare(None, None, text));
}

/// The half that makes "unknown sinks last" true in *both* directions. A blank is the absence of
/// a value, not a small one — floating a screenful of blanks to the top the moment someone
/// reverses the sort is indistinguishable from a bug, so the band is applied above the direction
/// flip rather than being left to `ColumnComparison::compare`.
#[test]
fn the_known_band_is_what_keeps_blanks_down_under_a_descending_sort() {
    assert_eq!(0, ColumnComparison::known_band(Some(&words("something"))));
    assert_eq!(1, ColumnComparison::known_band(None));
}
