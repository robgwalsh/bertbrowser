// Port of tests/BertBrowser.Core.Tests/TransferExecutorTests.cs. Executes real transfers against
// real files. Every test asserts on file *contents*, not just existence, so a transfer that loses
// or truncates data fails loudly.
use std::collections::HashMap;
use std::io::Write;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bb_core::attributes::REPARSE_POINT;
use bb_core::cancel::CancelToken;
use bb_core::fs::DirectoryRemoval;
use bb_core::transfer::{
    ConflictResolution, FileCopier, FileSystemFileCopier, FileSystemTransferProbe, PauseGate,
    PlannedTransfer, ProgressFn, Resolutions, TransferError, TransferExecutor,
    TransferMergeExpander, TransferOutcome, TransferPlan, TransferPlanner, TransferProgress,
    TransferVerb,
};
use bb_paths::PathKey;
use bb_test_support::unique_temp_path;

use crate::directory_removal_tests::try_create_junction;

struct Fixture {
    root: PathBuf,
    planner: TransferPlanner,
    executor: TransferExecutor,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-xfer", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            root,
            planner: TransferPlanner::default(),
            executor: TransferExecutor::default(),
        }
    }

    fn p(&self, parts: &[&str]) -> String {
        let mut p = self.root.clone();
        for part in parts {
            p.push(part);
        }
        p.to_string_lossy().into_owned()
    }

    fn dir(&self, parts: &[&str]) -> String {
        let p = self.p(parts);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn file(&self, content: &str, parts: &[&str]) -> String {
        let p = self.p(parts);
        std::fs::create_dir_all(Path::new(&p).parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    fn run(
        &self,
        sources: &[&str],
        destination: &str,
        verb: TransferVerb,
        resolution: ConflictResolution,
    ) -> TransferOutcome {
        let plan = self.plan(sources, destination, verb);
        let resolutions: Resolutions = plan
            .transfers
            .iter()
            .map(|t| (PathKey::canonicalize(&t.source_path).unwrap(), resolution))
            .collect();
        self.execute(&plan, Some(&resolutions))
    }

    fn mv(&self, sources: &[&str], destination: &str) -> TransferOutcome {
        self.run(
            sources,
            destination,
            TransferVerb::Move,
            ConflictResolution::KeepBoth,
        )
    }

    fn plan(&self, sources: &[&str], destination: &str, verb: TransferVerb) -> TransferPlan {
        let sources: Vec<String> = sources.iter().map(|s| s.to_string()).collect();
        self.planner.plan(&sources, destination, verb)
    }

    fn execute(&self, plan: &TransferPlan, resolutions: Option<&Resolutions>) -> TransferOutcome {
        self.executor
            .execute(plan, resolutions, &CancelToken::none(), None, None)
    }

    fn execute_watched(&self, plan: &TransferPlan) -> Vec<TransferProgress> {
        let mut seen = Vec::new();
        self.executor.execute(
            plan,
            None,
            &CancelToken::none(),
            Some(&mut |p| seen.push(p)),
            None,
        );
        seen
    }

    /// Plans, expands and runs the way `ShellViewModel.ExecuteDropAsync` does.
    fn run_merged(
        &self,
        source: &str,
        destination: &str,
        resolution: ConflictResolution,
        verb: TransferVerb,
    ) -> TransferOutcome {
        let plan = TransferMergeExpander::default()
            .expand(
                self.plan(&[source], destination, verb),
                &CancelToken::none(),
            )
            .unwrap();
        let resolutions: Resolutions = plan
            .conflicts()
            .iter()
            .map(|t| (PathKey::canonicalize(&t.source_path).unwrap(), resolution))
            .collect();
        self.execute(&plan, Some(&resolutions))
    }

    /// A destination whose full path is well past 260 characters, with its folders made.
    fn deep_path(&self, name: &str) -> String {
        let mut dir = self.root.clone();
        while dir.to_string_lossy().len() < 300 {
            dir.push("d".repeat(40));
        }
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name).to_string_lossy().into_owned();
        assert!(
            path.len() > 260,
            "the fixture has to be past MAX_PATH to prove anything."
        );
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = DirectoryRemoval::remove_tree(&self.root.to_string_lossy());
    }
}

fn assert_content(path: &str, expected: &str) {
    assert!(Path::new(path).is_file(), "expected a file at {path}");
    assert_eq!(expected, std::fs::read_to_string(path).unwrap(), "{path}");
}

fn file_exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| !m.is_dir())
}

fn dir_exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & 0x10 != 0)
}

/// `File.Open(path, Open, Read, FileShare.None)`.
fn lock_exclusively(path: &str) -> std::fs::File {
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap()
}

/// Fails loudly rather than skipping: these tests guard a path where a junction could be silently
/// destroyed, so quietly not running them is worse than a red build. A symbolic link needs a
/// privilege an ordinary account may not have, so a junction (which does not) stands in for it:
/// both are reparse-point directories, which is all the code under test looks at.
fn create_directory_link(link: &str, target: &str) {
    if std::os::windows::fs::symlink_dir(target, link).is_ok() {
        return;
    }
    assert!(
        try_create_junction(Path::new(link), Path::new(target)),
        "This test needs permission to create a directory link. It covers a data-loss path and must not be skipped silently."
    );
}

fn canon(path: &str) -> String {
    PathKey::canonicalize(path).unwrap()
}

// --- moves that should just work ---

#[test]
fn move_file_lands_at_destination_and_leaves_the_source() {
    let f = Fixture::new();
    let source = f.file("hello", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);

    let outcome = f.mv(&[&source], &dest);

    assert!(outcome.failed.is_empty());
    assert_content(&f.p(&["dest", "a.txt"]), "hello");
    assert!(!file_exists(&source));
    assert_eq!(f.p(&["dest", "a.txt"]), outcome.completed[0].final_path);
    assert_eq!(1, outcome.completed.len());
}

#[test]
fn move_directory_takes_the_whole_tree() {
    let f = Fixture::new();
    f.file("one", &["src", "tree", "a.txt"]);
    f.file("two", &["src", "tree", "sub", "b.txt"]);
    f.file("three", &["src", "tree", "sub", "deep", "c.txt"]);
    let dest = f.dir(&["dest"]);

    let outcome = f.mv(&[&f.p(&["src", "tree"])], &dest);

    assert!(outcome.failed.is_empty());
    assert_content(&f.p(&["dest", "tree", "a.txt"]), "one");
    assert_content(&f.p(&["dest", "tree", "sub", "b.txt"]), "two");
    assert_content(&f.p(&["dest", "tree", "sub", "deep", "c.txt"]), "three");
    assert!(!dir_exists(&f.p(&["src", "tree"])));
}

