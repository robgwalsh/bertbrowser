// Port of tests/BertBrowser.Core.Tests/SyncPlannerTests.cs
//
// Turning a comparison into what a sync would do. Pure, and worth pinning here rather than in a
// dialog: this is where a verdict becomes a write, and the one place that decides whether an
// entry nobody could compare is quietly acted on anyway.
use std::collections::HashSet;

use bb_core::FileTime;
use bb_core::compare::transfer_stubs::{
    ConflictResolution, DeleteMode, DeletePlan, DeletePlanning, DeleteSource, PlannedDelete,
    PlannedTransfer, RejectedDelete, RejectedTransfer, TransferPlan, TransferPlanning,
    TransferVerb,
};
use bb_core::compare::{
    CompareAvailability, CompareEntry, CompareSourceKind, CompareTolerance, FolderCompareOutcome,
    FolderComparer, SyncAction, SyncActionKind, SyncPlanner, SyncPreview,
};
use bb_paths::{file_name, fold_upper};

const LEFT: &str = r"C:\Work\left";
const RIGHT: &str = r"C:\Work\right";

fn noon() -> FileTime {
    FileTime::from_ymd_hms(2026, 3, 14, 12, 0, 0)
}

fn file(display: &str) -> CompareEntry {
    file_sized(display, 10)
}

fn file_sized(display: &str, size: i64) -> CompareEntry {
    CompareEntry::new(fold_upper(display), file_name(display), false, size, noon())
}

fn file_modified(display: &str, modified: FileTime) -> CompareEntry {
    CompareEntry::new(fold_upper(display), file_name(display), false, 10, modified)
}

fn folder(display: &str) -> CompareEntry {
    CompareEntry::new(fold_upper(display), file_name(display), true, 0, noon())
}

fn compared(left: &[CompareEntry], right: &[CompareEntry]) -> FolderCompareOutcome {
    FolderCompareOutcome {
        left_path: LEFT.to_string(),
        right_path: RIGHT.to_string(),
        result: FolderComparer::compare(left, right, CompareTolerance::STRICT),
        availability: CompareAvailability::Ready,
        left_source: CompareSourceKind::Index,
        right_source: CompareSourceKind::Index,
        truncated: false,
        cancelled: false,
        problem: None,
    }
}

fn preview(left: &[CompareEntry], right: &[CompareEntry]) -> SyncPreview {
    SyncPlanner::preview(&compared(left, right), false)
}

fn preview_removing(left: &[CompareEntry], right: &[CompareEntry]) -> SyncPreview {
    SyncPlanner::preview(&compared(left, right), true)
}

fn action<'a>(preview: &'a SyncPreview, relative_key: &str) -> &'a SyncAction {
    let matching: Vec<&SyncAction> = preview
        .actions
        .iter()
        .filter(|a| a.relative_key == relative_key)
        .collect();
    assert_eq!(1, matching.len(), "one action for {relative_key}");
    matching[0]
}

fn single(preview: &SyncPreview) -> &SyncAction {
    assert_eq!(1, preview.actions.len(), "{:?}", preview.actions);
    &preview.actions[0]
}

fn ticks(keys: &[&str]) -> HashSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

/// Knows about the right side's files, so a deletion is not refused for naming something that
/// (on this machine, in this test) was never there.
fn delete_probe(names: &[&str]) -> FakeDeletePlanner {
    let mut probe = FakeDeletePlanner::default();
    probe.add_directory(RIGHT);
    for name in names {
        probe.add_file(&format!("{RIGHT}\\{name}"));
    }
    probe
}

// --- what becomes an action ---

#[test]
fn two_identical_trees_need_no_sync() {
    let tree = [folder("src"), file(r"src\a.cs")];

    let preview = preview(&tree, &tree);

    assert!(preview.actions.is_empty());
    assert!(!preview.has_work());
}

#[test]
fn a_file_only_on_the_left_is_copied() {
    let preview = preview(&[file("new.txt")], &[]);

    let action = action(&preview, "NEW.TXT");
    assert_eq!(SyncActionKind::Copy, action.kind);
    assert!(action.ticked);
    assert_eq!(r"C:\Work\left\new.txt", action.source_path);
    assert_eq!(r"C:\Work\right\new.txt", action.target_path);
}

#[test]
fn a_file_the_left_has_updated_is_overwritten() {
    let preview = preview(
        &[file_modified("a.txt", noon().add_hours(1))],
        &[file("a.txt")],
    );

    let action = action(&preview, "A.TXT");
    assert_eq!(SyncActionKind::Overwrite, action.kind);
    assert!(action.ticked);
}

/// Still shown, because "make the right side match" is not finished while it holds something
/// else, but not ticked, because overwriting the newer of two files is the one write nobody would
/// expect to have agreed to by asking for a sync.
#[test]
fn overwriting_the_newer_side_is_offered_but_not_ticked() {
    let preview = preview(
        &[file("a.txt")],
        &[file_modified("a.txt", noon().add_hours(1))],
    );

    let action = action(&preview, "A.TXT");
    assert_eq!(SyncActionKind::Overwrite, action.kind);
    assert!(!action.ticked);
    assert!(!preview.has_work());
}

