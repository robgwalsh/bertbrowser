// Port of tests/BertBrowser.Core.Tests/FsIndexRepositorySubtreeTests.cs: reading a whole subtree out
// of the index to pair against another one. A key that is off by a separator pairs nothing.
use std::collections::HashSet;

use bb_core::FileTime;
use bb_core::cancel::CancelToken;
use bb_core::data::{FsEntryRow, FsIndexRepository, FsSubtreeRow};
use bb_paths::PathKey;
use bb_test_support::TempDb;

fn noon() -> FileTime {
    FileTime::from_ymd_hms(2026, 3, 14, 12, 0, 0)
}

fn row(display_path: &str, is_dir: bool, size: i64, hidden: bool) -> FsEntryRow {
    FsEntryRow {
        path_key: PathKey::canonicalize(display_path).unwrap(),
        name: bb_paths::file_name(display_path).to_string(),
        is_directory: is_dir,
        size_bytes: size,
        modified_utc: noon(),
        hidden,
        attributes: 0,
        created_utc: FileTime::UNKNOWN,
    }
}

fn setup(rows: &[FsEntryRow]) -> (TempDb, FsIndexRepository) {
    let t = TempDb::new();
    let repo = FsIndexRepository::new(t.db());
    repo.upsert_entries(rows, 1).unwrap();
    (t, repo)
}

fn read(repo: &FsIndexRepository, root: &str, include_hidden: bool) -> Vec<FsSubtreeRow> {
    repo.subtree(root, include_hidden, 1000, None, &CancelToken::none())
        .unwrap()
        .0
}

#[test]
fn keys_are_relative_to_the_root_and_the_root_itself_is_not_among_them() {
    let (_t, repo) = setup(&[
        row(r"C:\Data", true, 0, false),
        row(r"C:\Data\src", true, 0, false),
        row(r"C:\Data\src\Main.cs", false, 42, false),
    ]);

    let rows = read(&repo, r"C:\Data", true);
    let keys: Vec<&str> = rows.iter().map(|r| r.relative_key.as_str()).collect();
    assert_eq!(vec!["SRC", r"SRC\MAIN.CS"], keys);
    assert_eq!(42, rows[1].size_bytes);
    assert_eq!(noon(), rows[1].modified_utc);
}

/// A drive root canonicalizes with its trailing separator kept, so slicing the wrong number of
/// characters off leaves every key starting with one, and nothing ever pairs.
#[test]
fn a_drive_root_does_not_leave_a_leading_separator_on_its_children() {
    let (_t, repo) = setup(&[
        row(r"C:\notes.txt", false, 0, false),
        row(r"C:\sub", true, 0, false),
    ]);

    let mut keys: Vec<String> = read(&repo, r"C:\", true)
        .into_iter()
        .map(|r| r.relative_key)
        .collect();
    keys.sort();
    assert_eq!(vec!["NOTES.TXT", "SUB"], keys);
}

#[test]
fn the_key_is_uppercased_and_the_name_keeps_its_casing() {
    let (_t, repo) = setup(&[
        row(r"C:\Data\Source", true, 0, false),
        row(r"C:\Data\Source\ReadMe.md", false, 0, false),
    ]);

    let rows = read(&repo, r"C:\Data", true);
    let file = rows.iter().find(|r| !r.is_directory).unwrap();
    assert_eq!(r"SOURCE\README.MD", file.relative_key);
    assert_eq!("ReadMe.md", file.name);
}

#[test]
fn hidden_rows_are_dropped_on_request() {
    let (_t, repo) = setup(&[
        row(r"C:\Data\seen.txt", false, 0, false),
        row(r"C:\Data\secret.txt", false, 0, true),
    ]);

    assert_eq!(2, read(&repo, r"C:\Data", true).len());
    let visible = read(&repo, r"C:\Data", false);
    assert_eq!(1, visible.len());
    assert_eq!("SEEN.TXT", visible[0].relative_key);
}

/// A file in a delete's holding folder is still on disk but has been deleted as far as the user is
/// concerned. Left in, it would be offered for deletion a second time.
#[test]
fn the_exclude_predicate_drops_a_whole_held_subtree() {
    let (_t, repo) = setup(&[
        row(r"C:\Data\keep.txt", false, 0, false),
        row(r"C:\Data\.bertbrowser-trash", true, 0, false),
        row(
            r"C:\Data\.bertbrowser-trash\delete-1\gone.txt",
            false,
            0,
            false,
        ),
    ]);

    let is_held = |key: &str| key.contains(".BERTBROWSER-TRASH");
    let rows = repo
        .subtree(r"C:\Data", true, 1000, Some(&is_held), &CancelToken::none())
        .unwrap()
        .0;
    assert_eq!(1, rows.len());
    assert_eq!("KEEP.TXT", rows[0].relative_key);
}

/// A ceiling that reported half as if it were everything would be the worst of all.
#[test]
fn the_cap_truncates_rather_than_throwing() {
    let rows: Vec<FsEntryRow> = (0..10)
        .map(|i| row(&format!(r"C:\Data\f{i:02}.txt"), false, 0, false))
        .collect();
    let (_t, repo) = setup(&rows);

    let (rows, truncated) = repo
        .subtree(r"C:\Data", true, 4, None, &CancelToken::none())
        .unwrap();
    assert!(truncated);
    assert_eq!(4, rows.len());
    assert!(
        !repo
            .subtree(r"C:\Data", true, 10, None, &CancelToken::none())
            .unwrap()
            .1
    );
}

/// Ancestors are strict prefixes, so the table's own order already hands every parent over before
/// its children.
#[test]
fn rows_arrive_with_every_parent_before_its_children() {
    let (_t, repo) = setup(&[
        row(r"C:\Data\b", true, 0, false),
        row(r"C:\Data\b\deep", true, 0, false),
        row(r"C:\Data\b\deep\x.txt", false, 0, false),
        row(r"C:\Data\a.txt", false, 0, false),
    ]);

    let mut seen: HashSet<String> = HashSet::new();
    for r in read(&repo, r"C:\Data", true) {
        if let Some(cut) = r.relative_key.rfind('\\') {
            assert!(seen.contains(&r.relative_key[..cut]), "{}", r.relative_key);
        }
        seen.insert(r.relative_key);
    }
    assert_eq!(4, seen.len());
}