#[test]
fn move_many_items_all_arrive() {
    let f = Fixture::new();
    let a = f.file("a", &["src", "a.txt"]);
    let b = f.file("b", &["src", "b.txt"]);
    f.file("c", &["src", "folder", "c.txt"]);
    let dest = f.dir(&["dest"]);

    let outcome = f.mv(&[&a, &b, &f.p(&["src", "folder"])], &dest);

    assert_eq!(3, outcome.completed.len());
    assert_content(&f.p(&["dest", "a.txt"]), "a");
    assert_content(&f.p(&["dest", "b.txt"]), "b");
    assert_content(&f.p(&["dest", "folder", "c.txt"]), "c");
}

#[test]
fn nested_selection_moves_once_and_keeps_everything() {
    // Selecting a folder and something inside it must not try to move the inner item twice.
    let f = Fixture::new();
    f.file("deep", &["src", "tree", "inner", "leaf.txt"]);
    let dest = f.dir(&["dest"]);

    let outcome = f.mv(
        &[&f.p(&["src", "tree"]), &f.p(&["src", "tree", "inner"])],
        &dest,
    );

    assert_eq!(1, outcome.completed.len());
    assert!(outcome.failed.is_empty());
    assert_content(&f.p(&["dest", "tree", "inner", "leaf.txt"]), "deep");
}

// --- conflicts ---

#[test]
fn keep_both_numbers_the_newcomer_and_leaves_the_existing_file_alone() {
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("existing", &["dest", "a.txt"]);

    let outcome = f.run(
        &[&source],
        &dest,
        TransferVerb::Move,
        ConflictResolution::KeepBoth,
    );

    assert_content(&f.p(&["dest", "a.txt"]), "existing");
    assert_content(&f.p(&["dest", "a (2).txt"]), "new");
    assert_eq!(f.p(&["dest", "a (2).txt"]), outcome.completed[0].final_path);
}

#[test]
fn keep_both_numbers_directories_after_the_whole_name() {
    let f = Fixture::new();
    f.file("new", &["src", "tree", "x.txt"]);
    f.dir(&["dest", "tree"]);

    f.run(
        &[&f.p(&["src", "tree"])],
        &f.p(&["dest"]),
        TransferVerb::Move,
        ConflictResolution::KeepBoth,
    );

    assert_content(&f.p(&["dest", "tree (2)", "x.txt"]), "new");
}

#[test]
fn skip_leaves_both_sides_untouched() {
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("existing", &["dest", "a.txt"]);

    let outcome = f.run(
        &[&source],
        &dest,
        TransferVerb::Move,
        ConflictResolution::Skip,
    );

    assert_content(&f.p(&["dest", "a.txt"]), "existing");
    assert_content(&source, "new");
    assert!(outcome.completed.is_empty());
    assert_eq!(1, outcome.skipped.len());
}

#[test]
fn replace_stages_the_displaced_file_instead_of_deleting_it() {
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("existing", &["dest", "a.txt"]);

    let outcome = f.run(
        &[&source],
        &dest,
        TransferVerb::Move,
        ConflictResolution::Replace,
    );

    assert_content(&f.p(&["dest", "a.txt"]), "new");
    assert!(!file_exists(&source));

    // The displaced content must still be on disk, not gone.
    let staged = TransferExecutor::staged_items(&outcome);
    assert_eq!(1, staged.len());
    assert_content(&staged[0], "existing");
}

#[test]
fn replace_is_downgraded_to_keep_both_for_a_copy() {
    // Copy is defined as purely additive; it must never displace anything.
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("existing", &["dest", "a.txt"]);

    f.run(
        &[&source],
        &dest,
        TransferVerb::Copy,
        ConflictResolution::Replace,
    );

    assert_content(&f.p(&["dest", "a.txt"]), "existing");
    assert_content(&f.p(&["dest", "a (2).txt"]), "new");
    assert_content(&source, "new");
}

#[test]
fn replace_that_fails_after_staging_puts_the_displaced_file_back() {
    // Clearing the name succeeds, then the move of the locked source fails. The displaced file
    // must return to its own name rather than being stranded in staging with no undo record.
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("existing", &["dest", "a.txt"]);

    let outcome = {
        let _lock = lock_exclusively(&source);
        f.run(
            &[&source],
            &dest,
            TransferVerb::Move,
            ConflictResolution::Replace,
        )
    };

    assert_eq!(1, outcome.failed.len());
    assert!(outcome.completed.is_empty());
    assert_content(&f.p(&["dest", "a.txt"]), "existing"); // back under its own name
    assert_content(&source, "new"); // and the source never left
    assert!(TransferExecutor::staged_items(&outcome).is_empty());
}

#[test]
fn two_sources_with_the_same_name_both_survive() {
    let f = Fixture::new();
    let one = f.file("first", &["one", "a.txt"]);
    let two = f.file("second", &["two", "a.txt"]);
    let dest = f.dir(&["dest"]);

    f.run(
        &[&one, &two],
        &dest,
        TransferVerb::Move,
        ConflictResolution::KeepBoth,
    );

    assert_content(&f.p(&["dest", "a.txt"]), "first");
    assert_content(&f.p(&["dest", "a (2).txt"]), "second");
}

// --- undo ---

#[test]
fn undo_puts_every_item_back_where_it_came_from() {
    let f = Fixture::new();
    let a = f.file("a", &["src", "a.txt"]);
    let b = f.file("b", &["src", "b.txt"]);
    f.file("c", &["src", "folder", "c.txt"]);
    let dest = f.dir(&["dest"]);

    let outcome = f.mv(&[&a, &b, &f.p(&["src", "folder"])], &dest);
    assert!(outcome.can_undo());

    let undo = f.executor.undo(&outcome, &CancelToken::none());

    assert!(undo.failed.is_empty());
    assert_eq!(3, undo.restored);
    assert_content(&a, "a");
    assert_content(&b, "b");
    assert_content(&f.p(&["src", "folder", "c.txt"]), "c");
    assert!(!file_exists(&f.p(&["dest", "a.txt"])));
    assert!(!dir_exists(&f.p(&["dest", "folder"])));
}

