// Port of tests/BertBrowser.Core.Tests/UndoRecordTests.cs
//
// Every kind of record taken undo, redo, undo against real folders. Each test asserts on file
// *contents*, following transfer_executor_tests: a redo that restores the right names over the
// wrong bytes is exactly the failure that would otherwise pass.
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bb_core::FileTime;
use bb_core::cancel::CancelToken;
use bb_core::compare::{SyncPlans, SyncRunner};
use bb_core::delete::{
    DeleteDisposition, DeleteExecutor, DeleteMode, DeletePlan, FileSystemDeleteProbe, PlannedDelete,
};
use bb_core::rename::{PlannedRename, RenameExecutor, RenamePlan};
use bb_core::transfer::{
    ConflictResolution, Resolutions, TransferExecutor, TransferMergeExpander, TransferOutcome,
    TransferPlanner, TransferVerb,
};
use bb_core::undo::{
    DeleteRecord, HeldEntry, HeldFolderSizes, HeldSize, HeldSizeEstimator, PlainUndoHost,
    RenameRecord, StepVerdict, SyncRecord, TransferRecord, UndoBudget, UndoStack, UndoableRecord,
};
use bb_paths::PathKey;
use bb_test_support::unique_temp_path;

use crate::delete_executor_tests::FakeRecycleBin;

struct Fixture {
    root: PathBuf,
    planner: TransferPlanner,
    transfers: TransferExecutor,
    deletes: DeleteExecutor<'static>,
    renames: RenameExecutor,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-undo", "");
        std::fs::create_dir_all(&root).unwrap();
        let deletes = DeleteExecutor::new(&FileSystemDeleteProbe)
            .protecting(&[])
            .staging_root(&root.to_string_lossy());
        Fixture {
            root,
            planner: TransferPlanner::default(),
            transfers: TransferExecutor::default(),
            deletes,
            renames: RenameExecutor,
        }
    }

    // --- helpers ---

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

    fn host(&self) -> PlainUndoHost<'_> {
        PlainUndoHost::without_archives(&self.transfers, &self.deletes, &self.renames)
    }

    fn transfer(
        &self,
        verb: TransferVerb,
        destination: &str,
        resolution: ConflictResolution,
        sources: &[&str],
    ) -> TransferOutcome {
        let sources: Vec<String> = sources.iter().map(|s| s.to_string()).collect();
        let plan = self.planner.plan(&sources, destination, verb);
        let resolutions: Resolutions = plan
            .transfers
            .iter()
            .map(|t| (PathKey::canonicalize(&t.source_path).unwrap(), resolution))
            .collect();
        self.transfers
            .execute(&plan, Some(&resolutions), &CancelToken::none(), None, None)
    }

    fn moved(&self, resolution: ConflictResolution, sources: &[&str]) -> TransferRecord {
        TransferRecord::for_outcome(self.transfer(
            TransferVerb::Move,
            &self.p(&["dst"]),
            resolution,
            sources,
        ))
        .unwrap()
    }

    fn paste(&self, resolution: ConflictResolution, sources: &[&str]) -> TransferRecord {
        let mut outcome = self.transfer(TransferVerb::Copy, &self.p(&["dst"]), resolution, sources);
        outcome.can_undo_copy = true;
        TransferRecord::for_outcome(outcome).unwrap()
    }

    /// Takes one step and checks it was clean.
    fn step(&self, record: &dyn UndoableRecord) -> Box<dyn UndoableRecord> {
        let result = record.step(&self.host());
        assert!(
            result.report.clean(),
            "{}",
            result.report.failures.join("; ")
        );
        assert_ne!(record.is_applied(), result.next.is_applied());
        result.next
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

fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

fn set_modified(path: &str, when: SystemTime) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

// --- move ---

#[test]
fn a_move_undo_redo_undo_ends_where_it_started() {
    let f = Fixture::new();
    f.file("a", &["src", "a.txt"]);
    f.file("b", &["src", "sub", "b.txt"]);
    f.dir(&["dst"]);

    let record = f.moved(
        ConflictResolution::KeepBoth,
        &[&f.p(&["src", "a.txt"]), &f.p(&["src", "sub"])],
    );

    let undone = f.step(&record);
    assert_content(&f.p(&["src", "a.txt"]), "a");
    assert_content(&f.p(&["src", "sub", "b.txt"]), "b");

    let redone = f.step(undone.as_ref());
    assert!(!exists(&f.p(&["src", "a.txt"])));
    assert_content(&f.p(&["dst", "a.txt"]), "a");
    assert_content(&f.p(&["dst", "sub", "b.txt"]), "b");

    f.step(redone.as_ref());
    assert_content(&f.p(&["src", "a.txt"]), "a");
    assert!(!exists(&f.p(&["dst", "sub"])));
}

/// A Replace is redone as a Replace: the entry the undo put back is set aside again, not numbered
/// past, and comes back once more on the second undo.
#[test]
fn a_replacing_move_replaces_again_on_redo_and_restores_again_on_undo() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.file("old", &["dst", "a.txt"]);

    let record = f.moved(ConflictResolution::Replace, &[&f.p(&["src", "a.txt"])]);
    assert_eq!(1, record.held().len());

    let undone = f.step(&record);
    assert_content(&f.p(&["dst", "a.txt"]), "old");
    assert_content(&f.p(&["src", "a.txt"]), "new");
    assert!(undone.held().is_empty());

    let redone = f.step(undone.as_ref());
    assert_content(&f.p(&["dst", "a.txt"]), "new");
    assert_eq!(1, redone.held().len());
    assert!(!exists(&f.p(&["dst", "a (2).txt"])));

    f.step(redone.as_ref());
    assert_content(&f.p(&["dst", "a.txt"]), "old");
    assert_content(&f.p(&["src", "a.txt"]), "new");
}

