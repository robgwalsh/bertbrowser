// Port of tests/BertBrowser.Core.Tests/ArchiveIndexBuilderTests.cs.
use bb_core::FileTime;
use bb_core::archives::{
    ArchiveCapabilities, ArchiveFailure, ArchiveIndex, ArchiveIndexBuilder, RawArchiveEntry,
};

fn file(key: &str) -> RawArchiveEntry {
    file_sized(key, 100, 40)
}

fn file_sized(key: &str, size: i64, packed: i64) -> RawArchiveEntry {
    RawArchiveEntry::new(
        Some(key),
        size,
        packed,
        Some(FileTime::from_ymd_hms(2026, 1, 2, 3, 4, 5)),
        false,
    )
}

fn dir(key: &str, modified: Option<FileTime>) -> RawArchiveEntry {
    RawArchiveEntry::new(Some(key), 0, 0, modified, true)
}

fn build(entries: Vec<RawArchiveEntry>) -> ArchiveIndex {
    ArchiveIndexBuilder::build(
        entries,
        ArchiveCapabilities::UNKNOWN,
        ArchiveIndex::MAX_ENTRIES,
    )
}

fn names(index: &ArchiveIndex, dir: &str) -> Vec<String> {
    index
        .children(dir)
        .expect("directory")
        .iter()
        .map(|n| n.name.clone())
        .collect()
}

/// The case that matters most: zips routinely carry no folder entries at all. Trusting the
/// explicit ones would leave "src" in every path and reachable from nowhere.
#[test]
fn src_is_reachable_in_a_zip_that_omits_its_folder_entries() {
    let index = build(vec![file("src/lib/util.js"), file("readme.txt")]);

    assert!(index.find("src").is_some());
    assert!(index.find("src").unwrap().is_directory);
    assert!(index.find("src\\lib").is_some());

    let mut top = names(&index, "");
    top.sort();
    assert_eq!(vec!["readme.txt", "src"], top);

    assert_eq!(vec!["util.js"], names(&index, "src\\lib"));
}

#[test]
fn separators_become_windows_separators() {
    let index = build(vec![file("a/b/c.txt")]);

    assert!(index.find("a\\b\\c.txt").is_some());
    assert_eq!("a\\b\\c.txt", index.find("a\\b\\c.txt").unwrap().path);
}

/// An explicit directory entry decorates a node; it must never be what creates one.
#[test]
fn an_explicit_folder_entry_only_supplies_its_timestamp() {
    let stamp = FileTime::from_ymd_hms(2020, 5, 6, 7, 8, 9);
    let index = build(vec![file("src/lib/util.js"), dir("src/", Some(stamp))]);

    assert_eq!(Some(stamp), index.find("src").unwrap().modified);
    // The implicitly created one carries none, and renders blank rather than 1601.
    assert_eq!(None, index.find("src\\lib").unwrap().modified);
}

#[test]
fn an_empty_folder_entry_is_still_a_folder() {
    let index = build(vec![dir("empty/", None)]);

    let node = index.find("empty").expect("node");
    assert!(node.is_directory);
    assert!(index.children("empty").unwrap().is_empty());
    assert_eq!(0, node.size_bytes);
}

/// Zip Slip. Refused at read time so the entry never exists to be extracted, which makes the
/// extractor's own check a second line rather than the only one.
#[test]
fn an_entry_that_escapes_the_root_is_refused() {
    for key in [
        "../../Windows/System32/evil.dll",
        "..\\..\\Windows\\System32\\evil.dll",
        "good/../../../evil.dll",
        "/etc/passwd",
        "\\Windows\\evil.dll",
        "C:\\Windows\\evil.dll",
        "\\\\server\\share\\evil.dll",
    ] {
        let index = build(vec![file(key), file("safe.txt")]);

        assert_eq!(1, index.file_count, "{key}");
        assert_eq!(1, index.refused_count, "{key}");
        assert_eq!(vec!["safe.txt"], names(&index, ""), "{key}");
    }
}

