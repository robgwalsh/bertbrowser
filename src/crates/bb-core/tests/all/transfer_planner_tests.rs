// Port of tests/BertBrowser.Core.Tests/TransferPlannerTests.cs. Rules that decide whether a drop
// is allowed at all. These run against a fake filesystem so the dangerous layouts (a folder
// dropped into its own subtree, reached directly or through a junction) can be exercised without
// needing the privileges to create real links.
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use bb_core::transfer::{
    TransferPlan, TransferPlanner, TransferProbe, TransferRejection, TransferVerb,
};
use bb_paths::{PathKey, fold_upper, parent_key};

/// In-memory filesystem: a set of directories and files, plus a link table used to resolve
/// junction targets the same way the real probe walks them.
#[derive(Default)]
struct FakeProbe {
    directories: Mutex<HashSet<String>>,
    files: Mutex<HashSet<String>>,
    links: Mutex<HashMap<String, String>>,
}

fn full(path: &str) -> String {
    PathKey::normalize_display(path).unwrap()
}

/// Keep drive roots as "C:\", trim the separator everywhere else.
fn normalize(path: &str) -> String {
    let full = full(path);
    if full.len() > 3 {
        full.trim_end_matches('\\').to_string()
    } else {
        full
    }
}

impl FakeProbe {
    fn add_directory(&self, path: &str) {
        let mut p = Some(full(path));
        while let Some(current) = p {
            let trimmed = current.trim_end_matches('\\');
            let stored = if trimmed.len() > 2 { trimmed } else { &current };
            self.directories.lock().unwrap().insert(fold_upper(stored));
            p = parent_key(&current);
        }
    }

    fn add_file(&self, path: &str) {
        let full = full(path);
        self.files.lock().unwrap().insert(fold_upper(&full));
        if let Some(parent) = parent_key(&full) {
            self.add_directory(&parent);
        }
    }

    /// Registers `link` as a junction to `target`.
    fn add_link(&self, link: &str, target: &str) {
        self.links
            .lock()
            .unwrap()
            .insert(fold_upper(&full(link)), full(target));
        self.directories
            .lock()
            .unwrap()
            .insert(fold_upper(&full(link)));
    }

    fn try_resolve_once(&self, full: &str) -> Option<String> {
        let mut suffix = String::new();
        let mut current = Some(full.to_string());
        while let Some(c) = current {
            if let Some(target) = self.links.lock().unwrap().get(&fold_upper(&c)) {
                return Some(if suffix.is_empty() {
                    target.clone()
                } else {
                    format!("{target}\\{suffix}")
                });
            }
            let name = bb_paths::file_name(&c).to_string();
            if name.is_empty() {
                return None;
            }
            suffix = if suffix.is_empty() {
                name
            } else {
                format!("{name}\\{suffix}")
            };
            current = parent_key(&c);
        }
        None
    }
}

impl TransferProbe for FakeProbe {
    fn directory_exists(&self, path: &str) -> bool {
        self.directories
            .lock()
            .unwrap()
            .contains(&fold_upper(&normalize(path)))
    }

    fn file_exists(&self, path: &str) -> bool {
        self.files
            .lock()
            .unwrap()
            .contains(&fold_upper(&normalize(path)))
    }

    fn resolve_final_path(&self, path: &str) -> String {
        let mut full = normalize(path);
        for _ in 0..32 {
            match self.try_resolve_once(&full) {
                Some(next) => full = next,
                None => return full,
            }
        }
        full
    }
}

struct World {
    probe: &'static FakeProbe,
    planner: TransferPlanner,
}

impl World {
    fn new() -> World {
        // Leaked so the planner can own a boxed probe while the test keeps a handle to it.
        let probe: &'static FakeProbe = Box::leak(Box::new(FakeProbe::default()));
        let planner = TransferPlanner::new(Box::new(ProbeRef(probe)));
        World { probe, planner }
    }

    fn plan(&self, sources: &[&str], destination: &str, verb: TransferVerb) -> TransferPlan {
        let sources: Vec<String> = sources.iter().map(|s| s.to_string()).collect();
        self.planner.plan(&sources, destination, verb)
    }

    fn mv(&self, sources: &[&str], destination: &str) -> TransferPlan {
        self.plan(sources, destination, TransferVerb::Move)
    }
}