/// A Keep-both lands under the same number again, not a fresh one.
#[test]
fn a_keep_both_move_redoes_to_the_same_name() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.file("old", &["dst", "a.txt"]);

    let record = f.moved(ConflictResolution::KeepBoth, &[&f.p(&["src", "a.txt"])]);
    assert_content(&f.p(&["dst", "a (2).txt"]), "new");

    let undone = f.step(&record);
    f.step(undone.as_ref());

    assert_content(&f.p(&["dst", "a (2).txt"]), "new");
    assert_content(&f.p(&["dst", "a.txt"]), "old");
    assert!(!exists(&f.p(&["dst", "a (3).txt"])));
}

/// A name taken since the undo is nobody's choice to replace: the redo leaves it, reports it, and
/// carries on with the rest.
#[test]
fn a_redo_leaves_a_newcomer_alone_and_reports_it() {
    let f = Fixture::new();
    f.file("a", &["src", "a.txt"]);
    f.file("b", &["src", "b.txt"]);
    f.dir(&["dst"]);

    let record = f.moved(
        ConflictResolution::KeepBoth,
        &[&f.p(&["src", "a.txt"]), &f.p(&["src", "b.txt"])],
    );
    let undone = f.step(&record);
    f.file("newcomer", &["dst", "a.txt"]);

    let result = undone.step(&f.host());

    assert_eq!(StepVerdict::Partial, result.report.verdict());
    assert_content(&f.p(&["dst", "a.txt"]), "newcomer");
    assert_content(&f.p(&["src", "a.txt"]), "a");
    assert_content(&f.p(&["dst", "b.txt"]), "b");
    assert_eq!(1, result.next.item_count());
}

/// After a partial undo, the redo acts on what went back and nothing else: the item that could
/// not be put back is not moved a second time.
#[test]
fn a_partial_undo_is_redone_only_for_what_went_back() {
    let f = Fixture::new();
    f.file("a", &["src", "a.txt"]);
    f.file("b", &["src", "b.txt"]);
    f.dir(&["dst"]);

    let record = f.moved(
        ConflictResolution::KeepBoth,
        &[&f.p(&["src", "a.txt"]), &f.p(&["src", "b.txt"])],
    );
    f.file("in the way", &["src", "a.txt"]);

    let result = record.step(&f.host());
    assert_eq!(StepVerdict::Partial, result.report.verdict());
    assert_eq!(1, result.next.item_count());

    f.step(result.next.as_ref());
    assert_content(&f.p(&["dst", "a.txt"]), "a");
    assert_content(&f.p(&["dst", "b.txt"]), "b");
    assert_content(&f.p(&["src", "a.txt"]), "in the way");
}