/// The destructive half is opt-in, and off is what the dialog opens with.
#[test]
fn what_only_the_right_has_is_left_alone_unless_asked_for() {
    assert!(preview(&[], &[file("extra.txt")]).actions.is_empty());

    let removing = preview_removing(&[], &[file("extra.txt")]);
    let action = action(&removing, "EXTRA.TXT");
    assert_eq!(SyncActionKind::Delete, action.kind);
    assert_eq!(r"C:\Work\right\extra.txt", action.target_path);
    assert_eq!("", action.source_path);
}

/// The rule the whole safety story rests on. An entry no verdict could be reached for is counted
/// and left alone: never copied, and above all never deleted.
#[test]
fn an_entry_nobody_could_compare_produces_no_action_at_all() {
    let preview = preview_removing(
        &[file_modified("odd.txt", FileTime::UNKNOWN)],
        &[file_modified("odd.txt", FileTime::UNKNOWN)],
    );

    assert!(preview.actions.is_empty());
    assert_eq!(1, preview.unknown_count);
}

// --- folders are whole ---

#[test]
fn a_folder_the_right_side_lacks_is_one_action_covering_everything_in_it() {
    let left = [
        folder("bin"),
        folder(r"bin\deep"),
        file(r"bin\deep\app.exe"),
        file(r"bin\readme.md"),
    ];

    let preview = preview(&left, &[]);

    let action = single(&preview);
    assert_eq!("BIN", action.relative_key);
    assert!(action.is_directory);
}

#[test]
fn a_folder_only_the_right_side_has_is_one_deletion() {
    let right = [folder("old"), file(r"old\a.txt"), file(r"old\b.txt")];

    let preview = preview_removing(&[], &right);
    let action = single(&preview);

    assert_eq!("OLD", action.relative_key);
    assert!(action.is_directory);
}

/// A folder both sides have is opened rather than replaced, or a sync of one changed file would
/// rewrite everything beside it.
#[test]
fn a_folder_both_sides_have_is_descended_into_instead() {
    let left = [
        folder("src"),
        file_sized(r"src\a.cs", 10),
        file(r"src\b.cs"),
    ];
    let right = [
        folder("src"),
        file_sized(r"src\a.cs", 99),
        file(r"src\b.cs"),
    ];

    let preview = preview(&left, &right);
    let action = single(&preview);

    assert_eq!(r"SRC\A.CS", action.relative_key);
    assert!(!action.is_directory);
}

/// The awkward pair: a file where the other side has a folder. One action takes over the name,
/// and nothing inside the folder gets an action of its own.
#[test]
fn a_file_replacing_a_folder_is_still_one_action() {
    let preview = preview_removing(
        &[file("thing")],
        &[folder("thing"), file(r"thing\inner.txt")],
    );

    let action = single(&preview);
    assert_eq!("THING", action.relative_key);
    assert_eq!(SyncActionKind::Overwrite, action.kind);
}

// --- weights ---

#[test]
fn a_folder_weighs_what_is_inside_it() {
    let left = [
        folder("bin"),
        file_sized(r"bin\a", 100),
        file_sized(r"bin\deep\b", 25),
    ];

    assert_eq!(Some(125), action(&preview(&left, &[]), "BIN").bytes);
}

#[test]
fn the_total_counts_only_what_would_be_written() {
    let preview = preview_removing(
        &[file_sized("keep.txt", 40)],
        &[file_sized("gone.txt", 9_000)],
    );

    assert_eq!(Some(40), preview.total_bytes());
    assert_eq!(1, preview.copy_count());
    assert_eq!(1, preview.delete_count());
}

#[test]
fn an_unticked_action_is_not_in_the_total() {
    let preview = preview(&[file_sized("a.txt", 40)], &[]);
    let none = SyncPlanner::with_ticks(&preview, &HashSet::new());

    assert_eq!(Some(0), none.total_bytes());
    assert!(!none.has_work());
}

/// Unticking a folder unticks everything inside it, because everything inside it is that one
/// action.
#[test]
fn unticking_a_folder_removes_everything_under_it() {
    let preview = preview(
        &[folder("bin"), file(r"bin\app.exe"), file("loose.txt")],
        &[],
    );
    let kept = SyncPlanner::with_ticks(&preview, &ticks(&["LOOSE.TXT"]));

    let ticked = kept.ticked();
    assert_eq!(1, ticked.len());
    assert_eq!("LOOSE.TXT", ticked[0].relative_key);
}

// --- a compare that cannot be trusted ---

/// A truncated scan describes a prefix of a folder, not the folder. Read off it, "only on the
/// right" names files the left side has and would have offered to delete them.
#[test]
fn a_truncated_comparison_produces_nothing() {
    let compare = FolderCompareOutcome {
        truncated: true,
        ..compared(&[], &[file("a.txt")])
    };

    assert!(SyncPlanner::preview(&compare, true).actions.is_empty());
}

#[test]
fn a_refused_comparison_produces_nothing() {
    let compare = FolderCompareOutcome::refused(LEFT, RIGHT, "no");

    assert!(SyncPlanner::preview(&compare, true).actions.is_empty());
}

