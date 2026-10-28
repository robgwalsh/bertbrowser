// Port of tests/BertBrowser.Core.Tests/RenamePlannerTests.cs. The rules that decide whether a
// rename may go ahead. Run against a fake filesystem, because what matters is which names are taken
// and by whom — a name held by another item in the same batch is only free if that item is really
// leaving it.
use std::collections::HashSet;

use bb_core::rename::{RenamePlan, RenamePlanner, RenameProbe, RenameRejection, RenameSource};
use bb_paths::PathKey;

/// An in-memory filesystem: which paths exist, and whether each is a file or a folder.
#[derive(Default)]
pub struct FakeRenameProbe {
    directories: HashSet<String>,
    files: HashSet<String>,
}

impl FakeRenameProbe {
    pub fn add_directory(&mut self, path: &str) {
        self.directories
            .insert(PathKey::canonicalize(path).unwrap());
    }

    pub fn add_file(&mut self, path: &str) {
        self.files.insert(PathKey::canonicalize(path).unwrap());
    }
}

impl RenameProbe for FakeRenameProbe {
    fn directory_exists(&self, path: &str) -> bool {
        PathKey::canonicalize(path).is_ok_and(|k| self.directories.contains(&k))
    }

    fn file_exists(&self, path: &str) -> bool {
        PathKey::canonicalize(path).is_ok_and(|k| self.files.contains(&k))
    }
}

fn file(path: &str) -> RenameSource {
    RenameSource::file(path)
}

fn dir(path: &str) -> RenameSource {
    RenameSource::dir(path)
}

fn reason_for(plan: &RenamePlan, source: &str) -> RenameRejection {
    let mut found = plan
        .rejected
        .iter()
        .filter(|r| r.source_path.eq_ignore_ascii_case(source));
    let one = found.next().expect("a rejection for the source");
    assert!(found.next().is_none(), "one rejection for {source}");
    one.reason
}

fn target_of(plan: &RenamePlan, source: &str) -> String {
    let mut found = plan
        .renames
        .iter()
        .filter(|r| r.source_path.eq_ignore_ascii_case(source));
    let one = found.next().expect("a rename for the source");
    assert!(found.next().is_none(), "one rename for {source}");
    one.target_name().to_string()
}

// --- the ordinary cases ---

#[test]
fn one_file_is_planned_into_its_own_folder() {
    let mut probe = FakeRenameProbe::default();
    probe.add_file(r"C:\p\old.txt");

    let plan = RenamePlanner::new(&probe).plan_simple(&[file(r"C:\p\old.txt")], "new.txt");

    assert!(plan.rejected.is_empty());
    assert_eq!(1, plan.renames.len());
    assert_eq!(r"C:\p\new.txt", plan.renames[0].target_path);
}

#[test]
fn several_files_are_numbered() {
    let mut probe = FakeRenameProbe::default();
    probe.add_file(r"C:\p\a.jpg");
    probe.add_file(r"C:\p\b.jpg");

    let plan =
        RenamePlanner::new(&probe).plan_simple(&[file(r"C:\p\a.jpg"), file(r"C:\p\b.jpg")], "Trip");

    assert!(plan.rejected.is_empty());
    assert_eq!("Trip 1.jpg", target_of(&plan, r"C:\p\a.jpg"));
    assert_eq!("Trip 2.jpg", target_of(&plan, r"C:\p\b.jpg"));
}

#[test]
fn renaming_to_the_same_name_is_planned_but_is_no_work() {
    let mut probe = FakeRenameProbe::default();
    probe.add_file(r"C:\p\a.txt");

    let plan = RenamePlanner::new(&probe).plan_simple(&[file(r"C:\p\a.txt")], "a.txt");

    assert_eq!(1, plan.renames.len());
    assert!(plan.renames[0].is_no_op());
    assert!(!plan.has_work());
    assert!(plan.work().is_empty());
}

#[test]
fn changing_only_the_casing_is_real_work() {
    let mut probe = FakeRenameProbe::default();
    probe.add_file(r"C:\p\readme.md");

    let plan = RenamePlanner::new(&probe).plan_simple(&[file(r"C:\p\readme.md")], "README.md");

    assert!(plan.has_work());
    assert!(plan.rejected.is_empty());
}

