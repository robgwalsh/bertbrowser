// Natural sort had no C# test of its own (the comparer was a one-line P/Invoke); the order is pinned
// here because every name sort in the app goes through it.
use std::cmp::Ordering;

use bb_paths::{fold_upper, fold_upper_cmp, natural_cmp};

#[test]
fn numbers_compare_by_value_not_by_digit() {
    assert_eq!(Ordering::Less, natural_cmp("file2", "file10"));
    assert_eq!(Ordering::Greater, natural_cmp("file10", "file2"));
}

#[test]
fn case_does_not_split_the_order() {
    assert_eq!(Ordering::Equal, natural_cmp("Readme", "readme"));
}

#[test]
fn sorting_a_listing_matches_explorer() {
    let mut names = vec!["img12.png", "img2.png", "img1.png", "Notes", "a.txt"];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(
        vec!["a.txt", "img1.png", "img2.png", "img12.png", "Notes"],
        names
    );
}

#[test]
fn a_long_name_after_a_short_one_does_not_keep_the_short_ones_tail() {
    // The comparison reuses its buffers; a stale tail would make these two unequal.
    assert_eq!(Ordering::Less, natural_cmp("a much longer name.txt", "b"));
    assert_eq!(Ordering::Equal, natural_cmp("b", "B"));
}

// The sort's allocation-free fold must order exactly as comparing the two folded strings does.
#[test]
fn fold_upper_cmp_orders_as_the_folded_strings_do() {
    let words = [
        "", "a", "A", "ab", "aB", "b", "TXT file", "txt FILE", "PNG file", "é", "É", "ß", "SS",
        "ﬃ", "straße", "STRASSE", "\u{212A}", "k", "z", "Ω", "ω", "日本",
    ];
    for a in words {
        for b in words {
            assert_eq!(
                fold_upper(a).cmp(&fold_upper(b)),
                fold_upper_cmp(a, b),
                "{a:?} vs {b:?}"
            );
        }
    }
}