#[test]
fn undo_restores_both_sides_of_a_replace() {
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("existing", &["dest", "a.txt"]);

    let outcome = f.run(
        &[&source],
        &dest,
        TransferVerb::Move,
        ConflictResolution::Replace,
    );
    let undo = f.executor.undo(&outcome, &CancelToken::none());

    assert!(undo.failed.is_empty());
    assert_content(&source, "new"); // the mover went home
    assert_content(&f.p(&["dest", "a.txt"]), "existing"); // the displaced file came back
    assert!(TransferExecutor::staged_items(&outcome).is_empty());
    assert_eq!(1, outcome.staging_directories.len());
    assert!(!dir_exists(&outcome.staging_directories[0]));
}

#[test]
fn undo_restores_a_renamed_item_to_its_original_name() {
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("existing", &["dest", "a.txt"]);

    let outcome = f.run(
        &[&source],
        &dest,
        TransferVerb::Move,
        ConflictResolution::KeepBoth,
    );
    f.executor.undo(&outcome, &CancelToken::none());

    assert_content(&source, "new");
    assert_content(&f.p(&["dest", "a.txt"]), "existing");
    assert!(!file_exists(&f.p(&["dest", "a (2).txt"])));
}

#[test]
fn undo_refuses_to_overwrite_something_that_took_the_original_name() {
    let f = Fixture::new();
    let source = f.file("moved", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);

    let outcome = f.mv(&[&source], &dest);
    std::fs::write(&source, "a different file now lives here").unwrap();

    let undo = f.executor.undo(&outcome, &CancelToken::none());

    assert_eq!(0, undo.restored);
    assert_eq!(1, undo.failed.len());
    assert_content(&source, "a different file now lives here"); // not clobbered
    assert_content(&f.p(&["dest", "a.txt"]), "moved"); // still where it was moved to
}

#[test]
fn undo_reports_an_item_that_moved_on_afterwards() {
    let f = Fixture::new();
    let source = f.file("moved", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);

    let outcome = f.mv(&[&source], &dest);
    std::fs::remove_file(f.p(&["dest", "a.txt"])).unwrap();

    let undo = f.executor.undo(&outcome, &CancelToken::none());

    assert_eq!(0, undo.restored);
    assert_eq!(1, undo.failed.len());
}

#[test]
fn undo_is_refused_for_a_copy() {
    let f = Fixture::new();
    let source = f.file("x", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);

    let outcome = f.run(
        &[&source],
        &dest,
        TransferVerb::Copy,
        ConflictResolution::KeepBoth,
    );

    assert!(!outcome.can_undo());
    assert!(
        !f.executor
            .undo(&outcome, &CancelToken::none())
            .failed
            .is_empty()
    );
    assert_content(&f.p(&["dest", "a.txt"]), "x"); // the copy is untouched
}

// --- staging hygiene ---

#[test]
fn purge_staging_leaves_a_staging_folder_that_still_holds_data() {
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("existing", &["dest", "a.txt"]);

    let outcome = f.run(
        &[&source],
        &dest,
        TransferVerb::Move,
        ConflictResolution::Replace,
    );
    f.executor.purge_staging(&outcome);

    // Purging must never be the thing that destroys the displaced copy.
    let staged = TransferExecutor::staged_items(&outcome);
    assert_eq!(1, staged.len());
    assert_content(&staged[0], "existing");
}

#[test]
fn purge_staging_removes_the_folder_once_empty() {
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("existing", &["dest", "a.txt"]);

    let outcome = f.run(
        &[&source],
        &dest,
        TransferVerb::Move,
        ConflictResolution::Replace,
    );
    let staged = TransferExecutor::staged_items(&outcome);
    assert_eq!(1, staged.len());
    std::fs::remove_file(&staged[0]).unwrap();
    f.executor.purge_staging(&outcome);

    assert!(!dir_exists(&outcome.staging_directories[0]));
}

#[test]
fn purge_staging_ignores_a_path_it_did_not_create() {
    let f = Fixture::new();
    let precious = f.dir(&["not-staging"]);
    f.file("keep me", &["not-staging", "important.txt"]);
    let outcome = TransferOutcome::new(
        TransferVerb::Move,
        &f.root.to_string_lossy(),
        vec![],
        vec![],
        vec![],
        vec![precious],
        false,
    );

    f.executor.purge_staging(&outcome);

    assert_content(&f.p(&["not-staging", "important.txt"]), "keep me");
}

// --- live-disk revalidation between plan and execution ---

#[test]
fn source_deleted_after_planning_fails_that_item_only() {
    let f = Fixture::new();
    let doomed = f.file("gone soon", &["src", "doomed.txt"]);
    let survivor = f.file("fine", &["src", "survivor.txt"]);
    let dest = f.dir(&["dest"]);

    let plan = f.plan(&[&doomed, &survivor], &dest, TransferVerb::Move);
    std::fs::remove_file(&doomed).unwrap();
    let outcome = f.execute(&plan, None);

    assert_eq!(1, outcome.failed.len());
    assert_eq!(doomed, outcome.failed[0].source_path);
    assert_content(&f.p(&["dest", "survivor.txt"]), "fine");
}

#[test]
fn destination_turned_into_a_subfolder_after_planning_is_refused() {
    // Plan against a sibling folder, then make that folder live inside the source.
    let f = Fixture::new();
    f.file("payload", &["tree", "data.txt"]);
    let dest = f.dir(&["dest"]);

    let plan = f.plan(&[&f.p(&["tree"])], &dest, TransferVerb::Move);

    // Re-point the plan's destination at a folder inside the source.
    let inner = f.dir(&["tree", "inner"]);
    let sneaky = TransferPlan::new(
        plan.verb,
        &inner,
        plan.transfers.clone(),
        plan.rejected.clone(),
    );
    let outcome = f.execute(&sneaky, None);

    assert_eq!(1, outcome.failed.len());
    assert_content(&f.p(&["tree", "data.txt"]), "payload"); // the tree survived intact
    assert!(dir_exists(&f.p(&["tree"])));
    assert_eq!(0, std::fs::read_dir(&dest).unwrap().count());
}