// --- into the executors' own plans ---

#[test]
fn each_destination_folder_gets_its_own_transfer_plan() {
    let mut probe = FakeTransferPlanner::default();
    probe.add_directory(LEFT);
    probe.add_directory(RIGHT);
    probe.add_directory(r"C:\Work\left\src");
    probe.add_directory(r"C:\Work\right\src");
    probe.add_file(r"C:\Work\left\top.txt");
    probe.add_file(r"C:\Work\left\src\deep.txt");

    let left = [folder("src"), file("top.txt"), file(r"src\deep.txt")];
    let right = [folder("src")];

    let plans = SyncPlanner::to_plans(
        &preview(&left, &right),
        &probe,
        &delete_probe(&["gone.txt"]),
        DeleteMode::Recycle,
    );

    assert_eq!(2, plans.copies.len());
    assert_eq!(RIGHT, plans.copies[0].destination_directory);
    assert_eq!(r"C:\Work\right\src", plans.copies[1].destination_directory);
    assert!(plans.copies.iter().all(|p| p.verb == TransferVerb::Copy));
}

/// Every sync copy overwrites, including the ones the comparison found a free name for. A file
/// that appeared on the right in between is a name to take over, not one to sidestep with a
/// silent "x (2)" that leaves the sync looking done.
#[test]
fn every_sync_copy_is_given_the_overwrite_resolution() {
    let mut probe = FakeTransferPlanner::default();
    probe.add_directory(LEFT);
    probe.add_directory(RIGHT);
    probe.add_file(r"C:\Work\left\fresh.txt");

    let plans = SyncPlanner::to_plans(
        &preview(&[file("fresh.txt")], &[]),
        &probe,
        &delete_probe(&["gone.txt"]),
        DeleteMode::Recycle,
    );

    assert_eq!(1, plans.resolutions.len());
    assert_eq!(
        Some(&ConflictResolution::Overwrite),
        plans.resolutions.values().next()
    );
}

#[test]
fn deletions_become_a_delete_plan_in_the_mode_asked() {
    let mut probe = FakeTransferPlanner::default();
    probe.add_directory(LEFT);
    probe.add_directory(RIGHT);

    let plans = SyncPlanner::to_plans(
        &preview_removing(&[], &[file("gone.txt")]),
        &probe,
        &delete_probe(&["gone.txt"]),
        DeleteMode::Recycle,
    );

    assert!(plans.copies.is_empty());
    assert_eq!(DeleteMode::Recycle, plans.removals.mode);
    assert_eq!(1, plans.item_count());
}

/// The transfer planner over an in-memory probe: a source it knows is planned into
/// `destination\name`, one it does not is refused. The real planner's rules are its own tests'
/// business; this keeps only what `to_plans` relies on.
#[derive(Default)]
struct FakeTransferPlanner {
    dirs: HashSet<String>,
    files: HashSet<String>,
}

impl FakeTransferPlanner {
    fn add_directory(&mut self, path: &str) {
        self.dirs.insert(fold_upper(path));
    }
    fn add_file(&mut self, path: &str) {
        self.files.insert(fold_upper(path));
    }
    fn exists(&self, path: &str) -> bool {
        let key = fold_upper(path);
        self.dirs.contains(&key) || self.files.contains(&key)
    }
}

impl TransferPlanning for FakeTransferPlanner {
    fn plan(&self, sources: &[String], destination: &str, verb: TransferVerb) -> TransferPlan {
        let mut plan = TransferPlan::empty(verb, destination);
        for source in sources {
            if !self.exists(source) {
                plan.rejected.push(RejectedTransfer {
                    source_path: source.clone(),
                    message: format!("{source} does not exist"),
                    is_benign: false,
                });
                continue;
            }
            let destination_path = format!("{destination}\\{}", file_name(source));
            plan.transfers.push(PlannedTransfer {
                source_path: source.clone(),
                is_directory: self.dirs.contains(&fold_upper(source)),
                conflicts: self.exists(&destination_path),
                destination_path,
            });
        }
        plan
    }
}

#[derive(Default)]
struct FakeDeletePlanner {
    dirs: HashSet<String>,
    files: HashSet<String>,
}

impl FakeDeletePlanner {
    fn add_directory(&mut self, path: &str) {
        self.dirs.insert(fold_upper(path));
    }
    fn add_file(&mut self, path: &str) {
        self.files.insert(fold_upper(path));
    }
}

impl DeletePlanning for FakeDeletePlanner {
    fn plan(&self, sources: &[DeleteSource], mode: DeleteMode) -> DeletePlan {
        let mut plan = DeletePlan::empty(mode);
        for source in sources {
            let key = fold_upper(&source.path);
            if self.dirs.contains(&key) || self.files.contains(&key) {
                plan.deletions.push(PlannedDelete {
                    source_path: source.path.clone(),
                    is_directory: source.is_directory,
                });
            } else {
                plan.rejected.push(RejectedDelete {
                    source_path: source.path.clone(),
                    message: format!("{} does not exist", source.path),
                    is_benign: false,
                });
            }
        }
        plan
    }
}
