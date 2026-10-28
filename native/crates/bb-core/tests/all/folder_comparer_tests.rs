// Port of tests/BertBrowser.Core.Tests/FolderComparerTests.cs
//
// Pairing two trees and folding each folder's verdict up from what is beneath it. The fold is
// what lets a whole subtree be synced without opening it, which makes it the piece a wrong answer
// is hardest to notice in: a folder that says "same" is a folder nobody looks inside.
use bb_core::FileTime;
use bb_core::compare::{
    CompareEntry, CompareResult, CompareSide, CompareTolerance, CompareVerdict, FolderComparer,
};
use bb_paths::{file_name, fold_upper};

fn noon() -> FileTime {
    FileTime::from_ymd_hms(2026, 3, 14, 12, 0, 0)
}

fn file(display: &str) -> CompareEntry {
    file_sized(display, 10)
}

fn file_sized(display: &str, size: i64) -> CompareEntry {
    CompareEntry::new(fold_upper(display), file_name(display), false, size, noon())
}

fn file_modified(display: &str, modified: FileTime) -> CompareEntry {
    CompareEntry::new(fold_upper(display), file_name(display), false, 10, modified)
}

fn folder(display: &str) -> CompareEntry {
    CompareEntry::new(fold_upper(display), file_name(display), true, 0, noon())
}

fn compare(left: &[CompareEntry], right: &[CompareEntry]) -> CompareResult {
    FolderComparer::compare(left, right, CompareTolerance::STRICT)
}

// --- Pairing ---

#[test]
fn two_identical_trees_hold_no_differences() {
    let tree = [folder("src"), file(r"src\main.cs"), file("readme.md")];

    let result = compare(&tree, &tree);

    assert!(!result.any_difference());
    assert_eq!(3, result.same_count);
    assert_eq!(0, result.unknown_count);
}

#[test]
fn two_empty_trees_hold_no_differences() {
    assert!(!compare(&[], &[]).any_difference());
}

#[test]
fn each_side_keeps_what_only_it_has() {
    let result = compare(&[file("a.txt")], &[file("b.txt")]);

    assert_eq!(CompareVerdict::LeftOnly, result.verdict_for("A.TXT"));
    assert_eq!(CompareVerdict::RightOnly, result.verdict_for("B.TXT"));
}

/// Windows does not have two files whose names differ only in case, and neither does the key:
/// the pair must meet, or a copy would land beside its own original.
#[test]
fn names_differing_only_in_case_are_paired_together() {
    let result = compare(&[file("Notes.txt")], &[file("notes.TXT")]);

    assert_eq!(CompareVerdict::Same, result.verdict_for("NOTES.TXT"));
    assert_eq!(1, result.same_count);
}

#[test]
fn a_path_the_comparison_never_saw_is_unknown() {
    assert_eq!(
        CompareVerdict::Unknown,
        compare(&[], &[]).verdict_for(r"SRC\LATER.CS")
    );
}

// --- The fold ---

#[test]
fn a_folder_whose_contents_all_match_is_same() {
    let tree = [folder("src"), file(r"src\a.cs"), file(r"src\b.cs")];

    assert_eq!(
        CompareVerdict::Same,
        compare(&tree, &tree).verdict_for("SRC")
    );
}

#[test]
fn one_changed_file_makes_every_folder_above_it_differ() {
    let left = [
        folder("src"),
        folder(r"src\deep"),
        file_sized(r"src\deep\a.cs", 10),
    ];
    let right = [
        folder("src"),
        folder(r"src\deep"),
        file_sized(r"src\deep\a.cs", 99),
    ];

    let result = compare(&left, &right);

    assert_eq!(CompareVerdict::Differs, result.verdict_for("SRC"));
    assert_eq!(CompareVerdict::Differs, result.verdict_for(r"SRC\DEEP"));
}

/// The load-bearing one. A folder that says "same" is a folder nobody opens, and "same" is what
/// authorises deleting the other side of it, so one descendant nothing is known about has to stop
/// the whole subtree being called a match.
#[test]
fn one_unknown_descendant_makes_the_whole_subtree_unknown() {
    let left = [
        folder("src"),
        folder(r"src\deep"),
        file(r"src\a.cs"),
        file_modified(r"src\deep\b.cs", FileTime::UNKNOWN),
    ];

    let result = compare(&left, &left);

    assert_eq!(
        CompareVerdict::Unknown,
        result.verdict_for(r"SRC\DEEP\B.CS")
    );
    assert_eq!(CompareVerdict::Unknown, result.verdict_for(r"SRC\DEEP"));
    assert_eq!(CompareVerdict::Unknown, result.verdict_for("SRC"));
    assert_eq!(CompareVerdict::Same, result.verdict_for(r"SRC\A.CS"));
}

/// A folder missing from the right is missing wholesale; calling it merely "different" would have
/// the sync copy its contents into a folder that is not there.
#[test]
fn a_folder_missing_on_one_side_stays_missing_not_merely_different() {
    let left = [folder("bin"), file(r"bin\app.exe")];

    let result = compare(&left, &[]);

    assert_eq!(CompareVerdict::LeftOnly, result.verdict_for("BIN"));
    assert_eq!(CompareVerdict::LeftOnly, result.verdict_for(r"BIN\APP.EXE"));
}