#[test]
fn destination_deleted_after_planning_fails_without_touching_the_source() {
    let f = Fixture::new();
    let source = f.file("payload", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);

    let plan = f.plan(&[&source], &dest, TransferVerb::Move);
    std::fs::remove_dir(&dest).unwrap();
    let outcome = f.execute(&plan, None);

    assert_eq!(1, outcome.failed.len());
    assert_content(&source, "payload");
}

#[test]
fn conflict_appearing_after_planning_falls_back_to_keep_both() {
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);

    let plan = f.plan(&[&source], &dest, TransferVerb::Move);
    assert!(!plan.transfers[0].conflicts);

    f.file("appeared", &["dest", "a.txt"]); // someone else got there first
    f.execute(&plan, None);

    assert_content(&f.p(&["dest", "a.txt"]), "appeared");
    assert_content(&f.p(&["dest", "a (2).txt"]), "new");
}

// --- failure isolation, progress, cancellation ---

#[test]
fn a_locked_file_fails_alone_and_the_rest_still_move() {
    let f = Fixture::new();
    let locked = f.file("locked", &["src", "locked.txt"]);
    let other = f.file("other", &["src", "other.txt"]);
    let dest = f.dir(&["dest"]);

    {
        let _lock = lock_exclusively(&locked);
        let outcome = f.mv(&[&locked, &other], &dest);

        assert_eq!(1, outcome.failed.len());
        assert_eq!(locked, outcome.failed[0].source_path);
        assert_content(&f.p(&["dest", "other.txt"]), "other");
    }
    assert_content(&locked, "locked"); // never lost
}

#[test]
fn progress_reports_every_item() {
    let f = Fixture::new();
    let a = f.file("a", &["src", "a.txt"]);
    let b = f.file("b", &["src", "b.txt"]);
    let dest = f.dir(&["dest"]);

    let plan = f.plan(&[&a, &b], &dest, TransferVerb::Move);
    let seen = f.execute_watched(&plan);

    assert_eq!(2, seen.iter().map(|p| p.total).max().unwrap());
    assert!(seen.iter().any(|p| p.current_name == "a.txt"));
    assert!(seen.iter().any(|p| p.current_name == "b.txt"));
}

#[test]
fn cancellation_stops_cleanly_without_losing_anything() {
    let f = Fixture::new();
    let a = f.file("a", &["src", "a.txt"]);
    let b = f.file("b", &["src", "b.txt"]);
    let dest = f.dir(&["dest"]);

    let cancel = CancelToken::new();
    cancel.cancel();
    let plan = f.plan(&[&a, &b], &dest, TransferVerb::Move);
    let outcome = f.executor.execute(&plan, None, &cancel, None, None);

    assert!(outcome.completed.is_empty());
    assert!(outcome.failed.is_empty());
    assert!(outcome.cancelled);
    assert_content(&a, "a");
    assert_content(&b, "b");
}

// --- bytes ---

#[test]
fn progress_counts_the_bytes_it_wrote_not_just_the_items() {
    let f = Fixture::new();
    let a = f.file(&"a".repeat(4_000), &["src", "a.txt"]);
    let b = f.file(&"b".repeat(6_000), &["src", "b.txt"]);
    let dest = f.dir(&["dest"]);

    let plan = f.plan(&[&a, &b], &dest, TransferVerb::Copy);
    let seen = f.execute_watched(&plan);

    assert_eq!(10_000, seen.last().unwrap().bytes_done);
    assert_eq!(2, seen.last().unwrap().done);
}

#[test]
fn progress_counts_every_file_in_a_tree_it_copies() {
    let f = Fixture::new();
    f.file(&"x".repeat(1_500), &["src", "tree", "one.txt"]);
    f.file(&"y".repeat(2_500), &["src", "tree", "sub", "two.txt"]);
    let dest = f.dir(&["dest"]);

    let plan = f.plan(&[&f.p(&["src", "tree"])], &dest, TransferVerb::Copy);
    let seen = f.execute_watched(&plan);

    assert_eq!(4_000, seen.last().unwrap().bytes_done);
}

#[test]
fn progress_never_goes_backwards() {
    let f = Fixture::new();
    f.file(&"x".repeat(3_000), &["src", "tree", "one.txt"]);
    f.file(&"y".repeat(3_000), &["src", "tree", "two.txt"]);
    let c = f.file(&"z".repeat(3_000), &["src", "c.txt"]);
    let dest = f.dir(&["dest"]);

    let plan = f.plan(&[&f.p(&["src", "tree"]), &c], &dest, TransferVerb::Copy);
    let seen = f.execute_watched(&plan);

    for i in 1..seen.len() {
        assert!(
            seen[i].bytes_done >= seen[i - 1].bytes_done,
            "report {i} went backwards: {} then {}",
            seen[i - 1].bytes_done,
            seen[i].bytes_done
        );
    }
}

#[test]
fn a_same_volume_move_reports_no_bytes_because_it_is_a_rename() {
    // Not a stall and not a bug: relocating within a volume writes nothing, and a bar that
    // crawled through 50 GB of "progress" here would be an invention.
    let f = Fixture::new();
    let a = f.file(&"a".repeat(8_000), &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);

    let plan = f.plan(&[&a], &dest, TransferVerb::Move);
    let seen = f.execute_watched(&plan);

    assert_eq!(0, seen.last().unwrap().bytes_done);
    assert_content(&f.p(&["dest", "a.txt"]), &"a".repeat(8_000));
}

// --- cancelling part-way through a file ---

type Hook = Arc<dyn Fn(&str, i64) + Send + Sync>;

/// A copier that really copies, but in a fixed number of chunks with a hook between them, so a
/// cancel can be made to land in the middle of a named file every time. It reproduces the real
/// copier's contract (return `Cancelled` and leave no partial destination) because the tests are
/// about what the *executor* does around that contract.
struct SteppedCopier {
    chunks: usize,
    after_chunk: Hook,
}

impl SteppedCopier {
    fn new(
        chunks: usize,
        after_chunk: impl Fn(&str, i64) + Send + Sync + 'static,
    ) -> SteppedCopier {
        SteppedCopier {
            chunks,
            after_chunk: Arc::new(after_chunk),
        }
    }

