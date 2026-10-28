// Port of tests/BertBrowser.Core.Tests/DeletePlannerTests.cs. The rules that decide what a delete
// is allowed to reach. Run against a fake filesystem, because the interesting cases — a drive root,
// the Windows folder, a folder selected together with something inside it — are ones nobody should
// set up for real to find out.
use std::cell::Cell;
use std::collections::HashSet;

use bb_core::delete::{
    DeleteDisposition, DeleteMode, DeletePlan, DeletePlanner, DeleteProbe, DeleteRejection,
    DeleteSource, RecycleProbe,
};
use bb_paths::{PathKey, root_len};

/// A Recycle Bin that exists on some volumes and not others — the case that matters, since a
/// network share or media with the bin turned off is what makes the staged fallback necessary and
/// is not something a unit test can conjure up for real.
pub struct FakeRecycleProbe {
    volumes: HashSet<String>,
    was_asked: Cell<bool>,
}

impl FakeRecycleProbe {
    pub fn new(volumes_with_a_bin: &[&str]) -> FakeRecycleProbe {
        FakeRecycleProbe {
            volumes: volumes_with_a_bin
                .iter()
                .map(|v| PathKey::canonicalize(v).unwrap())
                .collect(),
            was_asked: Cell::new(false),
        }
    }

    pub fn was_asked(&self) -> bool {
        self.was_asked.get()
    }
}

impl RecycleProbe for FakeRecycleProbe {
    fn can_recycle(&self, path: &str) -> bool {
        self.was_asked.set(true);
        let root = &path[..root_len(path)];
        !root.is_empty() && PathKey::canonicalize(root).is_ok_and(|key| self.volumes.contains(&key))
    }
}

#[derive(Default)]
pub struct FakeDeleteProbe {
    directories: HashSet<String>,
    files: HashSet<String>,
}

impl FakeDeleteProbe {
    pub fn add_directory(&mut self, path: &str) {
        self.directories
            .insert(PathKey::canonicalize(path).unwrap());
    }

    pub fn add_file(&mut self, path: &str) {
        self.files.insert(PathKey::canonicalize(path).unwrap());
    }
}

impl DeleteProbe for FakeDeleteProbe {
    fn directory_exists(&self, path: &str) -> bool {
        PathKey::canonicalize(path).is_ok_and(|k| self.directories.contains(&k))
    }

    fn file_exists(&self, path: &str) -> bool {
        PathKey::canonicalize(path).is_ok_and(|k| self.files.contains(&k))
    }
}

struct Fixture {
    probe: FakeDeleteProbe,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            probe: FakeDeleteProbe::default(),
        }
    }

    fn planner(&self) -> DeletePlanner<'_> {
        DeletePlanner::new(&self.probe).protecting(&[r"C:\Windows", r"C:\Users\rob"])
    }

    fn plan(&self, sources: &[DeleteSource]) -> DeletePlan {
        self.planner().plan(sources, DeleteMode::Staged)
    }

    /// Where each item is really going is decided here rather than in the executor, so the
    /// confirmation the user answers can say what will actually happen to it.
    fn plan_with_bin(
        &self,
        bin: &FakeRecycleProbe,
        mode: DeleteMode,
        sources: &[DeleteSource],
    ) -> DeletePlan {
        DeletePlanner::new(&self.probe)
            .protecting(&[r"C:\Windows"])
            .recycle_probe(bin)
            .plan(sources, mode)
    }
}

fn file(path: &str) -> DeleteSource {
    DeleteSource::new(path, false)
}

fn dir(path: &str) -> DeleteSource {
    DeleteSource::new(path, true)
}

fn reason_for(plan: &DeletePlan, source: &str) -> DeleteRejection {
    let matching: Vec<_> = plan
        .rejected
        .iter()
        .filter(|r| r.source_path.eq_ignore_ascii_case(source))
        .collect();
    assert_eq!(matching.len(), 1, "expected one rejection of {source}");
    matching[0].reason
}

fn single<T>(items: &[T]) -> &T {
    assert_eq!(items.len(), 1, "expected exactly one item");
    &items[0]
}

// --- the ordinary cases ---

#[test]
fn a_file_is_planned() {
    let mut f = Fixture::new();
    f.probe.add_file(r"C:\p\a.txt");

    let plan = f.plan(&[file(r"C:\p\a.txt")]);

    assert!(plan.rejected.is_empty());
    let deletion = single(&plan.deletions);
    assert_eq!(deletion.source_path, r"C:\p\a.txt");
    assert!(!deletion.is_directory);
}

#[test]
fn a_folder_is_planned_as_a_directory_whatever_the_caller_claimed() {
    let mut f = Fixture::new();
    f.probe.add_directory(r"C:\p\sub");

    // The selection says "file"; disk says otherwise, and disk wins — the executor needs the
    // right answer to know whether to move a file or a directory.
    let plan = f.plan(&[file(r"C:\p\sub")]);

    assert!(single(&plan.deletions).is_directory);
}