#[test]
fn a_merged_move_recreates_and_prunes_its_folders_each_way() {
    let f = Fixture::new();
    f.file("a", &["src", "Photos", "a.jpg"]);
    f.file("old", &["dst", "Photos", "b.jpg"]);
    f.file("b", &["src", "Photos", "b.jpg"]);

    let plan = TransferMergeExpander::default()
        .expand(
            f.planner.plan(
                &[f.p(&["src", "Photos"])],
                &f.p(&["dst"]),
                TransferVerb::Move,
            ),
            &CancelToken::none(),
        )
        .unwrap();
    let resolutions: Resolutions = plan
        .transfers
        .iter()
        .map(|t| {
            (
                PathKey::canonicalize(&t.source_path).unwrap(),
                ConflictResolution::Replace,
            )
        })
        .collect();
    let record = TransferRecord::for_outcome(f.transfers.execute(
        &plan,
        Some(&resolutions),
        &CancelToken::none(),
        None,
        None,
    ))
    .unwrap();
    assert!(!exists(&f.p(&["src", "Photos"])));

    let undone = f.step(&record);
    assert_content(&f.p(&["src", "Photos", "a.jpg"]), "a");
    assert_content(&f.p(&["dst", "Photos", "b.jpg"]), "old");

    let redone = f.step(undone.as_ref());
    assert!(!exists(&f.p(&["src", "Photos"])));
    assert_content(&f.p(&["dst", "Photos", "b.jpg"]), "b");

    f.step(redone.as_ref());
    assert_content(&f.p(&["src", "Photos", "b.jpg"]), "b");
    assert_content(&f.p(&["dst", "Photos", "b.jpg"]), "old");
}

// --- paste ---

#[test]
fn an_overwriting_paste_undo_redo_undo_keeps_the_original() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.file("old", &["dst", "a.txt"]);

    let record = f.paste(ConflictResolution::Overwrite, &[&f.p(&["src", "a.txt"])]);
    assert_content(&f.p(&["dst", "a.txt"]), "new");

    let undone = f.step(&record);
    assert_content(&f.p(&["dst", "a.txt"]), "old");

    let redone = f.step(undone.as_ref());
    assert_content(&f.p(&["dst", "a.txt"]), "new");

    f.step(redone.as_ref());
    assert_content(&f.p(&["dst", "a.txt"]), "old");
    assert_content(&f.p(&["src", "a.txt"]), "new");
}

/// The history can reach a paste long after it happened; a copy edited since is the user's work
/// now, and is left in place rather than removed with the paste.
#[test]
fn a_paste_undo_leaves_a_copy_that_was_edited_since() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.dir(&["dst"]);
    let record = f.paste(ConflictResolution::KeepBoth, &[&f.p(&["src", "a.txt"])]);
    std::fs::write(f.p(&["dst", "a.txt"]), "edited afterwards").unwrap();
    set_modified(
        &f.p(&["dst", "a.txt"]),
        SystemTime::now() + Duration::from_secs(5 * 60),
    );

    let result = record.step(&f.host());

    assert_eq!(StepVerdict::Failed, result.report.verdict());
    assert_content(&f.p(&["dst", "a.txt"]), "edited afterwards");
}

// --- undo_copies itself ---

/// The bug the history would otherwise have walked into: undoing the same copy twice deleted the
/// original the first call had just put back. The second call must now touch nothing.
#[test]
fn undo_copies_twice_leaves_the_restored_original_alone() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.file("old", &["dst", "a.txt"]);
    // Same length, and name tunnelling hands the copy the original's creation time, so two writes
    // inside one clock tick would make the restored original pass for the copy.
    set_modified(
        &f.p(&["src", "a.txt"]),
        SystemTime::now() - Duration::from_secs(60 * 60),
    );
    let outcome = f.transfer(
        TransferVerb::Copy,
        &f.p(&["dst"]),
        ConflictResolution::Overwrite,
        &[&f.p(&["src", "a.txt"])],
    );

    let first = f.transfers.undo_copies(&outcome, &CancelToken::none());
    let second = f.transfers.undo_copies(&outcome, &CancelToken::none());

    assert_eq!(1, first.restored);
    assert_eq!(0, second.restored);
    assert_eq!(1, second.failed.len());
    assert_content(&f.p(&["dst", "a.txt"]), "old");
}

