// Port of tests/BertBrowser.Core.Tests/FolderCompareAgreementTests.cs
//
// The test that makes the two ways of listing a folder impossible to drift apart. A comparison
// reads each side either from the index or by walking the disk, and the two sides choose
// independently, so on any given run one folder may come from SQLite and the other from the
// filesystem. Every case below builds one real tree, indexes it, and compares it against its twin
// twice: once with both sides forced through the index, once with both forced through a walk.
// The verdicts and the counts must be identical. A wrong answer here does not fail a build; it
// shows up as a folder full of files that quietly went away.
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use bb_core::FileTime;
use bb_core::cancel::CancelToken;
use bb_core::compare::{CompareResult, CompareSourceKind, CompareVerdict, FolderCompareService};
use bb_core::data::FsIndexRepository;
use bb_core::fs::IndexCrawler;
use bb_core::mft::{MftIndex, NullMftIndex};
use bb_test_support::{TempDb, unique_temp_path};

struct Fixture {
    _db: TempDb,
    root: PathBuf,
    repo: Arc<FsIndexRepository>,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-cmp", "");
        std::fs::create_dir_all(&root).unwrap();
        let db = TempDb::new();
        let repo = Arc::new(FsIndexRepository::new(db.db()));
        Fixture {
            _db: db,
            root,
            repo,
        }
    }

    fn dir(&self, parts: &[&str]) -> String {
        let path = parts.iter().fold(self.root.clone(), |p, part| p.join(part));
        std::fs::create_dir_all(&path).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn file(&self, content: &str, parts: &[&str]) -> String {
        let path = parts.iter().fold(self.root.clone(), |p, part| p.join(part));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// Indexes both sides and compares them twice, asserting the two sources agree, then hands
    /// back the shared answer so a case can also say what it should be.
    fn agree(&self, left: &str, right: &str, include_hidden: bool) -> CompareResult {
        let crawler = IndexCrawler::new(self.repo.clone());
        assert!(crawler.crawl(left, &CancelToken::none(), None).unwrap());
        assert!(crawler.crawl(right, &CancelToken::none(), None).unwrap());

        let from_index =
            self.service(true)
                .compare(left, right, include_hidden, &CancelToken::none(), None);
        let from_disk =
            self.service(false)
                .compare(left, right, include_hidden, &CancelToken::none(), None);

        assert_eq!(CompareSourceKind::Index, from_index.left_source);
        assert_eq!(CompareSourceKind::Index, from_index.right_source);
        assert_eq!(CompareSourceKind::Walk, from_disk.left_source);
        assert_eq!(CompareSourceKind::Walk, from_disk.right_source);

        assert_same_answer(&from_index.result, &from_disk.result);
        from_index.result
    }

    fn service(&self, indexed: bool) -> FolderCompareService {
        let mft: Arc<dyn MftIndex> = if indexed {
            Arc::new(AlwaysIndexed)
        } else {
            Arc::new(NullMftIndex)
        };
        FolderCompareService::new(self.repo.clone(), mft)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn assert_same_answer(index: &CompareResult, walk: &CompareResult) {
    let mut from_index: Vec<(&String, &CompareVerdict)> = index.by_relative_key.iter().collect();
    let mut from_walk: Vec<(&String, &CompareVerdict)> = walk.by_relative_key.iter().collect();
    from_index.sort_by_key(|(key, _)| *key);
    from_walk.sort_by_key(|(key, _)| *key);
    assert_eq!(from_index, from_walk);

    assert_eq!(index.same_count, walk.same_count);
    assert_eq!(index.difference_count, walk.difference_count);
    assert_eq!(index.unknown_count, walk.unknown_count);
}

fn set_hidden(path: &str) {
    let status = Command::new("attrib").args(["+h", path]).status().unwrap();
    assert!(status.success());
}

fn set_last_write(path: &str, stamp: FileTime) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(stamp.to_system_time()).unwrap();
}

// --- the cases, chosen where the two sources will actually drift ---

#[test]
fn two_identical_trees_agree() {
    let f = Fixture::new();
    for side in ["left", "right"] {
        f.file("hello", &[side, "readme.md"]);
        f.file("code", &[side, "src", "main.cs"]);
        f.dir(&[side, "src", "nested"]);
    }

    let result = f.agree(&f.dir(&["left"]), &f.dir(&["right"]), true);

    assert!(!result.any_difference());
}

#[test]
fn a_file_only_on_one_side_agrees() {
    let f = Fixture::new();
    f.file("a", &["left", "shared.txt"]);
    f.file("a", &["right", "shared.txt"]);
    f.file("x", &["left", "extra.txt"]);

    let result = f.agree(&f.dir(&["left"]), &f.dir(&["right"]), true);

    assert_eq!(CompareVerdict::LeftOnly, result.verdict_for("EXTRA.TXT"));
}

/// A hidden file with hidden items turned off. The walk declines to descend a hidden folder at
/// all; the index filters on a stored flag that already means "this or an ancestor". If those two
/// ever stop lining up, one source sees files the other cannot and every one of them reads as
/// "only on this side".
#[test]
fn a_hidden_subtree_is_skipped_the_same_way_by_both_sources() {
    let f = Fixture::new();
    f.file("a", &["left", "seen.txt"]);
    f.file("a", &["right", "seen.txt"]);

    let hidden = f.dir(&["left", "secret"]);
    f.file("s", &["left", "secret", "inner.txt"]);
    set_hidden(&hidden);

    let result = f.agree(&f.dir(&["left"]), &f.dir(&["right"]), false);

    assert!(!result.any_difference());
    assert!(!result.by_relative_key.contains_key("SECRET"));
    assert!(!result.by_relative_key.contains_key(r"SECRET\INNER.TXT"));
}

/// The index stores a timestamp and reads it back; the walk hands over raw ticks. Both must land
/// on the same instant, or every file in an indexed folder reads as newer or older than its
/// identical twin by a fraction of a second.
#[test]
fn sub_second_timestamps_survive_the_round_trip_through_the_index() {
    let f = Fixture::new();
    let stamp = FileTime::from_ymd_hms(2026, 3, 14, 12, 0, 0).add_ticks(1_234_567);

    for side in ["left", "right"] {
        set_last_write(&f.file("same", &[side, "a.txt"]), stamp);
    }

    let result = f.agree(&f.dir(&["left"]), &f.dir(&["right"]), true);

    assert_eq!(CompareVerdict::Same, result.verdict_for("A.TXT"));
}

/// An empty folder is a row in the index and an entry in the walk, and it is the one thing a
/// listing keyed on files alone would drop.
#[test]
fn an_empty_folder_is_an_entry_to_both_sources() {
    let f = Fixture::new();
    // Both sides carry a file with bytes in it so the index stays the chosen source: a side
    // holding nothing measurable is indistinguishable from an unmeasured volume, and falls to the
    // walk on purpose.
    f.file("anchor", &["left", "anchor.txt"]);
    f.file("anchor", &["right", "anchor.txt"]);
    f.dir(&["left", "empty"]);

    let result = f.agree(&f.dir(&["left"]), &f.dir(&["right"]), true);

    assert_eq!(CompareVerdict::LeftOnly, result.verdict_for("EMPTY"));
}

/// A file sitting in a delete's holding folder is still on disk (that is what makes Ctrl+Z work)
/// but it has been deleted as far as the user is concerned. Left in by either source it would
/// show as "only on this side" and be offered for deletion a second time.
#[test]
fn a_held_delete_is_invisible_to_both_sources() {
    let f = Fixture::new();
    f.file("a", &["left", "kept.txt"]);
    f.file("a", &["right", "kept.txt"]);
    f.file(
        "gone",
        &["left", ".bertbrowser-trash", "delete-1", "removed.txt"],
    );

    let result = f.agree(&f.dir(&["left"]), &f.dir(&["right"]), true);

    assert!(!result.any_difference());
    assert!(!result.by_relative_key.keys().any(|k| k.contains("REMOVED")));
}

/// Nothing is compared by content, so two files of the same length whose bytes differ are "same"
/// once their timestamps match, and both sources have to say so, since a walk that reached for
/// the bytes and an index that could not would part company immediately.
#[test]
fn same_length_different_bytes_reads_the_same_to_both_sources() {
    let f = Fixture::new();
    let stamp = FileTime::from_ymd_hms(2026, 1, 2, 3, 4, 5);
    set_last_write(&f.file("aaaa", &["left", "a.txt"]), stamp);
    set_last_write(&f.file("bbbb", &["right", "a.txt"]), stamp);

    assert_eq!(
        CompareVerdict::Same,
        f.agree(&f.dir(&["left"]), &f.dir(&["right"]), true)
            .verdict_for("A.TXT")
    );
}

#[test]
fn a_file_against_a_folder_of_the_same_name_agrees() {
    let f = Fixture::new();
    f.file("x", &["left", "thing"]);
    f.file("y", &["right", "thing", "inner.txt"]);

    let result = f.agree(&f.dir(&["left"]), &f.dir(&["right"]), true);

    assert_eq!(CompareVerdict::Differs, result.verdict_for("THING"));
}

/// Reports every volume indexed, so the service takes its SQLite path. The other half of the
/// matrix is `NullMftIndex`, which reports none.
struct AlwaysIndexed;

impl MftIndex for AlwaysIndexed {
    fn start_with(&self, mode: bb_core::mft::IndexStartMode) -> Result<(), String> {
        NullMftIndex.start_with(mode)
    }
    fn stop(&self) {}
    fn any_indexed(&self) -> bool {
        true
    }
    fn is_building(&self) -> bool {
        false
    }
    fn building_drives(&self) -> Vec<String> {
        Vec::new()
    }
    fn completed_roots(&self) -> Vec<String> {
        Vec::new()
    }
    fn presence(&self) -> bb_core::mft::IndexerPresence {
        NullMftIndex.presence()
    }
    fn is_indexed(&self, _path_key: &str) -> bool {
        true
    }
    fn status_text(&self) -> String {
        String::new()
    }
    fn can_retry(&self) -> bool {
        false
    }
    fn can_start(&self) -> bool {
        false
    }
    fn retry(&self) {}
    fn change_log(&self) -> bb_core::changes::ChangeLogPolicy {
        NullMftIndex.change_log()
    }
    fn set_change_log(&self, _policy: bb_core::changes::ChangeLogPolicy) {}
    fn subscribe(&self, listener: std::sync::Arc<dyn bb_core::mft::IndexListener>) -> u64 {
        NullMftIndex.subscribe(listener)
    }
    fn unsubscribe(&self, id: u64) {
        NullMftIndex.unsubscribe(id)
    }
}
