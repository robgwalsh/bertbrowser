// Natural sort had no C# test of its own (the comparer was a one-line P/Invoke); the order is pinned
// here because every name sort in the app goes through it.
use std::cmp::Ordering;

use bb_paths::natural_cmp;

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