#[test]
fn undo_copies_does_not_count_what_was_already_gone() {
    let f = Fixture::new();
    f.file("new", &["src", "a.txt"]);
    f.dir(&["dst"]);
    let outcome = f.transfer(
        TransferVerb::Copy,
        &f.p(&["dst"]),
        ConflictResolution::KeepBoth,
        &[&f.p(&["src", "a.txt"])],
    );
    std::fs::remove_file(f.p(&["dst", "a.txt"])).unwrap();

    let result = f.transfers.undo_copies(&outcome, &CancelToken::none());

    assert_eq!(0, result.restored);
    assert!(result.reverted.is_empty());
}

// --- delete ---

#[test]
fn a_staged_delete_undo_redo_undo_puts_the_tree_back() {
    let f = Fixture::new();
    f.file("a", &["work", "a.txt"]);
    f.file("b", &["work", "sub", "b.txt"]);

    let plan = DeletePlan {
        mode: DeleteMode::Staged,
        deletions: vec![
            PlannedDelete::new(&f.p(&["work", "a.txt"]), false),
            PlannedDelete::new(&f.p(&["work", "sub"]), true),
        ],
        rejected: Vec::new(),
    };
    let record =
        DeleteRecord::for_outcome(f.deletes.execute(&plan, &CancelToken::none(), None), None)
            .unwrap();
    assert_eq!(2, record.held().len());

    let undone = f.step(&record);
    assert_content(&f.p(&["work", "sub", "b.txt"]), "b");

    let redone = f.step(undone.as_ref());
    assert!(!exists(&f.p(&["work", "a.txt"])));
    assert!(!exists(&f.p(&["work", "sub"])));

    f.step(redone.as_ref());
    assert_content(&f.p(&["work", "a.txt"]), "a");
    assert_content(&f.p(&["work", "sub", "b.txt"]), "b");
}

#[test]
fn a_recycled_delete_is_redone_into_the_bin() {
    let f = Fixture::new();
    let bin = FakeRecycleBin::new(PathBuf::from(f.dir(&["bin"])));
    let deletes = DeleteExecutor::new(&FileSystemDeleteProbe)
        .protecting(&[])
        .staging_root(&f.root.to_string_lossy())
        .recycle_bin(&bin)
        .recycle_probe(&bin);
    let host = PlainUndoHost::without_archives(&f.transfers, &deletes, &f.renames);
    f.file("a", &["work", "a.txt"]);

    let plan = DeletePlan {
        mode: DeleteMode::Recycle,
        deletions: vec![
            PlannedDelete::new(&f.p(&["work", "a.txt"]), false)
                .with_disposition(DeleteDisposition::Recycle),
        ],
        rejected: Vec::new(),
    };
    let record =
        DeleteRecord::for_outcome(deletes.execute(&plan, &CancelToken::none(), None), None)
            .unwrap();
    assert!(record.held().is_empty()); // the bin holds it, not us

    let undone = record.step(&host).next;
    assert_content(&f.p(&["work", "a.txt"]), "a");

    let redone = undone.step(&host).next;
    assert!(!exists(&f.p(&["work", "a.txt"])));
    assert!(redone.held().is_empty());

    redone.step(&host);
    assert_content(&f.p(&["work", "a.txt"]), "a");
}

// --- rename ---

#[test]
fn a_rename_undo_redo_undo_tracks_the_names() {
    let f = Fixture::new();
    f.file("a", &["a.txt"]);
    f.file("b", &["b.txt"]);
    let plan = RenamePlan::new(
        vec![
            PlannedRename::new(&f.p(&["a.txt"]), &f.p(&["b.txt"]), false),
            PlannedRename::new(&f.p(&["b.txt"]), &f.p(&["a.txt"]), false),
        ],
        Vec::new(),
    );
    let record = RenameRecord::for_outcome(RenameExecutor.execute(&plan)).unwrap();
    assert_content(&f.p(&["a.txt"]), "b");

    let undone = f.step(&record);
    assert_content(&f.p(&["a.txt"]), "a");
    let details = undone.details();
    let to_b: Vec<_> = details.iter().filter(|d| d.to == f.p(&["b.txt"])).collect();
    assert_eq!(1, to_b.len());
    assert_eq!(f.p(&["a.txt"]), to_b[0].from);

    let redone = f.step(undone.as_ref());
    assert_content(&f.p(&["a.txt"]), "b");

    f.step(redone.as_ref());
    assert_content(&f.p(&["a.txt"]), "a");
    assert_content(&f.p(&["b.txt"]), "b");
}

// --- sync ---