#[test]
fn the_same_item_listed_twice_is_renamed_once() {
    let mut probe = FakeRenameProbe::default();
    probe.add_file(r"C:\p\a.txt");

    let plan = RenamePlanner::new(&probe)
        .plan_simple(&[file(r"C:\p\a.txt"), file(r"C:\P\A.TXT")], "b.txt");

    // Not numbered — it is one item.
    assert_eq!(1, plan.renames.len());
    assert_eq!("b.txt", plan.renames[0].target_name());
}

// --- nothing is ever renamed over something else ---

#[test]
fn a_name_held_by_something_else_is_refused() {
    let mut probe = FakeRenameProbe::default();
    probe.add_file(r"C:\p\a.txt");
    probe.add_file(r"C:\p\taken.txt");

    let plan = RenamePlanner::new(&probe).plan_simple(&[file(r"C:\p\a.txt")], "taken.txt");

    assert!(!plan.has_work());
    assert_eq!(RenameRejection::NameTaken, reason_for(&plan, r"C:\p\a.txt"));
}

#[test]
fn a_name_held_by_a_folder_is_refused_for_a_file_too() {
    let mut probe = FakeRenameProbe::default();
    probe.add_file(r"C:\p\a.txt");
    probe.add_directory(r"C:\p\taken.txt");

    let plan = RenamePlanner::new(&probe).plan_simple(&[file(r"C:\p\a.txt")], "taken.txt");

    assert_eq!(RenameRejection::NameTaken, reason_for(&plan, r"C:\p\a.txt"));
}

#[test]
fn one_taken_name_in_a_batch_only_refuses_the_item_that_wanted_it() {
    let mut probe = FakeRenameProbe::default();
    probe.add_directory(r"C:\p\one");
    probe.add_directory(r"C:\p\two");
    probe.add_directory(r"C:\p\three");
    probe.add_directory(r"C:\p\Work 2"); // in the way of the second one

    let plan = RenamePlanner::new(&probe).plan_simple(
        &[dir(r"C:\p\one"), dir(r"C:\p\two"), dir(r"C:\p\three")],
        "Work",
    );

    assert_eq!(RenameRejection::NameTaken, reason_for(&plan, r"C:\p\two"));
    assert_eq!(2, plan.work().len());
}

// --- a name held by another selected item ---

#[test]
fn renaming_a_set_onto_its_own_names_is_allowed() {
    // Re-running the same numbered rename: every target is a name the batch already holds.
    let mut probe = FakeRenameProbe::default();
    probe.add_file(r"C:\p\Trip 1.jpg");
    probe.add_file(r"C:\p\Trip 2.jpg");

    let plan = RenamePlanner::new(&probe).plan_simple(
        &[file(r"C:\p\Trip 2.jpg"), file(r"C:\p\Trip 1.jpg")],
        "Trip",
    );

    assert!(plan.rejected.is_empty());
    assert_eq!("Trip 1.jpg", target_of(&plan, r"C:\p\Trip 2.jpg"));
    assert_eq!("Trip 2.jpg", target_of(&plan, r"C:\p\Trip 1.jpg"));
}

#[test]
fn aiming_at_the_name_of_a_selected_item_that_is_not_moving_is_refused() {
    // "Work 1" is selected but its own rename is refused (the name is taken), so it stays put —
    // and the item aiming at "Work 1" must be refused as well rather than overwrite it.
    let mut probe = FakeRenameProbe::default();
    probe.add_directory(r"C:\p\keep");
    probe.add_directory(r"C:\p\Work 2");
    probe.add_directory(r"C:\p\Work 1");

    let plan =
        RenamePlanner::new(&probe).plan_simple(&[dir(r"C:\p\keep"), dir(r"C:\p\Work 1")], "Work");

    // "keep" -> "Work 1" would land on "Work 1", which is only leaving if its own rename to
    // "Work 2" succeeds — it does not, because an unrelated "Work 2" is already there.
    assert_eq!(
        RenameRejection::NameTaken,
        reason_for(&plan, r"C:\p\Work 1")
    );
    assert_eq!(RenameRejection::NameTaken, reason_for(&plan, r"C:\p\keep"));
    assert!(!plan.has_work());
}