/// The inverse, and the mistake a naive fold makes: one new file inside a folder that exists on
/// both sides must not rename the folder "left only".
#[test]
fn a_new_file_inside_a_shared_folder_leaves_the_folder_shared() {
    let left = [folder("src"), file(r"src\a.cs"), file(r"src\new.cs")];
    let right = [folder("src"), file(r"src\a.cs")];

    assert_eq!(
        CompareVerdict::Differs,
        compare(&left, &right).verdict_for("SRC")
    );
}

#[test]
fn a_file_on_one_side_and_a_folder_on_the_other_differs() {
    let result = compare(
        &[file("thing")],
        &[folder("thing"), file(r"thing\inner.txt")],
    );

    assert_eq!(CompareVerdict::Differs, result.verdict_for("THING"));
}

/// A listing that carries a file but not its parent folder row (which the index can do) still
/// gets a verdict for the folder, decided by what is under it.
#[test]
fn a_folder_with_no_row_of_its_own_is_still_judged_by_its_contents() {
    let result = compare(
        &[file_sized(r"src\a.cs", 10)],
        &[file_sized(r"src\a.cs", 99)],
    );

    assert_eq!(CompareVerdict::Differs, result.verdict_for("SRC"));
}

// --- Display paths ---

/// Rebuilt rather than stored, so this is the only place a path a dialog shows is spelled, and it
/// must come back in the casing that side recorded, not the uppercased key it was paired on.
#[test]
fn a_display_path_is_rebuilt_in_each_sides_own_casing() {
    let left = [folder("Source"), file(r"Source\ReadMe.md")];
    let right = [folder("source"), file(r"source\readme.MD")];

    let result = compare(&left, &right);

    assert_eq!(
        r"Source\ReadMe.md",
        result.display_path(r"SOURCE\README.MD", CompareSide::Left)
    );
    assert_eq!(
        r"source\readme.MD",
        result.display_path(r"SOURCE\README.MD", CompareSide::Right)
    );
}

/// A listing can hold a file whose folder row is missing. Falling back to that segment of the key
/// is ugly, but it still points at the right file.
#[test]
fn a_missing_folder_row_falls_back_to_the_keys_own_spelling() {
    let result = compare(&[file(r"Gone\orphan.txt")], &[]);

    assert_eq!(
        r"GONE\orphan.txt",
        result.display_path(r"GONE\ORPHAN.TXT", CompareSide::Left)
    );
}

#[test]
fn a_top_level_display_path_is_just_the_name() {
    assert_eq!(
        "ReadMe.md",
        compare(&[file("ReadMe.md")], &[]).display_path("README.MD", CompareSide::Left)
    );
}

// --- What counts as one difference ---

/// A folder the other side lacks is one thing to report, however much is inside it. Counting its
/// contents as well would have the banner say "201 only on the left" over a list showing one new
/// folder, and disagree with the sync, which copies it in one action.
#[test]
fn a_missing_folder_counts_once_for_its_whole_subtree() {
    let left = [
        folder("bin"),
        folder(r"bin\deep"),
        file(r"bin\deep\a"),
        file(r"bin\b"),
    ];

    assert_eq!(1, compare(&left, &[]).count(CompareVerdict::LeftOnly));
}

/// A folder both sides have carries only the roll-up of its contents, so counting it as well
/// would report one changed file as two differences.
#[test]
fn a_folder_both_sides_have_is_not_a_difference_of_its_own() {
    let left = [folder("deep"), file_sized(r"deep\a.txt", 10)];
    let right = [folder("deep"), file_sized(r"deep\a.txt", 99)];

    let result = compare(&left, &right);

    assert_eq!(CompareVerdict::Differs, result.verdict_for("DEEP"));
    assert!(result.is_answered_by_an_ancestor("DEEP"));
    assert_eq!(1, result.count(CompareVerdict::Differs));
}

#[test]
fn an_entry_with_no_folder_answering_for_it_counts_itself() {
    let result = compare(&[file("a.txt"), file("b.txt")], &[]);

    assert!(!result.is_answered_by_an_ancestor("A.TXT"));
    assert_eq!(2, result.count(CompareVerdict::LeftOnly));
}

// --- Counts ---

#[test]
fn the_counts_cover_every_path_either_side_holds() {
    let left = [
        file("same.txt"),
        file("gone.txt"),
        file_modified("odd.txt", FileTime::UNKNOWN),
    ];
    let right = [
        file("same.txt"),
        file_modified("odd.txt", FileTime::UNKNOWN),
    ];

    let result = compare(&left, &right);

    assert_eq!(1, result.same_count);
    assert_eq!(1, result.unknown_count);
    assert_eq!(1, result.difference_count);
    assert!(result.any_difference());
}

/// Presence is answered before timestamps are. A file the other side simply does not have is
/// "only here" whatever its own timestamp says: there is nothing to be uncertain about, and
/// calling it unknown would quietly refuse to copy it.
#[test]
fn a_missing_timestamp_on_an_unpaired_file_is_still_only_on_one_side() {
    let result = compare(&[file_modified("odd.txt", FileTime::UNKNOWN)], &[]);

    assert_eq!(CompareVerdict::LeftOnly, result.verdict_for("ODD.TXT"));
}