    fn write(
        &self,
        source: &str,
        destination: &str,
        mut progress: Option<ProgressFn<'_>>,
        cancel: &CancelToken,
        delete_source: bool,
    ) -> Result<(), TransferError> {
        let bytes = std::fs::read(source)?;
        let size = ((bytes.len() as f64 / self.chunks as f64).ceil() as usize).max(1);
        let mut cancelled = false;

        {
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)?;
            let mut offset = 0;
            while offset < bytes.len() {
                if cancel.is_cancelled() {
                    cancelled = true;
                    break;
                }
                let count = size.min(bytes.len() - offset);
                output.write_all(&bytes[offset..offset + count])?;
                if let Some(p) = progress.as_mut() {
                    p((offset + count) as i64, bytes.len() as i64);
                }
                (self.after_chunk)(destination, (offset + count) as i64);
                offset += count;
            }
        }

        if cancelled || cancel.is_cancelled() {
            std::fs::remove_file(destination)?;
            return Err(TransferError::Cancelled);
        }

        if delete_source {
            std::fs::remove_file(source)?;
        }
        Ok(())
    }
}

impl FileCopier for SteppedCopier {
    fn copy(
        &self,
        source: &str,
        destination: &str,
        progress: Option<ProgressFn<'_>>,
        cancel: &CancelToken,
    ) -> Result<(), TransferError> {
        self.write(source, destination, progress, cancel, false)
    }

    fn move_file(
        &self,
        source: &str,
        destination: &str,
        progress: Option<ProgressFn<'_>>,
        cancel: &CancelToken,
    ) -> Result<(), TransferError> {
        self.write(source, destination, progress, cancel, true)
    }
}

/// An executor whose bytes move through a copier we can interrupt at a known point. A real
/// mid-file cancel would need a fixture large enough to still be copying when the token trips,
/// which is neither fast nor deterministic.
fn with_copier(copier: SteppedCopier) -> TransferExecutor {
    TransferExecutor::new(Box::new(FileSystemTransferProbe), Box::new(copier))
}

#[test]
fn cancelling_inside_a_file_leaves_the_source_and_says_it_was_cancelled() {
    let f = Fixture::new();
    let a = f.file(&"a".repeat(4_000), &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);

    let cancel = CancelToken::new();
    let copier = {
        let cancel = cancel.clone();
        SteppedCopier::new(4, move |_, _| cancel.cancel())
    };
    let plan = f.plan(&[&a], &dest, TransferVerb::Copy);

    let outcome = with_copier(copier).execute(&plan, None, &cancel, None, None);

    assert!(outcome.cancelled);
    assert!(outcome.completed.is_empty());
    assert!(outcome.failed.is_empty());
    assert_content(&a, &"a".repeat(4_000));
    assert!(
        !file_exists(&f.p(&["dest", "a.txt"])),
        "a half-written file was left where a finished one belongs."
    );
}

#[test]
fn cancelling_inside_a_directory_copy_removes_the_partial_tree() {
    // A copy is defined as purely additive, so a cancelled one must add nothing: half a tree
    // reads as a finished copy to everything that looks at it afterwards.
    let f = Fixture::new();
    f.file(&"x".repeat(2_000), &["src", "tree", "one.txt"]);
    f.file(&"y".repeat(2_000), &["src", "tree", "two.txt"]);
    f.file(&"z".repeat(2_000), &["src", "tree", "three.txt"]);
    let dest = f.dir(&["dest"]);

    let cancel = CancelToken::new();
    let copier = {
        let cancel = cancel.clone();
        SteppedCopier::new(2, move |_, done| {
            if done >= 2_000 {
                cancel.cancel(); // one whole file across, then stop
            }
        })
    };
    let plan = f.plan(&[&f.p(&["src", "tree"])], &dest, TransferVerb::Copy);

    let outcome = with_copier(copier).execute(&plan, None, &cancel, None, None);

    assert!(outcome.cancelled);
    assert!(
        !dir_exists(&f.p(&["dest", "tree"])),
        "a partial tree was left at the destination."
    );
    assert!(dir_exists(&f.p(&["src", "tree"])));
}

#[test]
fn a_cancelled_run_keeps_what_already_got_across_and_stays_undoable() {
    let f = Fixture::new();
    let a = f.file(&"a".repeat(2_000), &["src", "a.txt"]);
    let b = f.file(&"b".repeat(2_000), &["src", "b.txt"]);
    let dest = f.dir(&["dest"]);

    let cancel = CancelToken::new();
    let copier = {
        let cancel = cancel.clone();
        SteppedCopier::new(2, move |destination, _| {
            if destination.to_lowercase().ends_with("b.txt") {
                cancel.cancel();
            }
        })
    };
    // Cross-volume is what makes a file move copy rather than rename, so the copier is reached.
    let plan = TransferPlan::new(
        TransferVerb::Move,
        &dest,
        vec![
            PlannedTransfer::new(&a, false, &f.p(&["dest", "a.txt"]), false),
            PlannedTransfer::new(&b, false, &f.p(&["dest", "b.txt"]), false),
        ],
        vec![],
    );

    let outcome = with_copier(copier).execute(&plan, None, &cancel, None, None);

    assert!(outcome.cancelled);
    assert_eq!(1, outcome.completed.len());
    assert!(
        outcome.can_undo(),
        "what got across before the cancel still has to be undoable."
    );
    assert_content(&f.p(&["dest", "a.txt"]), &"a".repeat(2_000));
    assert_content(&b, &"b".repeat(2_000)); // never left home
}

// --- the real copier's own guarantees ---

#[test]
fn the_real_copier_reports_progress_while_a_large_file_is_copied() {
    let f = Fixture::new();
    let source = f.p(&["src", "big.bin"]);
    f.dir(&["src"]);
    std::fs::write(&source, vec![0u8; 8 * 1024 * 1024]).unwrap();
    let destination = f.p(&["dest", "big.bin"]);
    f.dir(&["dest"]);

    let mut reports: Vec<i64> = Vec::new();
    FileSystemFileCopier
        .copy(
            &source,
            &destination,
            Some(&mut |done, _| reports.push(done)),
            &CancelToken::none(),
        )
        .unwrap();

    assert!(!reports.is_empty());
    assert_eq!(
        8 * 1024 * 1024,
        std::fs::metadata(&destination).unwrap().len()
    );
}

