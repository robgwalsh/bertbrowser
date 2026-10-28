// Port of tests/BertBrowser.Core.Tests/SyncRunnerTests.cs
//
// A whole sync, on real files: compare two folders, plan it, run it, put it back. Every assertion
// is on file *contents*: a sync that copies the wrong side of a pair passes every test that only
// asks whether a file is there.
//
// The real transfer and delete executors are ported separately, so the runner is driven here
// through `DiskTransfers` / `DiskDeletes`: straightforward on-disk executors that keep the two
// contracts the runner relies on (a displaced entry goes to staging rather than being erased, and
// undo puts back exactly what was recorded). When the real executors land, these two fakes are
// what to swap for them.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use bb_core::cancel::CancelToken;
use bb_core::compare::transfer_stubs::{
    CompletedTransfer, ConflictResolution, DeleteMode, DeleteOutcome, DeletePlan, DeletePlanning,
    DeleteProgress, DeleteSource, DeleteUndoResult, DeletedItem, FailedTransfer, PlannedDelete,
    PlannedTransfer, SyncDeletes, SyncTransfers, TransferOutcome, TransferPlan, TransferPlanning,
    TransferProgress, TransferUndoResult, TransferVerb,
};
use bb_core::compare::{
    CompareAvailability, FolderCompareService, SyncOutcome, SyncPlanner, SyncPreview, SyncRunner,
};
use bb_core::data::FsIndexRepository;
use bb_core::mft::NullMftIndex;
use bb_paths::file_name;
use bb_test_support::{TempDb, unique_temp_path};

struct Fixture {
    root: PathBuf,
    transfers: DiskTransfers,
    deletes: DiskDeletes,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-sync", "");
        std::fs::create_dir_all(&root).unwrap();
        let f = Fixture {
            root,
            transfers: DiskTransfers::default(),
            deletes: DiskDeletes::default(),
        };
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

    fn runner(&self) -> SyncRunner<'_> {
        SyncRunner::new(&self.transfers, &self.deletes)
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
        let plans = SyncPlanner::to_plans(preview, &DiskPlanner, &DiskPlanner, DeleteMode::Staged);
        self.runner().run(&plans, &CancelToken::none(), None)
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
    let staged = DiskTransfers::staged_items(&outcome.copies[0]);
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
    let undo = f.runner().undo(&outcome, &CancelToken::none());

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
    f.runner().undo(&outcome, &CancelToken::none());

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
    f.runner().retire(&outcome);