#[test]
fn an_item_the_pattern_leaves_alone_still_counts_as_planned() {
    let mut probe = FakeRenameProbe::default();
    probe.add_directory(r"C:\p\Work 1");
    probe.add_directory(r"C:\p\other");

    let plan =
        RenamePlanner::new(&probe).plan_simple(&[dir(r"C:\p\Work 1"), dir(r"C:\p\other")], "Work");

    assert!(plan.rejected.is_empty());
    let untouched = plan
        .renames
        .iter()
        .find(|r| r.source_path == r"C:\p\Work 1")
        .unwrap();
    assert!(untouched.is_no_op());
    assert_eq!(1, plan.work().len()); // only "other" is written
}

// --- refusals ---

#[test]
fn a_missing_item_is_refused() {
    let probe = FakeRenameProbe::default();

    let plan = RenamePlanner::new(&probe).plan_simple(&[file(r"C:\p\gone.txt")], "new.txt");

    assert_eq!(
        RenameRejection::SourceMissing,
        reason_for(&plan, r"C:\p\gone.txt")
    );
}

#[test]
fn a_drive_root_is_refused() {
    let mut probe = FakeRenameProbe::default();
    probe.add_directory(r"C:\");

    let plan = RenamePlanner::new(&probe).plan_simple(&[dir(r"C:\")], "data");

    assert_eq!(RenameRejection::SourceIsRoot, reason_for(&plan, r"C:\"));
}

#[test]
fn an_illegal_name_is_refused() {
    for pattern in ["", "a/b", "CON"] {
        let mut probe = FakeRenameProbe::default();
        probe.add_file(r"C:\p\a.txt");

        let plan = RenamePlanner::new(&probe).plan_simple(&[file(r"C:\p\a.txt")], pattern);

        assert_eq!(
            RenameRejection::InvalidName,
            reason_for(&plan, r"C:\p\a.txt"),
            "{pattern:?}"
        );
        assert!(!plan.has_work(), "{pattern:?}");
    }
}

#[test]
fn an_illegal_pattern_in_a_batch_refuses_every_item() {
    let mut probe = FakeRenameProbe::default();
    probe.add_file(r"C:\p\a.txt");
    probe.add_file(r"C:\p\b.txt");

    let plan =
        RenamePlanner::new(&probe).plan_simple(&[file(r"C:\p\a.txt"), file(r"C:\p\b.txt")], "a/b");

    assert_eq!(2, plan.rejected.len());
    assert!(
        plan.rejected
            .iter()
            .all(|r| r.reason == RenameRejection::InvalidName)
    );
    assert!(!plan.has_work());
}

#[test]
fn a_batch_spanning_two_folders_numbers_across_both_and_renames_in_place() {
    // Search results are flat, so a selection can span folders; each item stays where it is.
    let mut probe = FakeRenameProbe::default();
    probe.add_file(r"C:\p\a.txt");
    probe.add_file(r"C:\q\b.txt");

    let plan =
        RenamePlanner::new(&probe).plan_simple(&[file(r"C:\p\a.txt"), file(r"C:\q\b.txt")], "Note");

    assert!(plan.rejected.is_empty());
    assert_eq!(r"C:\p\Note 1.txt", plan.renames[0].target_path);
    assert_eq!(r"C:\q\Note 2.txt", plan.renames[1].target_path);
}

#[test]
fn an_item_inside_a_folder_being_renamed_is_refused() {
    // A flattened search result can hold both, and renaming the folder first would move the
    // other item's path out from under it.
    let mut probe = FakeRenameProbe::default();
    probe.add_directory(r"C:\p\box");
    probe.add_file(r"C:\p\box\inner.txt");

    let plan = RenamePlanner::new(&probe)
        .plan_simple(&[dir(r"C:\p\box"), file(r"C:\p\box\inner.txt")], "Thing");

    assert_eq!(
        RenameRejection::InsideARenamedFolder,
        reason_for(&plan, r"C:\p\box\inner.txt")
    );
    assert_eq!(1, plan.work().len());
}

#[test]
fn no_sources_plans_nothing() {
    let probe = FakeRenameProbe::default();
    assert!(
        !RenamePlanner::new(&probe)
            .plan_simple(&[], "anything")
            .has_work()
    );
}