struct ProbeRef(&'static FakeProbe);

impl TransferProbe for ProbeRef {
    fn directory_exists(&self, path: &str) -> bool {
        self.0.directory_exists(path)
    }
    fn file_exists(&self, path: &str) -> bool {
        self.0.file_exists(path)
    }
    fn resolve_final_path(&self, path: &str) -> String {
        self.0.resolve_final_path(path)
    }
}

fn reason_for(plan: &TransferPlan, source: &str) -> TransferRejection {
    let matching: Vec<_> = plan
        .rejected
        .iter()
        .filter(|r| fold_upper(&r.source_path) == fold_upper(source))
        .collect();
    assert_eq!(matching.len(), 1, "expected one rejection for {source}");
    matching[0].reason
}

fn single(plan: &TransferPlan) -> &bb_core::transfer::PlannedTransfer {
    assert_eq!(plan.transfers.len(), 1, "expected one transfer");
    &plan.transfers[0]
}

// --- The core case: a folder must never go inside itself ---

#[test]
fn folder_dropped_onto_itself_is_rejected() {
    let w = World::new();
    w.probe.add_directory(r"C:\work\project");

    let plan = w.mv(&[r"C:\work\project"], r"C:\work\project");

    assert!(!plan.has_work());
    assert_eq!(
        TransferRejection::DestinationIsSource,
        reason_for(&plan, r"C:\work\project")
    );
}

#[test]
fn folder_dropped_onto_itself_is_rejected_regardless_of_casing() {
    let w = World::new();
    w.probe.add_directory(r"C:\work\project");

    let plan = w.mv(&[r"C:\work\project"], r"C:\WORK\PROJECT");

    assert!(!plan.has_work());
    assert_eq!(
        TransferRejection::DestinationIsSource,
        reason_for(&plan, r"C:\work\project")
    );
}

#[test]
fn folder_dropped_into_its_own_subtree_is_rejected() {
    for destination in [
        r"C:\work\project\sub",
        r"C:\work\project\sub\deeper\deepest",
    ] {
        let w = World::new();
        w.probe.add_directory(r"C:\work\project");
        w.probe.add_directory(destination);

        let plan = w.mv(&[r"C:\work\project"], destination);

        assert!(!plan.has_work(), "{destination}");
        assert_eq!(
            TransferRejection::DestinationInsideSource,
            reason_for(&plan, r"C:\work\project"),
            "{destination}"
        );
    }
}

#[test]
fn folder_dropped_into_its_own_subtree_through_a_junction_is_rejected() {
    // C:\link is a junction to the folder being moved, so C:\link\sub is physically inside it
    // even though nothing about the literal paths says so.
    let w = World::new();
    w.probe.add_directory(r"C:\work\project");
    w.probe.add_directory(r"C:\work\project\sub");
    w.probe.add_link(r"C:\link", r"C:\work\project");
    w.probe.add_directory(r"C:\link\sub");

    let plan = w.mv(&[r"C:\work\project"], r"C:\link\sub");

    assert!(!plan.has_work());
    assert_eq!(
        TransferRejection::DestinationInsideSource,
        reason_for(&plan, r"C:\work\project")
    );
}

#[test]
fn folder_dropped_onto_a_junction_pointing_at_itself_is_rejected() {
    let w = World::new();
    w.probe.add_directory(r"C:\work\project");
    w.probe.add_link(r"C:\link", r"C:\work\project");

    let plan = w.mv(&[r"C:\work\project"], r"C:\link");

    assert!(!plan.has_work());
    assert_eq!(
        TransferRejection::DestinationInsideSource,
        reason_for(&plan, r"C:\work\project")
    );
}

#[test]
fn folder_dropped_into_an_unrelated_junction_is_allowed() {
    let w = World::new();
    w.probe.add_directory(r"C:\work\project");
    w.probe.add_directory(r"C:\elsewhere");
    w.probe.add_link(r"C:\link", r"C:\elsewhere");

    let plan = w.mv(&[r"C:\work\project"], r"C:\link");

    assert!(plan.problems().is_empty());
    assert_eq!(r"C:\link\project", single(&plan).destination_path);
}

#[test]
fn sibling_with_a_shared_name_prefix_is_not_treated_as_a_subtree() {
    // "C:\workspace" starts with "C:\work" as a string but is not inside it.
    let w = World::new();
    w.probe.add_directory(r"C:\work");
    w.probe.add_directory(r"C:\workspace");

    let plan = w.mv(&[r"C:\work"], r"C:\workspace");

    assert!(plan.problems().is_empty());
    assert_eq!(r"C:\workspace\work", single(&plan).destination_path);
}

#[test]
fn file_named_like_its_destination_is_not_confused_for_a_folder() {
    // A file source can never contain the destination, so no containment rule applies.
    let w = World::new();
    w.probe.add_file(r"C:\work\project");
    w.probe.add_directory(r"C:\dest");

    let plan = w.mv(&[r"C:\work\project"], r"C:\dest");

    assert_eq!(r"C:\dest\project", single(&plan).destination_path);
}

// --- Sources that travel with something else ---

#[test]
fn source_nested_under_another_source_is_dropped_from_the_plan() {
    let w = World::new();
    w.probe.add_directory(r"C:\src\tree");
    w.probe.add_directory(r"C:\src\tree\inner");
    w.probe.add_file(r"C:\src\tree\inner\leaf.txt");
    w.probe.add_directory(r"C:\dest");

    let plan = w.mv(
        &[
            r"C:\src\tree",
            r"C:\src\tree\inner",
            r"C:\src\tree\inner\leaf.txt",
        ],
        r"C:\dest",
    );

    assert_eq!(r"C:\src\tree", single(&plan).source_path);
    assert_eq!(
        TransferRejection::MovesWithAncestor,
        reason_for(&plan, r"C:\src\tree\inner")
    );
    assert_eq!(
        TransferRejection::MovesWithAncestor,
        reason_for(&plan, r"C:\src\tree\inner\leaf.txt")
    );
    assert!(plan.problems().is_empty()); // travelling with an ancestor is a no-op, not an error
}

#[test]
fn sibling_sources_are_all_kept() {
    let w = World::new();
    w.probe.add_directory(r"C:\src\one");
    w.probe.add_directory(r"C:\src\two");
    w.probe.add_directory(r"C:\dest");

    let plan = w.mv(&[r"C:\src\one", r"C:\src\two"], r"C:\dest");

    assert_eq!(2, plan.transfers.len());
}

#[test]
fn repeated_sources_are_deduped_including_by_case() {
    let w = World::new();
    w.probe.add_file(r"C:\src\a.txt");
    w.probe.add_directory(r"C:\dest");

    let plan = w.mv(
        &[r"C:\src\a.txt", r"C:\SRC\A.TXT", r"C:\src\a.txt"],
        r"C:\dest",
    );

    assert_eq!(1, plan.transfers.len());
}

// --- Roots, missing pieces, bad destinations ---

#[test]
fn drive_root_is_rejected() {
    for root in [r"C:\", r"D:\"] {
        let w = World::new();
        w.probe.add_directory(root);
        w.probe.add_directory(r"C:\dest");

        let plan = w.mv(&[root], r"C:\dest");

        assert!(!plan.has_work(), "{root}");
        assert_eq!(
            TransferRejection::SourceIsRoot,
            reason_for(&plan, root),
            "{root}"
        );
    }
}

#[test]
fn missing_source_is_rejected_and_does_not_stop_the_others() {
    let w = World::new();
    w.probe.add_file(r"C:\src\present.txt");
    w.probe.add_directory(r"C:\dest");

    let plan = w.mv(&[r"C:\src\gone.txt", r"C:\src\present.txt"], r"C:\dest");

    assert_eq!(r"C:\src\present.txt", single(&plan).source_path);
    assert_eq!(
        TransferRejection::SourceMissing,
        reason_for(&plan, r"C:\src\gone.txt")
    );
}

#[test]
fn missing_destination_rejects_everything() {
    let w = World::new();
    w.probe.add_file(r"C:\src\a.txt");

    let plan = w.mv(&[r"C:\src\a.txt"], r"C:\nope");

    assert!(!plan.has_work());
    assert_eq!(
        TransferRejection::DestinationMissing,
        reason_for(&plan, r"C:\src\a.txt")
    );
}

#[test]
fn file_as_destination_rejects_everything() {
    let w = World::new();
    w.probe.add_file(r"C:\src\a.txt");
    w.probe.add_file(r"C:\dest.txt");

    let plan = w.mv(&[r"C:\src\a.txt"], r"C:\dest.txt");

    assert!(!plan.has_work());
    assert_eq!(
        TransferRejection::DestinationNotDirectory,
        reason_for(&plan, r"C:\src\a.txt")
    );
}

#[test]
fn no_sources_produces_no_work() {
    let w = World::new();
    w.probe.add_directory(r"C:\dest");

    assert!(!w.mv(&[], r"C:\dest").has_work());
}

// --- Same-folder drops ---

#[test]
fn move_into_its_own_folder_is_a_no_op() {
    let w = World::new();
    w.probe.add_file(r"C:\src\a.txt");
    w.probe.add_directory(r"C:\src");

    let plan = w.mv(&[r"C:\src\a.txt"], r"C:\src");

    assert!(!plan.has_work());
    assert_eq!(
        TransferRejection::AlreadyInDestination,
        reason_for(&plan, r"C:\src\a.txt")
    );
    assert!(plan.problems().is_empty()); // nothing to do is not an error
}

#[test]
fn copy_into_its_own_folder_is_allowed_and_conflicts() {
    let w = World::new();
    w.probe.add_file(r"C:\src\a.txt");
    w.probe.add_directory(r"C:\src");

    let plan = w.plan(&[r"C:\src\a.txt"], r"C:\src", TransferVerb::Copy);

    assert!(single(&plan).conflicts);
}

// --- Conflict detection ---

#[test]
fn existing_name_at_destination_is_flagged_as_a_conflict() {
    let w = World::new();
    w.probe.add_file(r"C:\src\a.txt");
    w.probe.add_directory(r"C:\dest");
    w.probe.add_file(r"C:\dest\a.txt");

    let plan = w.mv(&[r"C:\src\a.txt"], r"C:\dest");

    assert!(single(&plan).conflicts);
    assert_eq!(1, plan.conflicts().len());
}

#[test]
fn existing_folder_blocking_a_file_name_is_a_conflict() {
    let w = World::new();
    w.probe.add_file(r"C:\src\report");
    w.probe.add_directory(r"C:\dest");
    w.probe.add_directory(r"C:\dest\report");

    let plan = w.mv(&[r"C:\src\report"], r"C:\dest");

    assert!(single(&plan).conflicts);
}

#[test]
fn two_sources_with_the_same_name_second_one_conflicts() {
    let w = World::new();
    w.probe.add_file(r"C:\one\a.txt");
    w.probe.add_file(r"C:\two\a.txt");
    w.probe.add_directory(r"C:\dest");

    let plan = w.mv(&[r"C:\one\a.txt", r"C:\two\a.txt"], r"C:\dest");

    assert!(!plan.transfers[0].conflicts);
    assert!(plan.transfers[1].conflicts);
}

/// The other half of that case, and the reason it needs naming: nothing is on disk at the
/// destination, so asking the filesystem what the second item clashes with answers "missing".
/// What the user has to be told is which other item in the same drop they are choosing between.
#[test]
fn the_second_of_two_same_named_sources_clashes_with_the_first_not_with_the_disk() {
    let w = World::new();
    w.probe.add_file(r"C:\one\a.txt");
    w.probe.add_file(r"C:\two\a.txt");
    w.probe.add_directory(r"C:\dest");

    let plan = w.mv(&[r"C:\one\a.txt", r"C:\two\a.txt"], r"C:\dest");

    assert_eq!(
        Some(&plan.transfers[0]),
        plan.earlier_claimant_of(&plan.transfers[1])
    );
    assert_eq!(None, plan.earlier_claimant_of(&plan.transfers[0]));
}

#[test]
fn a_clash_with_something_already_on_disk_has_no_earlier_claimant() {
    let w = World::new();
    w.probe.add_file(r"C:\src\a.txt");
    w.probe.add_directory(r"C:\dest");
    w.probe.add_file(r"C:\dest\a.txt");

    let plan = w.mv(&[r"C:\src\a.txt"], r"C:\dest");

    assert!(single(&plan).conflicts);
    assert_eq!(None, plan.earlier_claimant_of(single(&plan)));
}

#[test]
fn no_existing_name_is_not_a_conflict() {
    let w = World::new();
    w.probe.add_file(r"C:\src\a.txt");
    w.probe.add_directory(r"C:\dest");

    assert!(!single(&w.mv(&[r"C:\src\a.txt"], r"C:\dest")).conflicts);
}
