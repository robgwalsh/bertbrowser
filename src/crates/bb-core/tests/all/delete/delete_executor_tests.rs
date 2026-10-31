// Port of tests/BertBrowser.Core.Tests/DeleteExecutorTests.cs. Deletes real files. Every test asserts
// on file *contents* after an undo, not just on existence: a delete that can be reversed into an
// empty shell of the original tree is not reversible at all.
//
// The executor is given the test root as its holding area rather than the root of the machine's
// C: drive — same volume as the temp files, which is what the staged design needs, without a test
// run leaving folders at the root of a real disk.
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::fs::OpenOptions;
use std::os::windows::fs::{FileTimesExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bb_core::attributes::{HIDDEN, READONLY, REPARSE_POINT};
use bb_core::cancel::CancelToken;
use bb_core::delete::{
    DeleteDisposition, DeleteExecutor, DeleteMode, DeleteOutcome, DeletePlanner, DeleteProgress,
    DeleteSource, DeleteUndoResult, DeletedItem, FileSystemDeleteProbe, PlannedDelete, RecycleBin,
    RecycleProbe, RecycleResult, RecycledItem,
};
use bb_test_support::unique_temp_path;

use crate::fs::directory_removal_tests::try_create_junction;

const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-del", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn root(&self) -> String {
        s(&self.root)
    }

    fn p(&self, parts: &[&str]) -> PathBuf {
        let mut p = self.root.clone();
        for part in parts {
            p.push(part);
        }
        p
    }

    fn dir(&self, parts: &[&str]) -> PathBuf {
        let p = self.p(parts);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn file(&self, content: &str, parts: &[&str]) -> PathBuf {
        let p = self.p(parts);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    fn planner(&self) -> DeletePlanner<'static> {
        DeletePlanner::new(&FileSystemDeleteProbe).protecting(&[])
    }

    fn executor(&self) -> DeleteExecutor<'static> {
        DeleteExecutor::new(&FileSystemDeleteProbe)
            .protecting(&[])
            .staging_root(&self.root())
    }

    fn plan(&self, sources: &[PathBuf], mode: DeleteMode) -> bb_core::delete::DeletePlan {
        let sources: Vec<DeleteSource> = sources
            .iter()
            .map(|p| DeleteSource::new(&s(p), p.is_dir()))
            .collect();
        self.planner().plan(&sources, mode)
    }

    fn run(&self, sources: &[PathBuf], mode: DeleteMode) -> DeleteOutcome {
        self.executor()
            .execute(&self.plan(sources, mode), &CancelToken::none(), None)
    }

    fn run_staged(&self, sources: &[PathBuf]) -> DeleteOutcome {
        self.run(sources, DeleteMode::Staged)
    }

    fn undo(&self, outcome: &DeleteOutcome) -> DeleteUndoResult {
        self.executor().undo(outcome, &CancelToken::none())
    }

    // --- the Recycle Bin ---
    //
    // Against a fake bin that really moves the files, so these assert on contents like the rest
    // of the suite. The real one is IFileOperation and cannot be exercised here; what is being
    // pinned down is the executor's own behaviour around it — routing, a mixed batch, and undo.

    fn new_bin(&self, volumeless_paths: &[&PathBuf]) -> FakeRecycleBin {
        let mut bin = FakeRecycleBin::new(self.p(&["fake-bin"]));
        for path in volumeless_paths {
            bin.refuse_volume_for(&s(path));
        }
        bin
    }

    fn run_with_bin(&self, bin: &FakeRecycleBin, sources: &[PathBuf]) -> DeleteOutcome {
        let planner = DeletePlanner::new(&FileSystemDeleteProbe)
            .protecting(&[])
            .recycle_probe(bin);
        let executor = DeleteExecutor::new(&FileSystemDeleteProbe)
            .protecting(&[])
            .staging_root(&self.root())
            .recycle_bin(bin)
            .recycle_probe(bin);
        let sources: Vec<DeleteSource> = sources
            .iter()
            .map(|p| DeleteSource::new(&s(p), p.is_dir()))
            .collect();
        let plan = planner.plan(&sources, DeleteMode::Recycle);
        executor.execute(&plan, &CancelToken::none(), None)
    }

    fn undo_with_bin(&self, bin: &FakeRecycleBin, outcome: &DeleteOutcome) -> DeleteUndoResult {
        DeleteExecutor::new(&FileSystemDeleteProbe)
            .protecting(&[])
            .staging_root(&self.root())
            .recycle_bin(bin)
            .recycle_probe(bin)
            .undo(outcome, &CancelToken::none())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn attributes(path: &str) -> u32 {
    std::fs::symlink_metadata(path).unwrap().file_attributes()
}

fn set_read_only(path: &Path, read_only: bool) {
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(read_only);
    std::fs::set_permissions(path, perms).unwrap();
}

fn assert_content(path: &str, expected: &str) {
    assert!(Path::new(path).is_file(), "expected a file at {path}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), expected);
}

fn single<T>(items: &[T]) -> &T {
    assert_eq!(items.len(), 1, "expected exactly one item");
    &items[0]
}

/// Every file under a tree as "relative path → contents", so a restored tree can be compared with
/// what it was byte for byte rather than by count. Follows links, as `SearchOption.AllDirectories`
/// did.
fn snapshot(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .to_uppercase();
                out.insert(relative, std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    out
}

fn file_names_through_links(root: &Path) -> Vec<String> {
    snapshot(root)
        .keys()
        .map(|k| bb_paths::file_name(k).to_lowercase())
        .collect()
}

// --- the ordinary delete ---

#[test]
fn a_file_leaves_its_folder_but_is_still_held() {
    let f = Fixture::new();
    let file = f.file("hello", &["docs", "a.txt"]);

    let outcome = f.run_staged(std::slice::from_ref(&file));

    assert!(outcome.failed.is_empty());
    assert!(!file.exists());

    let deleted = single(&outcome.deleted);
    assert_content(deleted.staged_path.as_ref().unwrap(), "hello"); // intact until committed
    assert!(outcome.can_undo());
}

#[test]
fn a_folder_goes_whole_and_comes_back_whole() {
    let f = Fixture::new();
    let folder = f.dir(&["tree"]);
    f.file("one", &["tree", "a.txt"]);
    f.file("two", &["tree", "deep", "b.txt"]);
    f.file("three", &["tree", "deep", "deeper", "c.txt"]);
    let before = snapshot(&folder);

    let outcome = f.run_staged(std::slice::from_ref(&folder));
    assert!(!folder.exists());

    let undo = f.undo(&outcome);

    assert!(undo.failed.is_empty());
    assert_eq!(undo.restored, 1);
    assert_eq!(before, snapshot(&folder));
}

#[test]
fn several_items_are_all_deleted_and_all_restored() {
    let f = Fixture::new();
    let a = f.file("a", &["a.txt"]);
    let b = f.file("b", &["b.txt"]);
    let c = f.dir(&["c"]);
    f.file("inside", &["c", "inner.txt"]);

    let outcome = f.run_staged(&[a.clone(), b.clone(), c.clone()]);

    assert_eq!(outcome.deleted.len(), 3);
    assert!(!exists(&s(&a)));
    assert!(!exists(&s(&b)));
    assert!(!exists(&s(&c)));

    f.undo(&outcome);

    assert_content(&s(&a), "a");
    assert_content(&s(&b), "b");
    assert_content(&s(&f.p(&["c", "inner.txt"])), "inside");
}

#[test]
fn two_items_with_the_same_name_are_both_held_without_one_eating_the_other() {
    // A flattened search result can hand over two files that share a name.
    let f = Fixture::new();
    let one = f.file("first", &["x", "same.txt"]);
    let two = f.file("second", &["y", "same.txt"]);

    let outcome = f.run_staged(&[one.clone(), two.clone()]);

    assert!(outcome.failed.is_empty());
    assert_eq!(outcome.deleted.len(), 2);
    assert_ne!(
        outcome.deleted[0].staged_path,
        outcome.deleted[1].staged_path
    );

    f.undo(&outcome);

    assert_content(&s(&one), "first");
    assert_content(&s(&two), "second");
}

#[test]
fn the_holding_folder_is_hidden_so_a_delete_does_not_leave_something_on_screen() {
    let f = Fixture::new();
    let file = f.file("hello", &["a.txt"]);

    let outcome = f.run_staged(&[file]);

    let staged = outcome.deleted[0].staged_path.as_ref().unwrap();
    let batch = Path::new(staged).parent().unwrap();
    assert!(attributes(&s(batch)) & HIDDEN != 0);
    assert!(attributes(&s(batch.parent().unwrap())) & HIDDEN != 0);
}

// --- committing ---

#[test]
fn committing_erases_what_was_held() {
    let f = Fixture::new();
    let folder = f.dir(&["tree"]);
    f.file("one", &["tree", "a.txt"]);

    let outcome = f.run_staged(&[folder]);
    let staged = outcome.deleted[0].staged_path.clone().unwrap();
    assert!(Path::new(&staged).is_dir());

    DeleteExecutor::commit_staging(&outcome);

    assert!(!Path::new(&staged).is_dir());
    for directory in &outcome.staging_directories {
        assert!(!Path::new(directory).is_dir());
    }
}

#[test]
fn committing_takes_the_trash_folder_with_it_when_its_last_batch_goes() {
    let f = Fixture::new();
    let outcome = f.run_staged(&[f.file("hello", &["a.txt"])]);
    let trash = Path::new(&outcome.staging_directories[0])
        .parent()
        .unwrap()
        .to_path_buf();
    assert!(trash.is_dir());

    DeleteExecutor::commit_staging(&outcome);

    assert!(!trash.is_dir());
}

#[test]
fn committing_refuses_anything_that_is_not_a_holding_folder() {
    // The guard that stops a mangled outcome turning CommitStaging into "delete this folder".
    let f = Fixture::new();
    let precious = f.dir(&["precious"]);
    f.file("keep me", &["precious", "a.txt"]);

    DeleteExecutor::commit_staging(&DeleteOutcome {
        permanent: false,
        deleted: vec![],
        failed: vec![],
        staging_directories: vec![s(&precious)],
    });

    assert_content(&s(&f.p(&["precious", "a.txt"])), "keep me");
}

// --- undo ---

#[test]
fn undo_refuses_to_overwrite_whatever_took_the_original_name() {
    let f = Fixture::new();
    let file = f.file("original", &["a.txt"]);
    let outcome = f.run_staged(std::slice::from_ref(&file));

    std::fs::write(&file, "something else entirely").unwrap();

    let undo = f.undo(&outcome);

    assert_eq!(undo.restored, 0);
    assert_eq!(undo.failed.len(), 1);
    assert_content(&s(&file), "something else entirely"); // untouched
    assert_content(outcome.deleted[0].staged_path.as_ref().unwrap(), "original"); // still held
}

#[test]
fn undo_leaves_the_holding_folder_behind_when_it_still_holds_something() {
    let f = Fixture::new();
    let kept = f.file("original", &["kept.txt"]);
    let moved = f.file("moved", &["moved.txt"]);
    let outcome = f.run_staged(&[kept.clone(), moved.clone()]);

    std::fs::write(&kept, "in the way").unwrap();

    let undo = f.undo(&outcome);

    assert_eq!(undo.restored, 1);
    assert_content(&s(&moved), "moved");
    // The one that could not go back must not be swept up with the empty-folder cleanup.
    let held = outcome
        .deleted
        .iter()
        .find(|d| d.source_path == s(&kept))
        .unwrap();
    assert_content(held.staged_path.as_ref().unwrap(), "original");
}

#[test]
fn undo_removes_the_holding_folder_once_everything_is_back() {
    let f = Fixture::new();
    let outcome = f.run_staged(&[f.file("hello", &["a.txt"])]);

    f.undo(&outcome);

    for directory in &outcome.staging_directories {
        assert!(!Path::new(directory).is_dir());
    }
}

#[test]
fn undo_is_refused_for_a_permanent_delete() {
    let f = Fixture::new();
    let outcome = f.run(&[f.file("hello", &["a.txt"])], DeleteMode::Permanent);

    let undo = f.undo(&outcome);

    assert_eq!(undo.restored, 0);
    assert_eq!(undo.failed.len(), 1);
}

// --- permanent delete ---

#[test]
fn a_permanent_delete_erases_in_place_and_holds_nothing() {
    let f = Fixture::new();
    let folder = f.dir(&["tree"]);
    f.file("one", &["tree", "a.txt"]);

    let outcome = f.run(std::slice::from_ref(&folder), DeleteMode::Permanent);

    assert!(outcome.failed.is_empty());
    assert!(!folder.is_dir());
    assert!(outcome.staging_directories.is_empty());
    assert!(outcome.deleted[0].staged_path.is_none());
    assert!(!outcome.can_undo());
}

#[test]
fn a_permanent_delete_gets_past_a_read_only_file_inside_the_tree() {
    let f = Fixture::new();
    let folder = f.dir(&["tree"]);
    let locked = f.file("locked", &["tree", "readonly.txt"]);
    set_read_only(&locked, true);

    let outcome = f.run(std::slice::from_ref(&folder), DeleteMode::Permanent);

    assert!(outcome.failed.is_empty());
    assert!(!folder.is_dir());
}

/// Clearing read-only to get a permanent delete moving must not travel through a junction. The
/// erase itself never did — the tree removal removes a link rather than following it — but the
/// attribute sweep reached the whole of whatever the junction pointed at, and changed files the
/// user had not selected. Swap the hand-rolled walk in `clear_read_only` for one that follows links
/// and this goes red.
#[test]
fn a_permanent_delete_does_not_clear_read_only_through_a_junction() {
    let f = Fixture::new();
    // The bystander: read-only, outside the tree, reachable only through the link.
    let outside = f.dir(&["outside"]);
    let bystander = f.file("keep me", &["outside", "bystander.txt"]);
    set_read_only(&bystander, true);

    // The tree being deleted: a read-only file at the top level, so the first erase attempt
    // fails there and the read-only sweep runs; and a junction one level down, which that attempt
    // therefore never reaches and which is consequently still standing when the sweep walks. Depth
    // is what makes this deterministic — put the junction beside the read-only file and whether it
    // survives to be walked depends on directory enumeration order.
    let folder = f.dir(&["tree"]);
    let locked = f.file("locked", &["tree", "readonly.txt"]);
    set_read_only(&locked, true);
    f.dir(&["tree", "sub"]);
    assert!(
        try_create_junction(&f.p(&["tree", "sub", "link"]), &outside),
        "junction was not created"
    );

    let outcome = f.run(std::slice::from_ref(&folder), DeleteMode::Permanent);

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert!(!folder.is_dir());

    // Nothing beyond the link was touched — neither erased nor quietly made writable.
    assert_content(&s(&bystander), "keep me");
    assert!(
        attributes(&s(&bystander)) & READONLY != 0,
        "the read-only bit was cleared on a file outside the deleted tree"
    );

    // The fixture's teardown is a recursive delete, and a read-only file left behind would make
    // it fail. Clearing it here is exactly what the code under test declined to do.
    set_read_only(&bystander, false);
}

/// A permanent delete of a tree containing a junction has to *work*, and the way it used to fail
/// was the worst shape available: a recursive delete erases everything else in the tree and then
/// fails naming the link, so the contents were gone for good and the user was told the delete had
/// failed. Put that call back in `remove_tree` and this goes red on the very first assertion.
#[test]
fn a_permanent_delete_removes_a_tree_containing_a_junction_and_leaves_its_target_alone() {
    let f = Fixture::new();
    let outside = f.dir(&["outside"]);
    let bystander = f.file("keep me", &["outside", "bystander.txt"]);

    let folder = f.dir(&["tree"]);
    f.file("a", &["tree", "a.txt"]);
    f.file("b", &["tree", "sub", "b.txt"]);
    assert!(
        try_create_junction(&f.p(&["tree", "link"]), &outside),
        "junction was not created"
    );

    let outcome = f.run(std::slice::from_ref(&folder), DeleteMode::Permanent);

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert!(
        !folder.is_dir(),
        "the tree was reported deleted but is still there"
    );

    // The link went as one entry; what it pointed at is untouched.
    assert_content(&s(&bystander), "keep me");
}

/// Meta-test for the two above: with the junction really in place, a walk that *does* follow it
/// reaches the bystander. Without this, the assertions could be passing because the link was never
/// usable rather than because it was declined.
#[test]
fn meta_a_junction_really_does_lead_to_the_bystander() {
    let f = Fixture::new();
    let outside = f.dir(&["outside"]);
    f.file("keep me", &["outside", "bystander.txt"]);
    f.dir(&["tree"]);
    assert!(
        try_create_junction(&f.p(&["tree", "link"]), &outside),
        "junction was not created"
    );
    assert!(attributes(&s(&f.p(&["tree", "link"]))) & REPARSE_POINT != 0);

    let through_the_link = file_names_through_links(&f.p(&["tree"]));

    assert!(through_the_link.contains(&"bystander.txt".to_string()));
}

// --- failures are per item ---

#[test]
fn an_item_that_vanished_between_planning_and_deleting_fails_alone_and_is_reported() {
    let f = Fixture::new();
    let gone = f.file("gone", &["gone.txt"]);
    let kept = f.file("kept", &["kept.txt"]);

    let plan = f.plan(&[gone.clone(), kept.clone()], DeleteMode::Staged);
    std::fs::remove_file(&gone).unwrap(); // disk changes while the confirmation is open

    let outcome = f.executor().execute(&plan, &CancelToken::none(), None);

    assert_eq!(outcome.failed.len(), 1);
    assert_eq!(single(&outcome.deleted).source_path, s(&kept));
    assert!(!kept.exists());
}

#[test]
fn a_protected_location_is_refused_at_the_last_moment_too() {
    // Planned by an executor that does not protect it, run by one that does: the executor must
    // hold the line itself rather than trusting a plan built before the confirmation.
    let f = Fixture::new();
    let folder = f.dir(&["system-ish"]);
    let plan = f.plan(std::slice::from_ref(&folder), DeleteMode::Permanent);

    let guarded = DeleteExecutor::new(&FileSystemDeleteProbe)
        .protecting(&[&s(&folder)])
        .staging_root(&f.root());
    let outcome = guarded.execute(&plan, &CancelToken::none(), None);

    assert_eq!(outcome.failed.len(), 1);
    assert!(folder.is_dir());
}

#[test]
fn cancellation_stops_the_run_without_deleting_anything() {
    let f = Fixture::new();
    let a = f.file("a", &["a.txt"]);
    let b = f.file("b", &["b.txt"]);
    let plan = f.plan(&[a.clone(), b.clone()], DeleteMode::Staged);

    let cancel = CancelToken::new();
    cancel.cancel();
    let outcome = f.executor().execute(&plan, &cancel, None);

    assert!(outcome.deleted.is_empty());
    assert!(outcome.failed.is_empty());
    assert_content(&s(&a), "a");
    assert_content(&s(&b), "b");
}

#[test]
fn progress_reports_every_item_and_then_finishes() {
    let f = Fixture::new();
    let plan = f.plan(
        &[f.file("a", &["a.txt"]), f.file("b", &["b.txt"])],
        DeleteMode::Staged,
    );

    let reports: RefCell<Vec<DeleteProgress>> = RefCell::new(Vec::new());
    f.executor().execute(
        &plan,
        &CancelToken::none(),
        Some(&|p| reports.borrow_mut().push(p)),
    );

    let reports = reports.into_inner();
    assert_eq!(reports.last().unwrap().total, 2);
    assert_eq!(reports.last().unwrap().done, 2);
}

// --- sweeping up after a crash ---

#[test]
fn abandoned_holding_folders_are_swept_up_but_recent_ones_are_left_alone() {
    let f = Fixture::new();
    let outcome = f.run_staged(&[f.file("hello", &["a.txt"])]);
    let batch = outcome.staging_directories[0].clone();
    let roots = [f.root()];

    // A batch from this session belongs to a possibly-still-running instance: leave it.
    DeleteExecutor::purge_abandoned_staging_under(Duration::from_secs(86_400), &roots);
    assert!(Path::new(&batch).is_dir());

    set_creation_time(&batch, SystemTime::now() - Duration::from_secs(2 * 86_400));
    DeleteExecutor::purge_abandoned_staging_under(Duration::from_secs(86_400), &roots);

    assert!(!Path::new(&batch).is_dir());
    assert!(!f.p(&[".bertbrowser-trash"]).is_dir());
}

fn set_creation_time(directory: &str, when: SystemTime) {
    let handle = OpenOptions::new()
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(directory)
        .unwrap();
    handle
        .set_times(std::fs::FileTimes::new().set_created(when))
        .unwrap();
}

// --- what search has to hide ---

#[test]
fn a_staged_item_is_recognised_as_held_so_search_can_hide_it() {
    let f = Fixture::new();
    let outcome = f.run_staged(&[f.file("hello", &["a.txt"])]);

    assert!(DeleteExecutor::is_held_path(
        outcome.deleted[0].staged_path.as_ref().unwrap()
    ));
}

#[test]
fn held_paths_are_the_ones_in_a_holding_folder() {
    for (path, held) in [
        (r"C:\.bertbrowser-trash\delete-abc123\a.txt", true),
        (r"C:\p\.bertbrowser-deleted-abc123\a.txt", true),
        (r"C:\.BERTBROWSER-TRASH\delete-abc123\a.txt", true),
        (r"C:\p\a.txt", false),
        // A folder that merely starts the same way is somebody's own folder, not ours.
        (r"C:\.bertbrowser-trash-notes\a.txt", false),
    ] {
        assert_eq!(held, DeleteExecutor::is_held_path(path), "{path}");
    }
}

// --- meta: the checks above can actually fail ---

#[test]
fn meta_the_snapshot_comparison_notices_a_missing_file() {
    let f = Fixture::new();
    let folder = f.dir(&["tree"]);
    f.file("one", &["tree", "a.txt"]);
    f.file("two", &["tree", "b.txt"]);
    let before = snapshot(&folder);

    std::fs::remove_file(f.p(&["tree", "b.txt"])).unwrap();

    assert_ne!(before, snapshot(&folder));
}

#[test]
fn meta_the_snapshot_comparison_notices_changed_contents() {
    let f = Fixture::new();
    let folder = f.dir(&["tree"]);
    f.file("one", &["tree", "a.txt"]);
    let before = snapshot(&folder);

    std::fs::write(f.p(&["tree", "a.txt"]), "tampered").unwrap();

    assert_ne!(before, snapshot(&folder));
}

// --- what search must not show ---

/// A file the user just deleted turning up in search looks exactly like a delete that did not
/// work — and a recycled one turns up under a name that is not even the one they deleted, as
/// `$RAB1234.txt`. Both are filtered, which is why this is the same predicate for both.
#[test]
fn deleted_items_are_hidden_from_search() {
    for path in [
        r"C:\.bertbrowser-trash\delete-abc123\a.txt",
        r"C:\p\.bertbrowser-deleted-abc123\a.txt",
        r"C:\$Recycle.Bin\S-1-5-21-1\$RAB1234.txt",
        r"C:\$RECYCLE.BIN\S-1-5-21-1\$RAB1234.txt",
    ] {
        assert!(DeleteExecutor::is_held_path(path), "{path}");
    }
}

#[test]
fn ordinary_files_are_not_hidden_from_search() {
    for path in [
        r"C:\p\a.txt",
        r"C:\p\$Recycled\a.txt",
        r"C:\p\bertbrowser-trash\a.txt",
    ] {
        assert!(!DeleteExecutor::is_held_path(path), "{path}");
    }
}

// --- the Recycle Bin ---

#[test]
fn a_recycled_file_leaves_its_folder_and_is_held_by_the_bin() {
    let f = Fixture::new();
    let file = f.file("hello", &["a.txt"]);
    let bin = f.new_bin(&[]);

    let outcome = f.run_with_bin(&bin, std::slice::from_ref(&file));

    assert!(!exists(&s(&file)));
    let item = single(&outcome.deleted);
    assert!(item.recycled_path.is_some());
    assert!(item.staged_path.is_none());
    assert_content(item.recycled_path.as_ref().unwrap(), "hello");
    assert!(outcome.can_undo());
}

/// Nothing is staged for a recycled item, so there is no holding folder to commit — which is the
/// whole structural gain: the data outlives the undo record by as long as the user leaves the bin
/// alone, rather than by exactly one operation.
#[test]
fn a_recycled_delete_creates_no_holding_folder() {
    let f = Fixture::new();
    let bin = f.new_bin(&[]);

    let outcome = f.run_with_bin(&bin, &[f.file("hello", &["a.txt"])]);

    assert!(outcome.staging_directories.is_empty());
    DeleteExecutor::commit_staging(&outcome); // must be a no-op rather than reaching into the bin
    assert_content(outcome.deleted[0].recycled_path.as_ref().unwrap(), "hello");
}

#[test]
fn undoing_a_recycled_delete_puts_the_file_back_with_its_contents() {
    let f = Fixture::new();
    let file = f.file("hello", &["a.txt"]);
    let bin = f.new_bin(&[]);
    let outcome = f.run_with_bin(&bin, std::slice::from_ref(&file));

    let undo = f.undo_with_bin(&bin, &outcome);

    assert_eq!(undo.restored, 1);
    assert!(undo.failed.is_empty());
    assert_content(&s(&file), "hello");
}

#[test]
fn undoing_a_recycled_folder_restores_the_tree_byte_for_byte() {
    let f = Fixture::new();
    let folder = f.dir(&["tree"]);
    f.file("one", &["tree", "a.txt"]);
    f.file("two", &["tree", "nested", "b.txt"]);
    let before = snapshot(&folder);
    let bin = f.new_bin(&[]);

    let outcome = f.run_with_bin(&bin, std::slice::from_ref(&folder));
    assert!(!exists(&s(&folder)));
    let undo = f.undo_with_bin(&bin, &outcome);

    assert_eq!(undo.restored, 1);
    assert_eq!(before, snapshot(&folder));
}

/// The case the whole fallback exists for: one selection spanning a volume with a bin and one
/// without. Both halves must survive, and undo must put both back.
#[test]
fn a_mixed_batch_recycles_what_it_can_and_holds_the_rest() {
    let f = Fixture::new();
    let recyclable = f.file("first", &["a.txt"]);
    let not_recyclable = f.file("second", &["b.txt"]);
    let bin = f.new_bin(&[&not_recyclable]);

    let outcome = f.run_with_bin(&bin, &[recyclable.clone(), not_recyclable.clone()]);

    assert!(!exists(&s(&recyclable)));
    assert!(!exists(&s(&not_recyclable)));

    let recycled = outcome
        .deleted
        .iter()
        .find(|d| d.source_path == s(&recyclable))
        .unwrap();
    let staged = outcome
        .deleted
        .iter()
        .find(|d| d.source_path == s(&not_recyclable))
        .unwrap();
    assert!(recycled.recycled_path.is_some());
    assert!(recycled.staged_path.is_none());
    assert!(staged.staged_path.is_some());
    assert!(staged.recycled_path.is_none());
    assert!(!outcome.staging_directories.is_empty());

    let undo = f.undo_with_bin(&bin, &outcome);

    assert_eq!(undo.restored, 2);
    assert!(undo.failed.is_empty());
    assert_content(&s(&recyclable), "first");
    assert_content(&s(&not_recyclable), "second");
}

/// With no bin wired up at all, a plan that asked to recycle must fall back to the holding folder.
/// The one outcome that would be unforgivable is erasing instead.
#[test]
fn with_no_bin_available_a_recycle_plan_is_held_never_erased() {
    let f = Fixture::new();
    let file = f.file("hello", &["a.txt"]);
    let bin = f.new_bin(&[]);
    let planner = DeletePlanner::new(&FileSystemDeleteProbe)
        .protecting(&[])
        .recycle_probe(&bin);
    let plan = planner.plan(&[DeleteSource::new(&s(&file), false)], DeleteMode::Recycle);
    assert_eq!(plan.deletions[0].disposition, DeleteDisposition::Recycle);

    // Same plan, executed by an executor that has no bin.
    let outcome = f.executor().execute(&plan, &CancelToken::none(), None);

    assert!(!exists(&s(&file)));
    let item = single(&outcome.deleted);
    assert!(item.staged_path.is_some());
    assert_content(item.staged_path.as_ref().unwrap(), "hello");

    assert_eq!(f.undo(&outcome).restored, 1);
    assert_content(&s(&file), "hello");
}

/// The shell erases rather than holds an item too big for the bin. That is not a failure, but
/// there is nothing to undo — and the outcome has to say so rather than offering a Ctrl+Z that
/// cannot work.
#[test]
fn an_item_the_bin_erased_instead_of_holding_is_not_offered_for_undo() {
    let f = Fixture::new();
    let file = f.file("hello", &["a.txt"]);
    let mut bin = f.new_bin(&[]);
    bin.erase_instead_of_holding(&s(&file));

    let outcome = f.run_with_bin(&bin, std::slice::from_ref(&file));

    assert!(!exists(&s(&file)));
    assert!(single(&outcome.deleted).recycled_path.is_none());
    assert!(!outcome.can_undo());
}

#[test]
fn undoing_an_item_the_bin_no_longer_holds_is_reported_rather_than_silent() {
    let f = Fixture::new();
    let file = f.file("hello", &["a.txt"]);
    let bin = f.new_bin(&[]);
    let outcome = f.run_with_bin(&bin, &[file]);

    bin.empty(); // the user emptied the Recycle Bin before pressing Ctrl+Z

    let undo = f.undo_with_bin(&bin, &outcome);

    assert_eq!(undo.restored, 0);
    assert!(single(&undo.failed).message.contains("Recycle Bin"));
}

#[test]
fn undo_will_not_overwrite_something_that_took_the_original_path() {
    let f = Fixture::new();
    let file = f.file("hello", &["a.txt"]);
    let bin = f.new_bin(&[]);
    let outcome = f.run_with_bin(&bin, std::slice::from_ref(&file));

    std::fs::write(&file, "something else entirely").unwrap();

    let undo = f.undo_with_bin(&bin, &outcome);

    assert_eq!(undo.restored, 0);
    assert_eq!(undo.failed.len(), 1);
    assert_content(&s(&file), "something else entirely");
}

/// A stand-in for the Windows Recycle Bin that really moves files, so tests can assert on contents.
/// It models the three behaviours the executor has to cope with: a volume the bin does not serve,
/// an item the shell erases instead of holding, and a bin that has been emptied.
pub struct FakeRecycleBin {
    bin_root: PathBuf,
    no_bin: HashSet<String>,
    erases: HashSet<String>,
    next: Cell<usize>,
}

impl FakeRecycleBin {
    pub fn new(bin_root: PathBuf) -> FakeRecycleBin {
        FakeRecycleBin {
            bin_root,
            no_bin: HashSet::new(),
            erases: HashSet::new(),
            next: Cell::new(0),
        }
    }

    /// Makes `path` look like it lives on a share or on media with the bin turned off.
    pub fn refuse_volume_for(&mut self, path: &str) {
        self.no_bin.insert(path.to_uppercase());
    }

    /// Makes the bin accept `path` but erase it — what the shell does with an item larger than
    /// the bin's quota.
    pub fn erase_instead_of_holding(&mut self, path: &str) {
        self.erases.insert(path.to_uppercase());
    }

    pub fn empty(&self) {
        if self.bin_root.is_dir() {
            std::fs::remove_dir_all(&self.bin_root).unwrap();
        }
    }
}

impl RecycleProbe for FakeRecycleBin {
    fn can_recycle(&self, path: &str) -> bool {
        !self.no_bin.contains(&path.to_uppercase())
    }
}

impl RecycleBin for FakeRecycleBin {
    fn recycle(
        &self,
        items: &[PlannedDelete],
        _cancel: &CancelToken,
        progress: Option<&dyn Fn(DeleteProgress)>,
    ) -> RecycleResult {
        std::fs::create_dir_all(&self.bin_root).unwrap();
        let mut recycled = Vec::new();

        for (done, item) in items.iter().enumerate() {
            if let Some(report) = progress {
                report(DeleteProgress {
                    done,
                    total: items.len(),
                    current_name: item.name().to_string(),
                });
            }

            if self.erases.contains(&item.source_path.to_uppercase()) {
                if item.is_directory {
                    std::fs::remove_dir_all(&item.source_path).unwrap();
                } else {
                    std::fs::remove_file(&item.source_path).unwrap();
                }
                recycled.push(RecycledItem {
                    source_path: item.source_path.clone(),
                    is_directory: item.is_directory,
                    recycled_path: None,
                });
                continue;
            }

            // The real bin renames to $R<id><ext>; the shape matters more than the exact name.
            let n = self.next.get();
            self.next.set(n + 1);
            let name = item.name();
            let extension = name.rfind('.').map_or("", |dot| &name[dot..]);
            let target = self.bin_root.join(format!("$R{n:06}{extension}"));
            std::fs::rename(&item.source_path, &target).unwrap();
            recycled.push(RecycledItem {
                source_path: item.source_path.clone(),
                is_directory: item.is_directory,
                recycled_path: Some(s(&target)),
            });
        }

        RecycleResult {
            recycled,
            failed: Vec::new(),
        }
    }

    fn restore(&self, item: &DeletedItem) -> bool {
        let Some(held) = &item.recycled_path else {
            return false;
        };
        let present = if item.is_directory {
            Path::new(held).is_dir()
        } else {
            Path::new(held).is_file()
        };
        if !present {
            return false;
        }
        std::fs::rename(held, &item.source_path).unwrap();
        true
    }
}
