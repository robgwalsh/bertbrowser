// Port of tests/BertBrowser.Core.Tests/ArchiveListingTests.cs.
use std::io::Cursor;

use bb_core::archives::ArchiveListing;
use bb_test_support::archive_support::{TempRoot, write_zip};

/// A zip in memory. Bytes, not text with a byte-order mark: the sizes these tests assert on would
/// otherwise silently be three bytes over.
fn zip(entries: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
    let root = TempRoot::new("bertbrowser-listing");
    let path = write_zip(&root.path("a.zip"), entries);
    Cursor::new(std::fs::read(path).unwrap())
}

#[test]
fn lists_every_entry_with_its_size() {
    let mut zip = zip(&[("readme.txt", b"hello"), ("src/app.js", b"console.log(1)")]);
    let contents = ArchiveListing::read(&mut zip, ArchiveListing::DEFAULT_MAX_ENTRIES);

    assert_eq!(None, contents.error);
    assert_eq!(2, contents.total_count);
    assert_eq!(2, contents.entries.len());
    assert_eq!(5 + 14, contents.total_bytes);
    assert!(
        contents
            .entries
            .iter()
            .any(|e| e.path == "readme.txt" && e.size_bytes == 5)
    );
}

#[test]
fn separators_are_windows_separators() {
    // The rest of the app writes paths one way; a preview should not be the exception.
    let contents = ArchiveListing::read(
        &mut zip(&[("src/app.js", b"x")]),
        ArchiveListing::DEFAULT_MAX_ENTRIES,
    );
    assert!(contents.entries.iter().any(|e| e.path == "src\\app.js"));
}

#[test]
fn entries_come_back_in_path_order() {
    let mut zip = zip(&[
        ("zebra.txt", b"z"),
        ("alpha.txt", b"a"),
        ("Middle.txt", b"m"),
    ]);
    let paths: Vec<String> = ArchiveListing::read(&mut zip, ArchiveListing::DEFAULT_MAX_ENTRIES)
        .entries
        .iter()
        .map(|e| e.path.clone())
        .collect();
    assert_eq!(vec!["alpha.txt", "Middle.txt", "zebra.txt"], paths);
}

#[test]
fn a_folder_entry_is_marked_as_one_and_carries_no_size() {
    let mut zip = zip(&[("docs/", b""), ("docs/a.txt", b"a")]);
    let contents = ArchiveListing::read(&mut zip, ArchiveListing::DEFAULT_MAX_ENTRIES);
    let folders: Vec<_> = contents.entries.iter().filter(|e| e.is_directory).collect();
    assert_eq!(1, folders.len());
    assert_eq!("docs\\", folders[0].path);
    assert_eq!(0, folders[0].size_bytes);
}

#[test]
fn the_entry_cap_truncates_but_the_total_still_counts_everything() {
    let many: Vec<String> = (0..50).map(|i| format!("file{i:02}.txt")).collect();
    let entries: Vec<(&str, &[u8])> = many.iter().map(|n| (n.as_str(), b"x" as &[u8])).collect();
    let mut zip = zip(&entries);

    let contents = ArchiveListing::read(&mut zip, 10);
    assert!(contents.truncated);
    assert_eq!(10, contents.entries.len());
    assert_eq!(50, contents.total_count);
    assert_eq!(50, contents.total_bytes);
}

#[test]
fn an_archive_inside_the_cap_is_not_reported_as_truncated() {
    let mut zip = zip(&[("a.txt", b"a"), ("b.txt", b"b")]);
    assert!(!ArchiveListing::read(&mut zip, 2).truncated);
}

#[test]
fn a_damaged_archive_is_a_message_rather_than_a_throw() {
    let mut rubbish = Cursor::new(b"this is not a zip file at all".to_vec());
    let contents = ArchiveListing::read(&mut rubbish, ArchiveListing::DEFAULT_MAX_ENTRIES);

    assert!(contents.error.is_some());
    assert!(contents.entries.is_empty());
    assert_eq!(0, contents.total_count);
}

#[test]
fn an_empty_archive_is_readable_and_empty() {
    let mut zip = zip(&[]);
    let contents = ArchiveListing::read(&mut zip, ArchiveListing::DEFAULT_MAX_ENTRIES);
    assert_eq!(None, contents.error);
    assert!(contents.entries.is_empty());
    assert_eq!(0.0, contents.compression_ratio());
}

#[test]
fn the_stream_is_left_open_for_the_caller_to_close() {
    let mut zip = zip(&[("a.txt", b"a")]);
    ArchiveListing::read(&mut zip, ArchiveListing::DEFAULT_MAX_ENTRIES);
    // Reading it twice must work: the listing owns nothing.
    assert_eq!(
        1,
        ArchiveListing::read(&mut zip, ArchiveListing::DEFAULT_MAX_ENTRIES).total_count
    );
}
