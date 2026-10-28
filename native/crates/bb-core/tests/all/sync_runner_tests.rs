// Port of tests/BertBrowser.Core.Tests/SyncRunnerTests.cs
//
// A whole sync, on real files: compare two folders, plan it, run it, put it back. Every assertion
// is on file *contents*: a sync that copies the wrong side of a pair passes every test that only
// asks whether a file is there.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bb_core::cancel::CancelToken;
use bb_core::compare::{
    CompareAvailability, FolderCompareService, SyncOutcome, SyncPlanner, SyncPreview, SyncRunner,
};
use bb_core::data::FsIndexRepository;
use bb_core::delete::{DeleteExecutor, DeleteMode, DeletePlanner, FileSystemDeleteProbe};
use bb_core::mft::NullMftIndex;
use bb_core::transfer::{FileSystemTransferProbe, TransferExecutor, TransferPlanner};
use bb_test_support::{TempDb, unique_temp_path};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-sync", "");
        std::fs::create_dir_all(&root).unwrap();
        let f = Fixture { root };
        f.dir(&["left"]);
        f.dir(&["right"]);
        f
    }

    fn p(&self, parts: &[&str]) -> String {
        parts
            .iter()
            .fold(self.root.clone(), |p, part| p.join(part))
            .to_string_lossy()
            .into_owned()
    }

    fn dir(&self, parts: &[&str]) -> String {
        let path = self.p(parts);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn file(&self, content: &str, parts: &[&str]) -> String {
        let path = self.p(parts);
        std::fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    fn with_runner<R>(&self, body: impl FnOnce(&SyncRunner<'_>) -> R) -> R {
        let transfers = TransferExecutor::default();
        let probe = FileSystemDeleteProbe;
        let deletes = DeleteExecutor::new(&probe);
        body(&SyncRunner::new(&transfers, &deletes))
    }

    /// The whole pipeline, walking both sides from disk: no index involved, so the test exercises
    /// what a comparison of two unindexed folders actually does.
    fn preview(&self, remove_right_only: bool) -> SyncPreview {
        let db = TempDb::new();
        let compare = FolderCompareService::new(
            Arc::new(FsIndexRepository::new(db.db())),
            Arc::new(NullMftIndex),
        )
        .compare(
            &self.p(&["left"]),
            &self.p(&["right"]),
            true,
            &CancelToken::none(),
            None,
        );

        assert_eq!(CompareAvailability::Ready, compare.availability);
        SyncPlanner::preview(&compare, remove_right_only)
    }

    fn run(&self, preview: &SyncPreview) -> SyncOutcome {
        let probe = FileSystemDeleteProbe;
        let plans = SyncPlanner::to_plans(
            preview,
            &TransferPlanner::new(Box::new(FileSystemTransferProbe)),
            &DeletePlanner::new(&probe),
            DeleteMode::Staged,
        );
        self.with_runner(|r| r.run(&plans, &CancelToken::none(), None))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn assert_content(path: &str, expected: &str) {
    assert!(Path::new(path).is_file(), "expected a file at {path}");
    assert_eq!(expected, std::fs::read_to_string(path).unwrap());
}

fn an_hour_from_now(path: &str) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(3600))
        .unwrap();
}

// --- the run ---

#[test]
fn a_file_the_right_side_lacks_arrives_with_its_contents() {
    let f = Fixture::new();
    f.file("hello", &["left", "new.txt"]);

    let outcome = f.run(&f.preview(false));

    assert_content(&f.p(&["right", "new.txt"]), "hello");
    assert_eq!(1, outcome.copied_count());
    assert_eq!(0, outcome.replaced_count());
    assert!(outcome.failed_copies().is_empty());
}

#[test]
fn a_stale_file_on_the_right_is_replaced_and_the_old_one_kept() {
    let f = Fixture::new();
    let newer = f.file("v2", &["left", "a.txt"]);
    f.file("v1", &["right", "a.txt"]);
    an_hour_from_now(&newer);

    let outcome = f.run(&f.preview(false));

    assert_content(&f.p(&["right", "a.txt"]), "v2");
    assert_eq!(1, outcome.replaced_count());
    let staged = TransferExecutor::staged_items(&outcome.copies[0]);
    assert_eq!(1, staged.len());
    assert_content(&staged[0], "v1");
}

#[test]
fn a_whole_folder_arrives_in_one_action() {
    let f = Fixture::new();
    f.file("x", &["left", "bin", "deep", "app.txt"]);

    let outcome = f.run(&f.preview(false));

    assert_content(&f.p(&["right", "bin", "deep", "app.txt"]), "x");
    assert_eq!(1, outcome.copied_count());
}

#[test]
fn nothing_is_removed_unless_removal_was_asked_for() {
    let f = Fixture::new();
    f.file("mine", &["right", "extra.txt"]);

    f.run(&f.preview(false));

    assert_content(&f.p(&["right", "extra.txt"]), "mine");
}

#[test]
fn what_only_the_right_has_is_removed_when_asked_for() {
    let f = Fixture::new();
    f.file("mine", &["right", "extra.txt"]);

    let outcome = f.run(&f.preview(true));

    assert!(!Path::new(&f.p(&["right", "extra.txt"])).exists());
    assert_eq!(1, outcome.removed_count());
}

/// Both halves in one run, which is the case the ordering exists for: the copies land before
/// anything is taken away, so a run stopped in the middle leaves the right side holding more than
/// it started with rather than less.
#[test]
fn a_mixed_run_copies_replaces_and_removes() {
    let f = Fixture::new();
    let newer = f.file("v2", &["left", "a.txt"]);
    f.file("v1", &["right", "a.txt"]);
    an_hour_from_now(&newer);
    f.file("fresh", &["left", "b.txt"]);
    f.file("stale", &["right", "c.txt"]);

    let outcome = f.run(&f.preview(true));

    assert_content(&f.p(&["right", "a.txt"]), "v2");
    assert_content(&f.p(&["right", "b.txt"]), "fresh");
    assert!(!Path::new(&f.p(&["right", "c.txt"])).exists());
    assert!(outcome.can_undo());
}

// --- putting it back ---

/// The undo the user was offered, in full: what arrived goes, what was replaced comes back, what
/// was removed returns. A half-undo would leave the right side in a state neither side ever had.
#[test]
fn undo_puts_the_right_side_back_exactly_as_it_was() {
    let f = Fixture::new();
    let newer = f.file("v2", &["left", "a.txt"]);
    f.file("v1", &["right", "a.txt"]);
    an_hour_from_now(&newer);
    f.file("fresh", &["left", "b.txt"]);
    f.file("stale", &["right", "c.txt"]);

    let outcome = f.run(&f.preview(true));
    let undo = f.with_runner(|r| r.undo(&outcome, &CancelToken::none()));

    assert!(undo.failed.is_empty(), "{:?}", undo.failed);
    assert_content(&f.p(&["right", "a.txt"]), "v1");
    assert!(!Path::new(&f.p(&["right", "b.txt"])).exists());
    assert_content(&f.p(&["right", "c.txt"]), "stale");

    // And the left side was never a party to any of it.
    assert_content(&f.p(&["left", "a.txt"]), "v2");
    assert_content(&f.p(&["left", "b.txt"]), "fresh");
}

#[test]
fn undo_removes_a_whole_folder_the_sync_brought() {
    let f = Fixture::new();
    f.file("x", &["left", "bin", "deep", "app.txt"]);

    let outcome = f.run(&f.preview(false));
    f.with_runner(|r| r.undo(&outcome, &CancelToken::none()));

    assert!(!Path::new(&f.p(&["right", "bin"])).exists());
    assert_content(&f.p(&["left", "bin", "deep", "app.txt"]), "x");
}

/// Once retired, the replaced file is finally gone and the one that replaced it stays, which is
/// the whole point of holding it until then.
#[test]
fn retiring_the_run_commits_what_it_set_aside() {
    let f = Fixture::new();
    let newer = f.file("v2", &["left", "a.txt"]);
    f.file("v1", &["right", "a.txt"]);
    an_hour_from_now(&newer);

    let outcome = f.run(&f.preview(false));
    SyncRunner::retire(&outcome);

    assert_content(&f.p(&["right", "a.txt"]), "v2");
    assert!(TransferExecutor::staged_items(&outcome.copies[0]).is_empty());
}

/// A second sync of a folder already in step must do nothing at all: not re-copy, and above all
/// not leave a trail of "a (2).txt" behind it.
#[test]
fn syncing_twice_is_syncing_once() {
    let f = Fixture::new();
    f.file("hello", &["left", "new.txt"]);
    f.file("x", &["left", "bin", "app.txt"]);

    f.run(&f.preview(false));
    let second = f.preview(false);

    assert!(second.actions.is_empty(), "{:?}", second.actions);
    assert_eq!(2, std::fs::read_dir(f.p(&["right"])).unwrap().count());
}