#[test]
fn the_real_copier_deletes_the_destination_when_cancelled_mid_file() {
    let f = Fixture::new();
    let source = f.p(&["src", "big.bin"]);
    f.dir(&["src"]);
    std::fs::write(&source, vec![0u8; 64 * 1024 * 1024]).unwrap();
    let destination = f.p(&["dest", "big.bin"]);
    f.dir(&["dest"]);

    let cancel = CancelToken::new();
    let result = FileSystemFileCopier.copy(
        &source,
        &destination,
        Some(&mut |_, _| cancel.cancel()),
        &cancel,
    );

    assert!(matches!(result, Err(TransferError::Cancelled)));
    assert!(
        !file_exists(&destination),
        "a cancelled copy left its partial destination behind."
    );
    assert!(file_exists(&source));
}

/// The raw Win32 copy needs the `\\?\` prefix past MAX_PATH; the standard library APIs that make
/// the folders do not. A malformed prefix fails every long copy, and only long ones, so a suite of
/// short temp paths never notices.
#[test]
fn the_real_copier_copies_into_a_path_longer_than_max_path() {
    let f = Fixture::new();
    let source = f.file("deep", &["src", "a.txt"]);
    let destination = f.deep_path("a.txt");

    FileSystemFileCopier
        .copy(&source, &destination, None, &CancelToken::none())
        .unwrap();

    assert_content(&destination, "deep");
}

#[test]
fn the_real_copier_moves_into_a_path_longer_than_max_path() {
    let f = Fixture::new();
    let source = f.file("deep", &["src", "b.txt"]);
    let destination = f.deep_path("b.txt");

    FileSystemFileCopier
        .move_file(&source, &destination, None, &CancelToken::none())
        .unwrap();

    assert_content(&destination, "deep");
    assert!(!file_exists(&source));
}

#[test]
fn the_real_copier_refuses_to_overwrite() {
    let f = Fixture::new();
    let source = f.file("new", &["src", "a.txt"]);
    let destination = f.file("existing", &["dest", "a.txt"]);

    let result = FileSystemFileCopier.copy(&source, &destination, None, &CancelToken::none());

    assert!(matches!(result, Err(TransferError::Io(_))));
    assert_content(&destination, "existing");
}

// --- copies ---

#[test]
fn copy_directory_leaves_the_source_in_place() {
    let f = Fixture::new();
    f.file("payload", &["src", "tree", "sub", "x.txt"]);
    let dest = f.dir(&["dest"]);

    f.run(
        &[&f.p(&["src", "tree"])],
        &dest,
        TransferVerb::Copy,
        ConflictResolution::KeepBoth,
    );

    assert_content(&f.p(&["dest", "tree", "sub", "x.txt"]), "payload");
    assert_content(&f.p(&["src", "tree", "sub", "x.txt"]), "payload");
}

// --- reparse points ---

#[test]
fn copying_a_tree_containing_a_junction_is_refused_rather_than_silently_dropping_it() {
    let f = Fixture::new();
    f.file("real", &["target", "inner.txt"]);
    let tree = f.dir(&["src", "tree"]);
    create_directory_link(&format!("{tree}\\link"), &f.p(&["target"]));

    let dest = f.dir(&["dest"]);
    let outcome = f.run(
        &[&tree],
        &dest,
        TransferVerb::Copy,
        ConflictResolution::KeepBoth,
    );

    assert_eq!(1, outcome.failed.len());
    assert!(
        outcome.failed[0]
            .message
            .to_lowercase()
            .contains("junction"),
        "{}",
        outcome.failed[0].message
    );
    assert_content(&f.p(&["target", "inner.txt"]), "real"); // link target untouched
}

#[test]
fn moving_a_tree_containing_a_junction_within_a_volume_keeps_the_junction() {
    // A same-volume move is a rename, so the junction rides along and must be preserved.
    let f = Fixture::new();
    f.file("real", &["target", "inner.txt"]);
    let tree = f.dir(&["src", "tree"]);
    create_directory_link(&format!("{tree}\\link"), &f.p(&["target"]));

    let dest = f.dir(&["dest"]);
    let outcome = f.mv(&[&tree], &dest);

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    let moved = f.p(&["dest", "tree", "link"]);
    assert!(dir_exists(&moved));
    assert_ne!(
        0,
        std::fs::symlink_metadata(&moved).unwrap().file_attributes() & REPARSE_POINT
    );
    assert_content(&format!("{moved}\\inner.txt"), "real");
}

// --- a merged move empties its source, and an undo puts it back ---

/// The real end-to-end shape: a folder merged into a folder of the same name, planned by the
/// expander against real files, run, and then undone. A whole-folder move removes the source
/// folders implicitly, so expanding the move must leave the source looking the same way, or a
/// merge reads as a half-failed move.
#[test]
fn a_merged_move_leaves_no_empty_source_folders_behind() {
    let f = Fixture::new();
    f.file("new", &["src", "photos", "2024", "a.txt"]);
    f.file("new", &["src", "photos", "fresh.txt"]);
    f.file("old", &["dest", "photos", "2024", "a.txt"]);

    let outcome = f.run_merged(
        &f.p(&["src", "photos"]),
        &f.dir(&["dest"]),
        ConflictResolution::Replace,
        TransferVerb::Move,
    );

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_content(&f.p(&["dest", "photos", "2024", "a.txt"]), "new");
    assert_content(&f.p(&["dest", "photos", "fresh.txt"]), "new");
    assert!(
        !dir_exists(&f.p(&["src", "photos"])),
        "the merged source folder should be gone"
    );
    assert!(!dir_exists(&f.p(&["src", "photos", "2024"])));
}

#[test]
fn a_merged_move_leaves_a_source_folder_that_still_holds_a_skipped_file() {
    let f = Fixture::new();
    f.file("new", &["src", "photos", "2024", "a.txt"]);
    f.file("old", &["dest", "photos", "2024", "a.txt"]);

    let outcome = f.run_merged(
        &f.p(&["src", "photos"]),
        &f.dir(&["dest"]),
        ConflictResolution::Skip,
        TransferVerb::Move,
    );

    assert!(outcome.failed.is_empty());
    assert_content(&f.p(&["dest", "photos", "2024", "a.txt"]), "old");
    assert_content(&f.p(&["src", "photos", "2024", "a.txt"]), "new");
    assert!(
        dir_exists(&f.p(&["src", "photos", "2024"])),
        "a folder that still holds a file stays"
    );
    assert!(outcome.pruned_directories.is_empty());
}

