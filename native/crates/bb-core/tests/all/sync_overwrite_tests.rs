// Port of tests/BertBrowser.Core.Tests/SyncOverwriteTests.cs
//
// The one resolution a copy may use to take over an existing name, and the undo that pays for it.
// Every test asserts on file *contents*, following transfer_executor_tests: a replacement that
// loses the original is exactly the failure that would otherwise pass.
use std::path::{Path, PathBuf};

use bb_core::cancel::CancelToken;
use bb_core::transfer::{
    ConflictResolution, Resolutions, TransferExecutor, TransferOutcome, TransferPlanner,
    TransferVerb,
};
use bb_paths::PathKey;
use bb_test_support::unique_temp_path;

struct Fixture {
    root: PathBuf,
    planner: TransferPlanner,
    executor: TransferExecutor,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-ovw", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            root,
            planner: TransferPlanner::default(),
            executor: TransferExecutor::default(),
        }
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

    fn run(
        &self,
        sources: &[&str],
        destination: &str,
        resolution: ConflictResolution,
        verb: TransferVerb,
    ) -> TransferOutcome {
        let sources: Vec<String> = sources.iter().map(|s| s.to_string()).collect();
        let plan = self.planner.plan(&sources, destination, verb);
        let resolutions: Resolutions = plan
            .transfers
            .iter()
            .map(|t| (PathKey::canonicalize(&t.source_path).unwrap(), resolution))
            .collect();
        self.executor
            .execute(&plan, Some(&resolutions), &CancelToken::none(), None, None)
    }

    fn copy(
        &self,
        sources: &[&str],
        destination: &str,
        resolution: ConflictResolution,
    ) -> TransferOutcome {
        self.run(sources, destination, resolution, TransferVerb::Copy)
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

fn staged(outcome: &TransferOutcome) -> Vec<String> {
    TransferExecutor::staged_items(outcome)
}

fn single(items: &[String]) -> &str {
    assert_eq!(1, items.len(), "{items:?}");
    &items[0]
}

// --- writing ---

#[test]
fn a_copy_with_overwrite_takes_the_name() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.file("old", &["dst", "a.txt"]);

    f.copy(
        &[&f.p(&["src", "a.txt"])],
        &f.dir(&["dst"]),
        ConflictResolution::Overwrite,
    );

    assert_content(&f.p(&["dst", "a.txt"]), "new");
}

/// The whole reason the value is allowed to exist: what it displaced is set aside intact, not
/// erased to make room.
#[test]
fn the_displaced_file_is_intact_in_staging() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.file("old", &["dst", "a.txt"]);

    let outcome = f.copy(
        &[&f.p(&["src", "a.txt"])],
        &f.dir(&["dst"]),
        ConflictResolution::Overwrite,
    );

    assert_content(single(&staged(&outcome)), "old");
}

/// The existing rule, left exactly as it was. A drag-drop copy offering Replace must still land
/// beside the original rather than over it.
#[test]
fn replace_on_a_copy_is_still_downgraded_to_keep_both() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.file("old", &["dst", "a.txt"]);

    let outcome = f.copy(
        &[&f.p(&["src", "a.txt"])],
        &f.dir(&["dst"]),
        ConflictResolution::Replace,
    );

    assert_content(&f.p(&["dst", "a.txt"]), "old");
    assert_content(&f.p(&["dst", "a (2).txt"]), "new");
    assert!(staged(&outcome).is_empty());
}

/// An ordinary copy stays out of the shared undo slot. Only a caller holding a sync outcome may
/// reverse one, and it comes in through `TransferExecutor::undo_copies`.
#[test]
fn a_copy_outcome_is_still_not_undoable_on_its_own() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);

    assert!(
        !f.copy(
            &[&f.p(&["src", "a.txt"])],
            &f.dir(&["dst"]),
            ConflictResolution::Overwrite
        )
        .can_undo()
    );
}

#[test]
fn a_folder_can_be_overwritten_too() {
    let f = Fixture::new();
    f.file("new", &["src", "lib", "x.txt"]);
    f.file("old", &["dst", "lib", "x.txt"]);
    f.file("only-old", &["dst", "lib", "gone.txt"]);

    let outcome = f.copy(
        &[&f.p(&["src", "lib"])],
        &f.dir(&["dst"]),
        ConflictResolution::Overwrite,
    );

    assert_content(&f.p(&["dst", "lib", "x.txt"]), "new");
    assert!(!Path::new(&f.p(&["dst", "lib", "gone.txt"])).exists());
    assert_content(
        &format!("{}\\gone.txt", single(&staged(&outcome))),
        "only-old",
    );
}