#[test]
fn an_entry_with_no_name_is_refused_rather_than_crashing() {
    let index = build(vec![
        RawArchiveEntry::new(None, 10, 5, None, false),
        file("ok.txt"),
    ]);

    assert_eq!(1, index.file_count);
    assert_eq!(1, index.refused_count);
}

/// Directory sizes are the exact recursive sum: nothing is walked to get them.
#[test]
fn folder_sizes_are_the_exact_recursive_sum() {
    let index = build(vec![
        file_sized("src/a.txt", 100, 30),
        file_sized("src/lib/b.txt", 250, 70),
        file_sized("top.txt", 7, 7),
    ]);

    assert_eq!(350, index.find("src").unwrap().size_bytes);
    assert_eq!(250, index.find("src\\lib").unwrap().size_bytes);
    assert_eq!(357, index.root().size_bytes);
    assert_eq!(100, index.find("src").unwrap().compressed_bytes);
}

/// An empty folder inside an archive really is zero bytes. That is a fact, not a missing row,
/// which is why the never-zero rule that governs dir_size_cache does not apply here.
#[test]
fn an_empty_folder_is_zero_bytes_and_that_is_a_fact() {
    let index = build(vec![dir("empty/", None)]);
    assert_eq!(0, index.find("empty").unwrap().size_bytes);
}

#[test]
fn duplicate_keys_keep_the_last_and_count_the_rest() {
    let index = build(vec![
        file_sized("dup.txt", 1, 40),
        file_sized("dup.txt", 999, 40),
    ]);

    assert_eq!(1, index.file_count);
    assert_eq!(1, index.refused_count);
    assert_eq!(999, index.find("dup.txt").unwrap().size_bytes);
    assert_eq!(1, index.children("").unwrap().len());
}

/// The directory wins: it is reachable and has contents the file would hide.
#[test]
fn a_directory_beats_a_file_of_the_same_name() {
    let index = build(vec![file("thing"), file("thing/inner.txt")]);

    assert!(index.find("thing").unwrap().is_directory);
    assert_eq!(vec!["inner.txt"], names(&index, "thing"));
    assert_eq!(1, index.refused_count);
}

#[test]
fn the_entry_cap_refuses_rather_than_half_building() {
    let many: Vec<RawArchiveEntry> = (0..20).map(|i| file(&format!("f{i}.txt"))).collect();

    let index = ArchiveIndexBuilder::build(many, ArchiveCapabilities::UNKNOWN, 10);

    assert!(!index.ok());
    assert_eq!(ArchiveFailure::TooManyEntries, index.failure);
    assert!(index.error.as_deref().unwrap().contains("10"));
}

#[test]
fn children_of_something_that_is_not_a_directory_is_null() {
    let index = build(vec![file("a.txt")]);

    assert!(index.children("a.txt").is_none());
    assert!(index.children("nope").is_none());
    assert!(index.children("").is_some());
}

#[test]
fn entries_come_back_in_name_order() {
    let index = build(vec![
        file("zebra.txt"),
        file("apple.txt"),
        file("Mango.txt"),
    ]);

    assert_eq!(
        vec!["apple.txt", "Mango.txt", "zebra.txt"],
        names(&index, "")
    );
}

/// Windows drops a trailing dot or space, so such a key would not round-trip.
#[test]
fn a_segment_windows_would_rewrite_is_cleaned_rather_than_trusted() {
    for key in ["bad./x.txt", "bad /x.txt"] {
        let index = build(vec![file(key)]);

        assert!(index.find("bad\\x.txt").is_some(), "{key}");
    }
}

#[test]
fn a_symlink_entry_is_listed_but_carries_its_target() {
    let mut link = RawArchiveEntry::new(Some("link"), 0, 0, None, false);
    link.link_target = Some("/etc/passwd".to_string());
    let index = build(vec![link]);

    assert_eq!(
        Some("/etc/passwd"),
        index.find("link").unwrap().link_target.as_deref()
    );
}

#[test]
fn capabilities_ride_along_unchanged() {
    let caps = ArchiveCapabilities::new(true, true, false);

    let index = ArchiveIndexBuilder::build(vec![file("a.txt")], caps, ArchiveIndex::MAX_ENTRIES);

    assert_eq!(caps, index.capabilities);
}