#[test]
fn a_sync_undo_redo_undo_puts_the_right_side_back_each_time() {
    let f = Fixture::new();
    f.file("new", &["left", "a.txt"]);
    f.file("old", &["right", "a.txt"]);
    f.file("extra", &["right", "gone.txt"]);

    let copy = f.planner.plan(
        &[f.p(&["left", "a.txt"])],
        &f.p(&["right"]),
        TransferVerb::Copy,
    );
    let resolutions: Resolutions = copy
        .transfers
        .iter()
        .map(|t| {
            (
                PathKey::canonicalize(&t.source_path).unwrap(),
                ConflictResolution::Overwrite,
            )
        })
        .collect();
    let plans = SyncPlans {
        copies: vec![copy],
        resolutions,
        removals: DeletePlan {
            mode: DeleteMode::Staged,
            deletions: vec![PlannedDelete::new(&f.p(&["right", "gone.txt"]), false)],
            rejected: Vec::new(),
        },
        refused: Vec::new(),
    };
    let record = SyncRecord::for_outcome(SyncRunner::new(&f.transfers, &f.deletes).run(
        &plans,
        &CancelToken::none(),
        None,
    ))
    .unwrap();
    assert_content(&f.p(&["right", "a.txt"]), "new");
    assert!(!exists(&f.p(&["right", "gone.txt"])));

    let undone = f.step(&record);
    assert_content(&f.p(&["right", "a.txt"]), "old");
    assert_content(&f.p(&["right", "gone.txt"]), "extra");

    let redone = f.step(undone.as_ref());
    assert_content(&f.p(&["right", "a.txt"]), "new");
    assert!(!exists(&f.p(&["right", "gone.txt"])));

    f.step(redone.as_ref());
    assert_content(&f.p(&["right", "a.txt"]), "old");
    assert_content(&f.p(&["right", "gone.txt"]), "extra");
}

// --- the history over real records ---

/// Releasing an entry to stay within the budget is what commits its data: the oldest Replace's
/// displaced file goes, the newer one's is still held.
#[test]
fn releasing_an_entry_commits_its_staging_and_no_one_elses() {
    let f = Fixture::new();
    f.file("new1", &["src", "one.txt"]);
    f.file("old1", &["dst", "one.txt"]);
    f.file("new2", &["src", "two.txt"]);
    f.file("old2", &["dst", "two.txt"]);
    let mut stack = UndoStack::new(UndoBudget::new(1, 1 << 40));

    let first = f.moved(ConflictResolution::Replace, &[&f.p(&["src", "one.txt"])]);
    let first_held = first.held();
    stack
        .push(Box::new(first), HeldSize::NONE, FileTime::now())
        .unwrap();
    let second = f.moved(ConflictResolution::Replace, &[&f.p(&["src", "two.txt"])]);
    let second_held = second.held();
    let (_, released) = stack
        .push(Box::new(second), HeldSize::NONE, FileTime::now())
        .unwrap();

    released.retire_all();

    assert_eq!(1, first_held.len());
    assert!(!exists(&first_held[0].held_path));
    assert_eq!(1, second_held.len());
    assert_content(&second_held[0].held_path, "old2");
}

#[test]
fn held_size_counts_files_exactly_and_walks_an_unindexed_folder() {
    let f = Fixture::new();
    f.file("12345", &["held", "a.txt"]);
    f.file("123", &["held", "dir", "b.txt"]);

    let estimator = HeldSizeEstimator::new(None);

    assert_eq!(
        HeldSize::new(5, true),
        estimator.measure(&HeldEntry::new(&f.p(&["held", "a.txt"]), "", false))
    );
    assert_eq!(
        HeldSize::new(8, true),
        estimator.measure(&HeldEntry::new(&f.p(&["held"]), &f.p(&["orig"]), true))
    );
}

#[test]
fn held_size_takes_the_index_answer_for_a_folder_when_there_is_one() {
    let f = Fixture::new();
    f.dir(&["held"]);
    let sizes = FixedSizes(1234);
    let estimator = HeldSizeEstimator::new(Some(&sizes));

    assert_eq!(
        HeldSize::new(1234, true),
        estimator.measure(&HeldEntry::new(&f.p(&["held"]), &f.p(&["orig"]), true))
    );
}

struct FixedSizes(i64);

impl HeldFolderSizes for FixedSizes {
    fn of(&self, _original_path: &str) -> Option<i64> {
        Some(self.0)
    }
}
