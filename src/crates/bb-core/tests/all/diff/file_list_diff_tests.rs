// Port of tests/BertBrowser.Core.Tests/FileListDiffTests.cs. The diff behind live refresh: rows that
// did not change must be left alone, since replacing them loses the selection, the scroll position
// and the keyboard focus.
use bb_core::FileTime;
use bb_core::attributes::{
    ARCHIVE, HIDDEN, NORMAL, NOT_CONTENT_INDEXED, OFFLINE, READONLY, SYSTEM, TEMPORARY,
};
use bb_core::fs::{FileListChanges, FileListDiff};
use bb_core::models::FileEntry;
use bb_paths::PathKey;

fn entry_at(name: &str, is_dir: bool, size: i64, minute: u32, attributes: u32) -> FileEntry {
    FileEntry::new(
        name,
        &format!(r"C:\Dir\{name}"),
        is_dir,
        size,
        FileTime::from_ymd_hms(2026, 1, 1, 0, minute, 0),
        attributes,
    )
}

fn entry(name: &str) -> FileEntry {
    entry_at(name, false, 10, 0, NORMAL)
}

fn key(name: &str) -> String {
    PathKey::canonicalize(&format!(r"C:\Dir\{name}")).unwrap()
}

fn compute(current: &[FileEntry], next: &[FileEntry]) -> FileListChanges {
    FileListDiff::compute(current, next).unwrap()
}

#[test]
fn an_identical_listing_is_no_change_at_all() {
    let listing = [entry("a.txt"), entry("b.txt")];

    let changes = compute(&listing, &[entry("a.txt"), entry("b.txt")]);

    assert!(!changes.any());
}

#[test]
fn a_new_file_is_added() {
    let changes = compute(&[entry("a.txt")], &[entry("a.txt"), entry("b.txt")]);

    assert_eq!(changes.added.len(), 1);
    assert_eq!(changes.added[0].name, "b.txt");
    assert!(changes.removed.is_empty());
    assert!(changes.updated.is_empty());
}

#[test]
fn a_vanished_file_is_removed() {
    let changes = compute(&[entry("a.txt"), entry("b.txt")], &[entry("a.txt")]);

    assert_eq!(changes.removed, vec![key("b.txt")]);
    assert!(changes.added.is_empty());
}

/// A rename is not a special case: the old path went and a new one arrived.
#[test]
fn a_rename_is_one_removal_and_one_addition() {
    let changes = compute(&[entry("old.txt")], &[entry("new.txt")]);

    assert_eq!(changes.removed, vec![key("old.txt")]);
    assert_eq!(changes.added.len(), 1);
    assert_eq!(changes.added[0].name, "new.txt");
}

#[test]
fn a_row_whose_details_changed_is_updated() {
    let rows: [(i64, u32, u32); 3] = [
        (99, 0, NORMAL), // grew
        (10, 5, NORMAL), // touched
        (10, 0, HIDDEN), // hidden bit set
    ];
    for (size, minute, attributes) in rows {
        let changes = compute(
            &[entry("a.txt")],
            &[entry_at("a.txt", false, size, minute, attributes)],
        );

        assert_eq!(
            changes.updated.len(),
            1,
            "size {size}, minute {minute}, attributes {attributes:#x}"
        );
        assert_eq!(changes.updated[0].name, "a.txt");
        assert!(changes.added.is_empty(), "size {size}, minute {minute}");
        assert!(changes.removed.is_empty(), "size {size}, minute {minute}");
    }
}

/// The case that would otherwise slip through. Path keys are uppercased, so a rename that only
/// changes casing is the *same* key; comparing keys alone would report no change and leave the list
/// showing the old spelling. Drop the ordinal name comparison and this goes red.
#[test]
fn a_case_only_rename_is_an_update_not_nothing() {
    let changes = compute(&[entry("notes.txt")], &[entry("Notes.txt")]);

    assert_eq!(changes.updated.len(), 1);
    assert_eq!(changes.updated[0].name, "Notes.txt");
    assert!(changes.added.is_empty());
    assert!(changes.removed.is_empty());
}

/// A file replaced by a folder of the same name is still a change worth showing.
#[test]
fn a_file_becoming_a_folder_is_an_update() {
    let changes = compute(&[entry("thing")], &[entry_at("thing", true, 10, 0, NORMAL)]);

    assert_eq!(changes.updated.len(), 1);
    assert!(changes.updated[0].is_directory);
}