#[test]
fn the_same_item_twice_is_planned_once() {
    let mut f = Fixture::new();
    f.probe.add_file(r"C:\p\a.txt");

    let plan = f.plan(&[file(r"C:\p\a.txt"), file(r"C:\P\A.TXT")]);

    assert_eq!(plan.deletions.len(), 1);
}

#[test]
fn no_sources_is_an_empty_plan() {
    let f = Fixture::new();
    let plan = f.planner().plan(&[], DeleteMode::Permanent);

    assert!(!plan.has_work());
    assert!(plan.permanent());
}

// --- the refusals ---

#[test]
fn an_item_that_has_gone_is_refused() {
    let f = Fixture::new();
    let plan = f.plan(&[file(r"C:\p\vanished.txt")]);

    assert!(!plan.has_work());
    assert_eq!(
        reason_for(&plan, r"C:\p\vanished.txt"),
        DeleteRejection::SourceMissing
    );
}

#[test]
fn a_drive_root_is_refused() {
    let mut f = Fixture::new();
    f.probe.add_directory(r"D:\");

    let plan = f.plan(&[dir(r"D:\")]);

    assert!(!plan.has_work());
    assert_eq!(reason_for(&plan, r"D:\"), DeleteRejection::SourceIsRoot);
}

#[test]
fn a_protected_location_is_refused() {
    let mut f = Fixture::new();
    f.probe.add_directory(r"C:\Windows");

    let plan = f.plan(&[dir(r"C:\Windows")]);

    assert!(!plan.has_work());
    assert_eq!(
        reason_for(&plan, r"C:\Windows"),
        DeleteRejection::ProtectedLocation
    );
}

#[test]
fn something_inside_a_protected_location_is_still_deletable() {
    // The folder is protected, not everything under it — this is a file browser.
    let mut f = Fixture::new();
    f.probe.add_file(r"C:\Windows\Temp\junk.log");

    let plan = f.plan(&[file(r"C:\Windows\Temp\junk.log")]);

    assert_eq!(plan.deletions.len(), 1);
}

#[test]
fn an_item_inside_a_folder_being_deleted_travels_with_it_and_is_not_reported() {
    let mut f = Fixture::new();
    f.probe.add_directory(r"C:\p\sub");
    f.probe.add_file(r"C:\p\sub\a.txt");

    let plan = f.plan(&[dir(r"C:\p\sub"), file(r"C:\p\sub\a.txt")]);

    assert_eq!(single(&plan.deletions).source_path, r"C:\p\sub");
    assert_eq!(
        reason_for(&plan, r"C:\p\sub\a.txt"),
        DeleteRejection::InsideADeletedFolder
    );
    assert!(plan.problems().is_empty()); // benign: it is being deleted, just not separately
}

#[test]
fn a_deeply_nested_item_travels_with_its_ancestor_too() {
    let mut f = Fixture::new();
    f.probe.add_directory(r"C:\p\sub");
    f.probe.add_directory(r"C:\p\sub\deep\deeper");

    let plan = f.plan(&[dir(r"C:\p\sub"), dir(r"C:\p\sub\deep\deeper")]);

    assert_eq!(single(&plan.deletions).source_path, r"C:\p\sub");
}

#[test]
fn a_sibling_with_a_shared_name_prefix_is_not_mistaken_for_a_child() {
    // "C:\p\sub2" starts with "C:\p\sub" as a string but is not inside it.
    let mut f = Fixture::new();
    f.probe.add_directory(r"C:\p\sub");
    f.probe.add_directory(r"C:\p\sub2");

    let plan = f.plan(&[dir(r"C:\p\sub"), dir(r"C:\p\sub2")]);

    assert_eq!(plan.deletions.len(), 2);
    assert!(plan.rejected.is_empty());
}

#[test]
fn one_refusal_does_not_stop_the_rest() {
    let mut f = Fixture::new();
    f.probe.add_file(r"C:\p\a.txt");
    f.probe.add_directory(r"C:\Windows");

    let plan = f.plan(&[dir(r"C:\Windows"), file(r"C:\p\a.txt")]);

    assert_eq!(single(&plan.deletions).source_path, r"C:\p\a.txt");
    assert_eq!(plan.problems().len(), 1);
}

// --- the Recycle Bin ---

#[test]
fn on_a_volume_with_a_bin_an_ordinary_delete_recycles() {
    let mut f = Fixture::new();
    f.probe.add_file(r"C:\p\a.txt");
    let bin = FakeRecycleProbe::new(&[r"C:\"]);

    let plan = f.plan_with_bin(&bin, DeleteMode::Recycle, &[file(r"C:\p\a.txt")]);

    assert_eq!(
        single(&plan.deletions).disposition,
        DeleteDisposition::Recycle
    );
    assert!(!plan.has_staged_fallback());
}

/// The rule that stops a network share turning a delete into an erase: with no bin to hand the
/// item to, it goes into this app's own holding folder, where Ctrl+Z can still reach it.
#[test]
fn on_a_volume_with_no_bin_an_ordinary_delete_falls_back_to_the_holding_folder() {
    let mut f = Fixture::new();
    f.probe.add_file(r"\\server\share\a.txt");
    let bin = FakeRecycleProbe::new(&[r"C:\"]);

    let plan = f.plan_with_bin(&bin, DeleteMode::Recycle, &[file(r"\\server\share\a.txt")]);

    assert_eq!(
        single(&plan.deletions).disposition,
        DeleteDisposition::Stage
    );
}

/// A mixed selection is exactly what the fallback exists for, and the confirmation has to be able
/// to say that part of it is taking a different route.
#[test]
fn a_mixed_selection_routes_each_item_on_its_own_volume_and_says_so() {
    let mut f = Fixture::new();
    f.probe.add_file(r"C:\p\a.txt");
    f.probe.add_file(r"\\server\share\b.txt");
    let bin = FakeRecycleProbe::new(&[r"C:\"]);

    let plan = f.plan_with_bin(
        &bin,
        DeleteMode::Recycle,
        &[file(r"C:\p\a.txt"), file(r"\\server\share\b.txt")],
    );

    assert_eq!(plan.deletions[0].disposition, DeleteDisposition::Recycle);
    assert_eq!(plan.deletions[1].disposition, DeleteDisposition::Stage);
    assert!(plan.has_staged_fallback());
}

#[test]
fn a_shift_delete_erases_even_where_a_bin_exists() {
    let mut f = Fixture::new();
    f.probe.add_file(r"C:\p\a.txt");
    let bin = FakeRecycleProbe::new(&[r"C:\"]);

    let plan = f.plan_with_bin(&bin, DeleteMode::Permanent, &[file(r"C:\p\a.txt")]);

    assert_eq!(
        single(&plan.deletions).disposition,
        DeleteDisposition::Erase
    );
    assert!(plan.permanent());
    assert!(!plan.has_staged_fallback());
}

/// Asking for the holding folder explicitly never consults the bin at all.
#[test]
fn a_staged_delete_ignores_the_bin_entirely() {
    let mut f = Fixture::new();
    f.probe.add_file(r"C:\p\a.txt");
    let bin = FakeRecycleProbe::new(&[r"C:\"]);

    let plan = f.plan_with_bin(&bin, DeleteMode::Staged, &[file(r"C:\p\a.txt")]);

    assert_eq!(
        single(&plan.deletions).disposition,
        DeleteDisposition::Stage
    );
    assert!(!bin.was_asked());
}

/// Core on its own has no bin to ask about, so it must not assume one is there.
#[test]
fn with_no_recycle_probe_injected_everything_is_held_rather_than_recycled() {
    let mut f = Fixture::new();
    f.probe.add_file(r"C:\p\a.txt");

    let plan = f
        .planner()
        .plan(&[file(r"C:\p\a.txt")], DeleteMode::Recycle);

    assert_eq!(
        single(&plan.deletions).disposition,
        DeleteDisposition::Stage
    );
}

// --- the bin's own contents ---

/// Those `$R` files are what Ctrl+Z restores from, so deleting one out from under a pending undo
/// would quietly break it — and this app is elevated, so Windows will not object.
#[test]
fn anything_in_the_recycle_bin_is_refused() {
    for path in [
        r"C:\$Recycle.Bin",
        r"C:\$Recycle.Bin\S-1-5-21-1\$RAB1234.txt",
        r"C:\$RECYCLE.BIN\S-1-5-21-1\$RAB1234.txt",
        r"C:\RECYCLER\S-1-5-21-1\Dc1.txt",
    ] {
        let mut f = Fixture::new();
        f.probe.add_file(path);
        f.probe.add_directory(path);

        let plan = f.plan(&[file(path)]);

        assert!(plan.deletions.is_empty(), "{path}");
        assert_eq!(
            reason_for(&plan, path),
            DeleteRejection::ProtectedLocation,
            "{path}"
        );
    }
}

#[test]
fn a_folder_merely_named_like_the_bin_deeper_in_a_tree_is_still_refused() {
    // Conservative on purpose: a name test cannot tell a real bin from a decoy, and refusing to
    // delete something is recoverable in a way that deleting it is not.
    let mut f = Fixture::new();
    f.probe.add_directory(r"C:\p\$Recycle.Bin\inner");

    assert!(
        f.plan(&[dir(r"C:\p\$Recycle.Bin\inner")])
            .deletions
            .is_empty()
    );
}

#[test]
fn an_ordinary_folder_with_a_dollar_in_its_name_is_not_mistaken_for_the_bin() {
    let mut f = Fixture::new();
    f.probe.add_directory(r"C:\p\$Recycled");

    assert_eq!(f.plan(&[dir(r"C:\p\$Recycled")]).deletions.len(), 1);
}