// --- undo ---

#[test]
fn undo_puts_the_displaced_file_back_and_removes_the_copy() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.file("old", &["dst", "a.txt"]);

    let outcome = f.copy(
        &[&f.p(&["src", "a.txt"])],
        &f.dir(&["dst"]),
        ConflictResolution::Overwrite,
    );
    let undo = f.executor.undo_copies(&outcome, &CancelToken::none());

    assert!(undo.failed.is_empty(), "{:?}", undo.failed);
    assert_content(&f.p(&["dst", "a.txt"]), "old");
    assert_content(&f.p(&["src", "a.txt"]), "new"); // the source was never touched
}

/// A sync adds as well as replaces, and the undo the user was offered covers both. A file the
/// run put at a name that was free has to go, or the right side is left half-synced with no way
/// to tell which half.
#[test]
fn undo_removes_what_the_copy_merely_added() {
    let f = Fixture::new();
    f.file("new", &["src", "fresh.txt"]);

    let outcome = f.copy(
        &[&f.p(&["src", "fresh.txt"])],
        &f.dir(&["dst"]),
        ConflictResolution::Overwrite,
    );
    assert_content(&f.p(&["dst", "fresh.txt"]), "new");

    f.executor.undo_copies(&outcome, &CancelToken::none());

    assert!(!Path::new(&f.p(&["dst", "fresh.txt"])).exists());
    assert_content(&f.p(&["src", "fresh.txt"]), "new");
}

#[test]
fn undo_removes_a_whole_copied_folder() {
    let f = Fixture::new();
    f.file("a", &["src", "lib", "deep", "x.txt"]);

    let outcome = f.copy(
        &[&f.p(&["src", "lib"])],
        &f.dir(&["dst"]),
        ConflictResolution::Overwrite,
    );
    assert!(Path::new(&f.p(&["dst", "lib", "deep"])).is_dir());

    f.executor.undo_copies(&outcome, &CancelToken::none());

    assert!(!Path::new(&f.p(&["dst", "lib"])).exists());
    assert_content(&f.p(&["src", "lib", "deep", "x.txt"]), "a");
}

/// Undo leaves no staging folder behind once it has emptied it, or every reversed sync would leave
/// a hidden directory in the destination for good.
#[test]
fn undo_clears_the_staging_folder_it_emptied() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.file("old", &["dst", "a.txt"]);

    let outcome = f.copy(
        &[&f.p(&["src", "a.txt"])],
        &f.dir(&["dst"]),
        ConflictResolution::Overwrite,
    );
    f.executor.undo_copies(&outcome, &CancelToken::none());

    assert!(staged(&outcome).is_empty());
    assert!(!Path::new(single(&outcome.staging_directories)).exists());
}

#[test]
fn undo_refuses_a_move() {
    let f = Fixture::new();
    f.file("x", &["src", "a.txt"]);

    let outcome = f.run(
        &[&f.p(&["src", "a.txt"])],
        &f.dir(&["dst"]),
        ConflictResolution::KeepBoth,
        TransferVerb::Move,
    );

    assert!(
        !f.executor
            .undo_copies(&outcome, &CancelToken::none())
            .failed
            .is_empty()
    );
    assert_content(&f.p(&["dst", "a.txt"]), "x");
}

/// Once the run can no longer be undone the displaced original is finally gone, and nothing else
/// with it.
#[test]
fn commit_staging_erases_the_displaced_original_and_nothing_else() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.file("old", &["dst", "a.txt"]);
    f.file("bystander", &["dst", "b.txt"]);

    let outcome = f.copy(
        &[&f.p(&["src", "a.txt"])],
        &f.dir(&["dst"]),
        ConflictResolution::Overwrite,
    );
    TransferExecutor::commit_staging(&outcome);

    assert!(staged(&outcome).is_empty());
    assert_content(&f.p(&["dst", "a.txt"]), "new");
    assert_content(&f.p(&["dst", "b.txt"]), "bystander");
    assert_content(&f.p(&["src", "a.txt"]), "new");
}
