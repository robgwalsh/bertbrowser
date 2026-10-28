// Port of tests/BertBrowser.Core.Tests/MftDirectorySizeBuilderTests.cs.
use std::collections::HashMap;

use bb_core::FileTime;
use bb_core::attributes::{ARCHIVE, DIRECTORY, HIDDEN};
use bb_core::data::DirSizeResult;
use bb_core::mft::{DirCache, MftDirectorySizeBuilder, MftFileRecord};

const ROOT: u64 = 5;

fn epoch() -> FileTime {
    FileTime::from_unix_millis(0)
}

fn dir(rec: u64, parent: u64, name: &str, hidden: bool) -> MftFileRecord {
    MftFileRecord {
        record_number: rec,
        parent_record_number: parent,
        name: name.to_string(),
        is_directory: true,
        attributes: DIRECTORY | if hidden { HIDDEN } else { 0 },
        size: 0,
        modified_utc: epoch(),
        created_utc: epoch(),
    }
}

fn file(rec: u64, parent: u64, name: &str, size: i64) -> MftFileRecord {
    MftFileRecord {
        record_number: rec,
        parent_record_number: parent,
        name: name.to_string(),
        is_directory: false,
        attributes: ARCHIVE,
        size,
        modified_utc: epoch(),
        created_utc: epoch(),
    }
}

fn by_key(results: Vec<DirSizeResult>) -> HashMap<String, DirSizeResult> {
    results
        .into_iter()
        .map(|r| (r.path_key.clone(), r))
        .collect()
}

#[test]
fn build_rolls_sizes_and_counts_up_the_tree() {
    let mut b = MftDirectorySizeBuilder::new();
    // C:\Users\Rob\{a.txt=100, b.txt=200}; C:\Users\c.txt=50; C:\Users\Junction (empty, e.g. reparse)
    b.add(&dir(20, ROOT, "Users", false));
    b.add(&dir(21, 20, "Rob", false));
    b.add(&dir(40, 20, "Junction", false));
    b.add(&file(30, 21, "a.txt", 100));
    b.add(&file(31, 21, "b.txt", 200));
    b.add(&file(32, 20, "c.txt", 50));

    let results = by_key(b.build(r"C:\", epoch(), &mut DirCache::new()));

    assert_eq!(300, results[r"C:\USERS\ROB"].size_bytes);
    assert_eq!(2, results[r"C:\USERS\ROB"].file_count);
    assert_eq!(0, results[r"C:\USERS\ROB"].dir_count);

    assert_eq!(350, results[r"C:\USERS"].size_bytes); // 300 under Rob + 50 direct
    assert_eq!(3, results[r"C:\USERS"].file_count);
    assert_eq!(2, results[r"C:\USERS"].dir_count); // Rob + Junction

    // A childless directory (junction target lives under its own record) contributes nothing.
    assert_eq!(0, results[r"C:\USERS\JUNCTION"].size_bytes);
    assert_eq!(0, results[r"C:\USERS\JUNCTION"].file_count);
}

#[test]
fn build_populates_dir_cache_and_marks_complete() {
    let mut b = MftDirectorySizeBuilder::new();
    b.add(&dir(20, ROOT, "Data", false));
    b.add(&file(30, 20, "x.bin", 10));

    let mut cache = DirCache::new();
    let results = b.build(r"D:\", epoch(), &mut cache);

    assert_eq!(r"D:\Data", cache[&20].path); // reused to build fs_entry file rows
    assert!(results.iter().all(|r| !r.incomplete)); // MFT sees everything, never partial
}

#[test]
fn build_skips_subtrees_whose_parent_chain_is_broken() {
    // A directory whose parent was never added (e.g. under a skipped $Extend) is unreachable
    // from the root and must not appear.
    let mut b = MftDirectorySizeBuilder::new();
    b.add(&dir(50, 11 /* not added */, "Orphan", false));
    b.add(&file(60, 50, "y.bin", 999));

    let results = b.build(r"C:\", epoch(), &mut DirCache::new());

    assert!(!results.iter().any(|r| r.path_key.contains("ORPHAN")));
}