#[test]
fn a_merged_move_then_undo_puts_the_whole_tree_back() {
    let f = Fixture::new();
    f.file("new", &["src", "photos", "2024", "a.txt"]);
    f.file("new", &["src", "photos", "fresh.txt"]);
    f.file("old", &["dest", "photos", "2024", "a.txt"]);

    let outcome = f.run_merged(
        &f.p(&["src", "photos"]),
        &f.dir(&["dest"]),
        ConflictResolution::Replace,
        TransferVerb::Move,
    );
    assert!(!outcome.pruned_directories.is_empty());

    let undone = f.executor.undo(&outcome, &CancelToken::none());

    assert!(undone.failed.is_empty(), "{:?}", undone.failed);
    assert_content(&f.p(&["src", "photos", "2024", "a.txt"]), "new");
    assert_content(&f.p(&["src", "photos", "fresh.txt"]), "new");
    assert_content(&f.p(&["dest", "photos", "2024", "a.txt"]), "old"); // the displaced file came back
}

#[test]
fn a_copy_never_prunes() {
    let f = Fixture::new();
    f.file("new", &["src", "photos", "a.txt"]);
    f.dir(&["dest", "photos"]);

    let outcome = f.run_merged(
        &f.p(&["src", "photos"]),
        &f.p(&["dest"]),
        ConflictResolution::KeepBoth,
        TransferVerb::Copy,
    );

    assert!(outcome.pruned_directories.is_empty());
    assert!(dir_exists(&f.p(&["src", "photos"])));
}

#[test]
fn pruning_never_removes_a_junction_that_looks_empty() {
    let f = Fixture::new();
    f.dir(&["target"]);
    f.dir(&["src", "photos"]);
    create_directory_link(&f.p(&["src", "photos", "link"]), &f.p(&["target"]));
    f.dir(&["dest", "photos"]);

    let outcome = f.run_merged(
        &f.p(&["src", "photos"]),
        &f.p(&["dest"]),
        ConflictResolution::KeepBoth,
        TransferVerb::Move,
    );

    // The link moved as an item of its own; nothing pruned it as an empty folder on the way.
    assert!(!outcome.pruned_directories.contains(&f.p(&["target"])));
    assert!(
        dir_exists(&f.p(&["target"])),
        "the link's target must survive"
    );
}

/// A merged paste may Replace, through `Overwrite`, and what it displaced is staged rather than
/// destroyed, so `undo_copies` can put both sides back.
#[test]
fn undo_copies_on_a_merged_paste_removes_what_it_wrote_and_restores_what_it_displaced() {
    let f = Fixture::new();
    f.file("new", &["src", "photos", "a.txt"]);
    f.file("new", &["src", "photos", "fresh.txt"]);
    f.file("old", &["dest", "photos", "a.txt"]);

    let outcome = f.run_merged(
        &f.p(&["src", "photos"]),
        &f.p(&["dest"]),
        ConflictResolution::Overwrite,
        TransferVerb::Copy,
    );

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_content(&f.p(&["dest", "photos", "a.txt"]), "new");
    assert!(
        outcome
            .completed
            .iter()
            .any(|c| c.displaced_stage_path.is_some()),
        "a Replace stages"
    );

    let undone = f.executor.undo_copies(&outcome, &CancelToken::none());

    assert!(undone.failed.is_empty(), "{:?}", undone.failed);
    assert_content(&f.p(&["dest", "photos", "a.txt"]), "old");
    assert!(
        !file_exists(&f.p(&["dest", "photos", "fresh.txt"])),
        "what the paste added is gone"
    );
    assert_content(&f.p(&["src", "photos", "a.txt"]), "new"); // a copy never touched the source
}

/// A cross-volume move of a linked folder must be refused before it starts. Asked of the rule
/// directly rather than through a move, because reaching the cross-volume path needs a genuine
/// second volume, and the thing being guarded is that walking through the link copies the
/// target's contents and then deletes the link.
#[test]
fn a_folder_that_is_itself_a_junction_is_refused_across_volumes() {
    let f = Fixture::new();
    f.file("real", &["target", "inner.txt"]);
    let link = f.p(&["src", "link"]);
    f.dir(&["src"]);
    create_directory_link(&link, &f.p(&["target"]));

    let refusal = TransferExecutor::cross_volume_refusal(&link);

    let refusal = refusal.expect("refused");
    assert!(refusal.to_lowercase().contains("junction"), "{refusal}");
    assert_content(&f.p(&["target", "inner.txt"]), "real");
}

#[test]
fn a_folder_containing_a_junction_is_still_refused_across_volumes() {
    let f = Fixture::new();
    f.file("real", &["target", "inner.txt"]);
    let tree = f.dir(&["src", "tree"]);
    create_directory_link(&format!("{tree}\\link"), &f.p(&["target"]));

    assert!(TransferExecutor::cross_volume_refusal(&tree).is_some());
}

#[test]
fn an_ordinary_folder_has_nothing_standing_in_the_way_of_a_cross_volume_move() {
    let f = Fixture::new();
    f.file("payload", &["src", "tree", "sub", "x.txt"]);

    assert_eq!(
        None,
        TransferExecutor::cross_volume_refusal(&f.p(&["src", "tree"]))
    );
}

// --- a mixed answer, one item at a time ---

