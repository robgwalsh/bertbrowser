// Port of tests/BertBrowser.Core.Tests/ShortcutPlannerTests.cs. What "Create shortcuts here" would
// produce. Run against a fake filesystem: the interesting cases are collisions — with what is
// already in the folder, and with each other — and setting those up for real would mean writing
// .lnk files to find out.
use std::collections::HashSet;

use bb_core::shortcuts::{ShortcutPlan, ShortcutPlanner, ShortcutProbe};
use bb_paths::PathKey;

#[derive(Default)]
pub struct FakeShortcutProbe {
    directories: HashSet<String>,
    files: HashSet<String>,
}

impl FakeShortcutProbe {
    pub fn add_directory(&mut self, path: &str) {
        self.directories
            .insert(PathKey::canonicalize(path).unwrap());
    }

    pub fn add_file(&mut self, path: &str) {
        self.files.insert(PathKey::canonicalize(path).unwrap());
    }
}

impl ShortcutProbe for FakeShortcutProbe {
    fn directory_exists(&self, path: &str) -> bool {
        PathKey::canonicalize(path).is_ok_and(|k| self.directories.contains(&k))
    }

    fn file_exists(&self, path: &str) -> bool {
        PathKey::canonicalize(path).is_ok_and(|k| self.files.contains(&k))
    }
}

fn probe() -> FakeShortcutProbe {
    let mut probe = FakeShortcutProbe::default();
    probe.add_directory(r"C:\dest");
    probe
}

fn plan(probe: &FakeShortcutProbe, sources: &[&str]) -> ShortcutPlan {
    let sources: Vec<String> = sources.iter().map(|s| s.to_string()).collect();
    ShortcutPlanner::new(probe).plan(&sources, r"C:\dest")
}

fn link_for(plan: &ShortcutPlan, source: &str) -> String {
    let mut found = plan
        .creations
        .iter()
        .filter(|c| c.target_path.eq_ignore_ascii_case(source));
    let one = found.next().expect("a creation for the source");
    assert!(found.next().is_none());
    one.link_path.clone()
}

fn single_link(plan: &ShortcutPlan) -> &str {
    assert_eq!(1, plan.creations.len());
    &plan.creations[0].link_path
}

// --- Naming: Explorer's rule, extension and all ---

#[test]
fn a_file_gets_a_name_and_shortcut_link() {
    let mut probe = probe();
    probe.add_file(r"C:\src\notes.txt");

    let plan = plan(&probe, &[r"C:\src\notes.txt"]);

    assert!(plan.problems.is_empty());
    assert_eq!(r"C:\dest\notes.txt - Shortcut.lnk", single_link(&plan));
}

/// A folder keeps its whole name too — the number, when one is needed, goes before the `.lnk`
/// because the thing being created is a file whatever it points at.
#[test]
fn a_folder_gets_the_same_treatment() {
    let mut probe = probe();
    probe.add_directory(r"C:\src\Reports");

    let plan = plan(&probe, &[r"C:\src\Reports"]);

    assert_eq!(r"C:\dest\Reports - Shortcut.lnk", single_link(&plan));
}

#[test]
fn a_taken_name_steps_aside() {
    let mut probe = probe();
    probe.add_file(r"C:\src\notes.txt");
    probe.add_file(r"C:\dest\notes.txt - Shortcut.lnk");

    let plan = plan(&probe, &[r"C:\src\notes.txt"]);

    assert_eq!(r"C:\dest\notes.txt - Shortcut (2).lnk", single_link(&plan));
}

/// The one a per-item rule gets wrong. Two files with the same name from different folders are
/// one drag, and nothing is on disk yet — so a planner that only asks the probe hands both the
/// same link path and the second write silently replaces the first.
#[test]
fn two_sources_named_alike_get_different_links() {
    let mut probe = probe();
    probe.add_file(r"C:\one\notes.txt");
    probe.add_file(r"C:\two\notes.txt");

    let plan = plan(&probe, &[r"C:\one\notes.txt", r"C:\two\notes.txt"]);

    assert_eq!(
        r"C:\dest\notes.txt - Shortcut.lnk",
        link_for(&plan, r"C:\one\notes.txt")
    );
    assert_eq!(
        r"C:\dest\notes.txt - Shortcut (2).lnk",
        link_for(&plan, r"C:\two\notes.txt")
    );
}

// --- Refusals ---

/// One item's problem never costs the others theirs — the same rule every other planner in the
/// app follows.
#[test]
fn a_missing_source_is_refused_and_the_rest_are_planned() {
    let mut probe = probe();
    probe.add_file(r"C:\src\here.txt");

    let plan = plan(&probe, &[r"C:\src\gone.txt", r"C:\src\here.txt"]);

    assert_eq!(1, plan.problems.len());
    assert_eq!(r"C:\src\gone.txt", plan.problems[0].source_path);
    assert_eq!(r"C:\dest\here.txt - Shortcut.lnk", single_link(&plan));
}

/// An item inside an archive has a path that reads like a real one but names nothing on disk, so
/// there is nothing for a .lnk to point at. No archive knowledge is needed to refuse it: it simply
/// does not exist, which is the rule that was already there.
#[test]
fn an_item_inside_an_archive_is_refused() {
    let mut probe = probe();
    probe.add_file(r"C:\src\bundle.zip");

    let plan = plan(&probe, &[r"C:\src\bundle.zip\inner\notes.txt"]);

    assert!(plan.creations.is_empty());
    assert_eq!(1, plan.problems.len());
}

#[test]
fn a_missing_destination_refuses_everything() {
    let mut probe = probe();
    probe.add_file(r"C:\src\notes.txt");

    let plan = ShortcutPlanner::new(&probe).plan(&[r"C:\src\notes.txt".to_string()], r"C:\nowhere");

    assert!(plan.creations.is_empty());
    assert!(!plan.has_work());
    assert_eq!(1, plan.problems.len());
}

#[test]
fn a_plan_with_nothing_in_it_has_no_work() {
    assert!(
        !ShortcutPlanner::new(&probe())
            .plan(&[], r"C:\dest")
            .has_work()
    );
}