    assert_content(&f.p(&["right", "a.txt"]), "v2");
    assert!(DiskTransfers::staged_items(&outcome.copies[0]).is_empty());
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

// --- the on-disk doubles ---

/// Plans every source it is handed; the real planners' refusals are their own tests' business.
struct DiskPlanner;

impl TransferPlanning for DiskPlanner {
    fn plan(&self, sources: &[String], destination: &str, verb: TransferVerb) -> TransferPlan {
        let mut plan = TransferPlan::empty(verb, destination);
        for source in sources {
            let destination_path = format!("{destination}\\{}", file_name(source));
            plan.transfers.push(PlannedTransfer {
                source_path: source.clone(),
                is_directory: Path::new(source).is_dir(),
                conflicts: Path::new(&destination_path).exists(),
                destination_path,
            });
        }
        plan
    }
}

impl DeletePlanning for DiskPlanner {
    fn plan(&self, sources: &[DeleteSource], mode: DeleteMode) -> DeletePlan {
        let mut plan = DeletePlan::empty(mode);
        for source in sources {
            plan.deletions.push(PlannedDelete {
                source_path: source.path.clone(),
                is_directory: source.is_directory,
            });
        }
        plan
    }
}

fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

fn remove_any(path: &str) -> std::io::Result<()> {
    if Path::new(path).is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// A copy executor that honours `Overwrite` by moving the displaced entry into a staging folder
/// beside the destination, and whose copy-undo removes what it wrote and puts that entry back.
#[derive(Default)]
struct DiskTransfers {
    staging_counter: AtomicUsize,
}

impl DiskTransfers {
    /// `TransferExecutor.StagedItems`: the displaced entries still sitting in staging.
    fn staged_items(outcome: &TransferOutcome) -> Vec<String> {
        outcome
            .completed
            .iter()
            .filter_map(|c| c.displaced_stage_path.clone())
            .filter(|p| Path::new(p).exists())
            .collect()
    }
}

impl SyncTransfers for DiskTransfers {
    fn execute(
        &self,
        plan: &TransferPlan,
        resolutions: &HashMap<String, ConflictResolution>,
        _cancel: &CancelToken,
        _progress: Option<&dyn Fn(&TransferProgress)>,
    ) -> TransferOutcome {
        let mut outcome = TransferOutcome {
            verb: plan.verb,
            destination_directory: plan.destination_directory.clone(),
            completed: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
            staging_directories: Vec::new(),
            cancelled: false,
        };
        let mut staging: Option<PathBuf> = None;

        for transfer in &plan.transfers {
            let destination = Path::new(&transfer.destination_path);
            let mut displaced = None;
            if destination.exists() {
                let key = bb_paths::PathKey::canonicalize(&transfer.source_path).unwrap();
                if resolutions.get(&key) != Some(&ConflictResolution::Overwrite) {
                    outcome.skipped.push(transfer.source_path.clone());
                    continue;
                }
                let stage = staging.get_or_insert_with(|| {
                    let n = self.staging_counter.fetch_add(1, Ordering::Relaxed);
                    let dir = Path::new(&plan.destination_directory)
                        .join(format!(".bertbrowser-staging-{n}"));
                    std::fs::create_dir_all(&dir).unwrap();
                    outcome
                        .staging_directories
                        .push(dir.to_string_lossy().into_owned());
                    dir
                });
                let staged = stage.join(file_name(&transfer.destination_path));
                std::fs::rename(destination, &staged).unwrap();
                displaced = Some(staged.to_string_lossy().into_owned());
            }

            match copy_recursive(Path::new(&transfer.source_path), destination) {
                Ok(()) => outcome.completed.push(CompletedTransfer {
                    source_path: transfer.source_path.clone(),
                    final_path: transfer.destination_path.clone(),
                    is_directory: transfer.is_directory,
                    displaced_stage_path: displaced,
                }),
                Err(e) => outcome.failed.push(FailedTransfer {
                    source_path: transfer.source_path.clone(),
                    message: e.to_string(),
                    access_denied: false,
                }),
            }
        }

        outcome
    }

    fn undo_copies(&self, outcome: &TransferOutcome, _cancel: &CancelToken) -> TransferUndoResult {
        let mut result = TransferUndoResult {
            restored: 0,
            failed: Vec::new(),
            reverted: Vec::new(),
        };
        if outcome.verb != TransferVerb::Copy {
            result.failed.push(FailedTransfer {
                source_path: String::new(),
                message: "Only a copy can be undone this way.".to_string(),
                access_denied: false,
            });
            return result;
        }

        for completed in outcome.completed.iter().rev() {
            let undone = remove_any(&completed.final_path).and_then(|()| {
                match &completed.displaced_stage_path {
                    Some(staged) => std::fs::rename(staged, &completed.final_path),
                    None => Ok(()),
                }
            });
            match undone {
                Ok(()) => {
                    result.restored += 1;
                    result.reverted.insert(0, completed.clone());
                }
                Err(e) => result.failed.push(FailedTransfer {
                    source_path: completed.source_path.clone(),
                    message: e.to_string(),
                    access_denied: false,
                }),
            }
        }

        for dir in &outcome.staging_directories {
            let _ = std::fs::remove_dir(dir);
        }
        result
    }

    fn commit_staging(&self, outcome: &TransferOutcome) {
        for dir in &outcome.staging_directories {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn redo_plan(
        &self,
        verb: TransferVerb,
        destination: &str,
        completed: &[CompletedTransfer],
    ) -> (TransferPlan, HashMap<String, ConflictResolution>) {
        let mut plan = TransferPlan::empty(verb, destination);
        let mut resolutions = HashMap::new();
        for item in completed {
            plan.transfers.push(PlannedTransfer {
                source_path: item.source_path.clone(),
                is_directory: item.is_directory,
                destination_path: item.final_path.clone(),
                conflicts: item.displaced_stage_path.is_some(),
            });
            resolutions.insert(
                bb_paths::PathKey::canonicalize(&item.source_path).unwrap(),
                if item.displaced_stage_path.is_some() {
                    ConflictResolution::Overwrite
                } else {
                    ConflictResolution::Skip
                },
            );
        }
        (plan, resolutions)
    }
}

/// A delete executor that holds every removed item in a staging folder beside it and moves it
/// back on undo.
#[derive(Default)]
struct DiskDeletes {
    staging_counter: AtomicUsize,
}

impl SyncDeletes for DiskDeletes {
    fn execute(
        &self,
        plan: &DeletePlan,
        _cancel: &CancelToken,
        _progress: Option<&dyn Fn(&DeleteProgress)>,
    ) -> DeleteOutcome {
        let mut outcome = DeleteOutcome::empty(plan.permanent());
        let mut staging: HashMap<String, PathBuf> = HashMap::new();

        for deletion in &plan.deletions {
            let parent = deletion.parent_path();
            let stage = staging.entry(parent.clone()).or_insert_with(|| {
                let n = self.staging_counter.fetch_add(1, Ordering::Relaxed);
                let dir = Path::new(&parent).join(format!(".bertbrowser-deleted-{n}"));
                std::fs::create_dir_all(&dir).unwrap();
                outcome
                    .staging_directories
                    .push(dir.to_string_lossy().into_owned());
                dir
            });
            let staged = stage.join(deletion.name());
            std::fs::rename(&deletion.source_path, &staged).unwrap();
            outcome.deleted.push(DeletedItem {
                source_path: deletion.source_path.clone(),
                is_directory: deletion.is_directory,
                staged_path: Some(staged.to_string_lossy().into_owned()),
                recycled_path: None,
            });
        }

        outcome
    }

    fn undo(&self, outcome: &DeleteOutcome, _cancel: &CancelToken) -> DeleteUndoResult {
        let mut result = DeleteUndoResult {
            restored: 0,
            failed: Vec::new(),
            reverted: Vec::new(),
        };
        for item in &outcome.deleted {
            if let Some(staged) = &item.staged_path {
                std::fs::rename(staged, &item.source_path).unwrap();
                result.restored += 1;
                result.reverted.push(item.clone());
            }
        }
        for dir in &outcome.staging_directories {
            let _ = std::fs::remove_dir(dir);
        }
        result
    }

    fn commit_staging(&self, outcome: &DeleteOutcome) {
        for dir in &outcome.staging_directories {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn redo_plan(&self, reverted: &[DeletedItem]) -> DeletePlan {
        let mut plan = DeletePlan::empty(DeleteMode::Staged);
        for item in reverted {
            plan.deletions.push(PlannedDelete {
                source_path: item.source_path.clone(),
                is_directory: item.is_directory,
            });
        }
        plan
    }
}