/// The executor has taken a per-path map since it was written, but every test above builds a
/// uniform one through `run`, so until the dialog could ask per item, nothing exercised three
/// different answers inside one plan. This is that.
#[test]
fn one_plan_can_carry_a_different_answer_for_every_clash() {
    let f = Fixture::new();
    let skipped = f.file("new a", &["src", "a.txt"]);
    let replaced = f.file("new b", &["src", "b.txt"]);
    let kept = f.file("new c", &["src", "c.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("old a", &["dest", "a.txt"]);
    f.file("old b", &["dest", "b.txt"]);
    f.file("old c", &["dest", "c.txt"]);

    let plan = f.plan(&[&skipped, &replaced, &kept], &dest, TransferVerb::Move);
    assert_eq!(3, plan.conflicts().len());

    let resolutions: Resolutions = HashMap::from([
        (canon(&skipped), ConflictResolution::Skip),
        (canon(&replaced), ConflictResolution::Replace),
        (canon(&kept), ConflictResolution::KeepBoth),
    ]);
    let outcome = f.execute(&plan, Some(&resolutions));

    assert!(outcome.failed.is_empty());

    // Skip: neither side moved.
    assert_content(&f.p(&["dest", "a.txt"]), "old a");
    assert_content(&skipped, "new a");
    assert_eq!(vec![skipped.clone()], outcome.skipped);

    // Replace: the incoming copy took the name, the displaced one went to staging.
    assert_content(&f.p(&["dest", "b.txt"]), "new b");
    assert!(!file_exists(&replaced));
    let staged = TransferExecutor::staged_items(&outcome);
    assert_eq!(1, staged.len());
    assert_content(&staged[0], "old b");

    // Keep both: the existing entry was never touched.
    assert_content(&f.p(&["dest", "c.txt"]), "old c");
    assert_content(&f.p(&["dest", "c (2).txt"]), "new c");
    assert!(!file_exists(&kept));
}

/// Anything the map does not mention still falls back to the one resolution that can destroy
/// nothing, which is what makes a dialog listing only the clashes a complete answer.
#[test]
fn answers_are_needed_only_for_the_items_that_clash() {
    let f = Fixture::new();
    let clashing = f.file("new", &["src", "a.txt"]);
    let free = f.file("fresh", &["src", "z.txt"]);
    let dest = f.dir(&["dest"]);
    f.file("existing", &["dest", "a.txt"]);

    let plan = f.plan(&[&clashing, &free], &dest, TransferVerb::Move);

    let resolutions: Resolutions = HashMap::from([(canon(&clashing), ConflictResolution::Replace)]);
    let outcome = f.execute(&plan, Some(&resolutions));

    assert!(outcome.failed.is_empty());
    assert_content(&f.p(&["dest", "a.txt"]), "new");
    assert_content(&f.p(&["dest", "z.txt"]), "fresh");
}

// --- pausing ---

/// How long something that should happen is given before it is called a hang.
const PATIENCE: Duration = Duration::from_secs(10);

/// Long enough that a run which was going to proceed would have.
const LONG_ENOUGH_TO_NOTICE: Duration = Duration::from_millis(200);

fn finished<T>(task: &JoinHandle<T>, within: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < within {
        if task.is_finished() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    task.is_finished()
}

fn wait_for(flag: &AtomicBool, within: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < within {
        if flag.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    flag.load(Ordering::SeqCst)
}

/// A copier that pauses the gate once, part-way through the first file it writes.
fn pausing_copier(gate: &Arc<PauseGate>, paused: &Arc<AtomicBool>) -> SteppedCopier {
    let gate = gate.clone();
    let paused = paused.clone();
    SteppedCopier::new(8, move |_, _| {
        if paused.load(Ordering::SeqCst) {
            return;
        }
        gate.pause();
        paused.store(true, Ordering::SeqCst);
    })
}

/// A pause lands *inside* a file, not merely between two of them: the gate is waited on in the
/// per-chunk progress callback, which for the real copier is a callback `CopyFileExW` is sitting
/// in.
#[test]
fn pausing_holds_a_run_part_way_through_a_file_and_resuming_finishes_it() {
    let f = Fixture::new();
    let content = "x".repeat(8192);
    let source = f.file(&content, &["src", "big.txt"]);
    let dest = f.dir(&["dest"]);

    let gate = Arc::new(PauseGate::new());
    let paused = Arc::new(AtomicBool::new(false));
    let executor = Arc::new(with_copier(pausing_copier(&gate, &paused)));

    let plan = f.plan(&[&source], &dest, TransferVerb::Move);
    let run = {
        let executor = executor.clone();
        let gate = gate.clone();
        std::thread::spawn(move || {
            executor.execute(&plan, None, &CancelToken::none(), None, Some(&gate))
        })
    };

    assert!(wait_for(&paused, PATIENCE));

    // The proof it is really held: given time to finish, it does not.
    assert!(!finished(&run, LONG_ENOUGH_TO_NOTICE));
    assert!(gate.is_paused());

    // And the proof it stopped *inside* the file rather than before it: a partial destination is
    // sitting there with the copy still holding it open. That is what a mid-file pause is, and it
    // is why a paused run keeps every other write in the app blocked behind it.
    assert!(file_exists(&f.p(&["dest", "big.txt"])));
    assert_content(&source, &content);

    gate.resume();

    assert!(finished(&run, PATIENCE));
    let outcome = run.join().unwrap();
    assert!(outcome.failed.is_empty());
    assert!(!outcome.cancelled);
    assert_content(&f.p(&["dest", "big.txt"]), &content);
    assert!(!file_exists(&source));
}

/// Cancelling a paused run stops it without anyone resuming the gate first. The App does resume
/// it as well, so the *next* job does not start held, but a run that could only be stopped by
/// first letting it go would be a deadlock waiting for a user who has already pressed Cancel.
#[test]
fn cancelling_a_paused_run_stops_it_without_being_resumed() {
    let f = Fixture::new();
    let content = "x".repeat(8192);
    let source = f.file(&content, &["src", "big.txt"]);
    let dest = f.dir(&["dest"]);

    let gate = Arc::new(PauseGate::new());
    let cancellation = CancelToken::new();
    let paused = Arc::new(AtomicBool::new(false));
    let executor = Arc::new(with_copier(pausing_copier(&gate, &paused)));

    let plan = f.plan(&[&source], &dest, TransferVerb::Move);
    let run = {
        let executor = executor.clone();
        let gate = gate.clone();
        let token = cancellation.clone();
        std::thread::spawn(move || executor.execute(&plan, None, &token, None, Some(&gate)))
    };

    assert!(wait_for(&paused, PATIENCE));
    assert!(!finished(&run, LONG_ENOUGH_TO_NOTICE));

    cancellation.cancel();

    assert!(finished(&run, PATIENCE));
    let outcome = run.join().unwrap();
    assert!(outcome.cancelled);
    assert!(outcome.completed.is_empty());
    assert!(!file_exists(&f.p(&["dest", "big.txt"])));
    assert_content(&source, &content);
}

/// A gate nobody ever pauses costs a run nothing and changes nothing.
#[test]
fn a_gate_that_is_never_paused_leaves_a_transfer_exactly_as_it_was() {
    let f = Fixture::new();
    let source = f.file("hello", &["src", "a.txt"]);
    let dest = f.dir(&["dest"]);
    let gate = PauseGate::new();

    let plan = f.plan(&[&source], &dest, TransferVerb::Move);
    let outcome = f
        .executor
        .execute(&plan, None, &CancelToken::none(), None, Some(&gate));

    assert!(outcome.failed.is_empty());
    assert_content(&f.p(&["dest", "a.txt"]), "hello");
}