#[test]
fn an_emptied_folder_removes_everything() {
    let changes = compute(&[entry("a.txt"), entry("b.txt")], &[]);

    assert_eq!(changes.removed.len(), 2);
    assert!(changes.any());
}

#[test]
fn a_folder_filling_from_empty_adds_everything() {
    let changes = compute(&[], &[entry("a.txt"), entry("b.txt")]);

    assert_eq!(changes.added.len(), 2);
}

/// Comparison is by canonical key, so the same file listed with different casing is the same file,
/// not a removal plus an addition.
#[test]
fn paths_are_compared_canonically() {
    let before = [FileEntry::new(
        "a.txt",
        r"C:\Dir\a.txt",
        false,
        10,
        FileTime::UNKNOWN,
        NORMAL,
    )];
    let after = [FileEntry::new(
        "a.txt",
        r"c:\DIR\a.txt",
        false,
        10,
        FileTime::UNKNOWN,
        NORMAL,
    )];

    assert!(!compute(&before, &after).any());
}

// --- What counts as a change, now that a row can render more than a name and a size ---

fn stamped(created: FileTime, accessed: FileTime, attributes: u32) -> FileEntry {
    FileEntry {
        name: "a.txt".to_string(),
        full_path: r"C:\Dir\a.txt".to_string(),
        is_directory: false,
        size_bytes: 10,
        modified_utc: FileTime::from_ymd_hms(2026, 1, 1, 0, 0, 0),
        attributes,
        created_utc: created,
        accessed_utc: accessed,
    }
}

/// The trap this feature opened. Reading a file moves its access time, and this app reads files
/// constantly. Weighing it would mark every row the user merely looked at as changed on the next
/// pass.
#[test]
fn an_access_time_change_alone_is_not_a_change() {
    let before = [stamped(
        FileTime::UNKNOWN,
        FileTime::from_ymd_hms(2026, 1, 1, 0, 0, 0),
        NORMAL,
    )];
    let after = [stamped(
        FileTime::UNKNOWN,
        FileTime::from_ymd_hms(2026, 6, 1, 0, 0, 0),
        NORMAL,
    )];

    assert!(!compute(&before, &after).any());
}

#[test]
fn a_creation_time_change_is_a_change() {
    let before = [stamped(
        FileTime::from_ymd_hms(2026, 1, 1, 0, 0, 0),
        FileTime::UNKNOWN,
        NORMAL,
    )];
    let after = [stamped(
        FileTime::from_ymd_hms(2026, 6, 1, 0, 0, 0),
        FileTime::UNKNOWN,
        NORMAL,
    )];

    assert_eq!(compute(&before, &after).updated.len(), 1);
}

#[test]
fn an_attribute_a_row_can_render_is_a_change() {
    for flag in [READONLY, HIDDEN, SYSTEM, ARCHIVE] {
        let before = [stamped(FileTime::UNKNOWN, FileTime::UNKNOWN, NORMAL)];
        let after = [stamped(FileTime::UNKNOWN, FileTime::UNKNOWN, flag)];

        assert_eq!(compute(&before, &after).updated.len(), 1, "flag {flag:#x}");
    }
}

/// A sync client flips these on files nobody touched, so they must not churn the list.
#[test]
fn an_attribute_no_row_renders_is_not_a_change() {
    for flag in [OFFLINE, NOT_CONTENT_INDEXED, TEMPORARY] {
        let before = [stamped(FileTime::UNKNOWN, FileTime::UNKNOWN, ARCHIVE)];
        let after = [stamped(
            FileTime::UNKNOWN,
            FileTime::UNKNOWN,
            ARCHIVE | flag,
        )];

        assert!(!compute(&before, &after).any(), "flag {flag:#x}");
    }
}

/// The meta-test: proof the comparison above can actually see an attribute change at all, so the
/// "not a change" rows are not passing because nothing is compared.
#[test]
fn the_attribute_comparison_can_tell_the_difference() {
    let before = [stamped(FileTime::UNKNOWN, FileTime::UNKNOWN, ARCHIVE)];
    let after = [stamped(
        FileTime::UNKNOWN,
        FileTime::UNKNOWN,
        ARCHIVE | READONLY,
    )];

    assert!(compute(&before, &after).any());
}
