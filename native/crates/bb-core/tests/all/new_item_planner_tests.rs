// Port of tests/BertBrowser.Core.Tests/NewItemPlannerTests.cs. Creating must be refused before
// anything is written, because the dialog shows the refusal while the name can still be changed.
// Every rule here is one the user reads rather than one they hit.
use std::collections::HashSet;

use bb_core::newitem::{NewItemKind, NewItemPlan, NewItemPlanner, NewItemProbe, NewItemRejection};
use bb_core::rename::RenamePattern;
use bb_paths::PathKey;

/// An in-memory filesystem: which paths exist, and whether each is a file or a folder.
#[derive(Default)]
pub struct FakeNewItemProbe {
    directories: HashSet<String>,
    files: HashSet<String>,
}

impl FakeNewItemProbe {
    pub fn add_directory(&mut self, path: &str) {
        self.directories
            .insert(PathKey::canonicalize(path).unwrap());
    }

    pub fn add_file(&mut self, path: &str) {
        self.files.insert(PathKey::canonicalize(path).unwrap());
    }
}

impl NewItemProbe for FakeNewItemProbe {
    fn directory_exists(&self, path: &str) -> bool {
        PathKey::canonicalize(path).is_ok_and(|k| self.directories.contains(&k))
    }

    fn file_exists(&self, path: &str) -> bool {
        PathKey::canonicalize(path).is_ok_and(|k| self.files.contains(&k))
    }
}

fn probe() -> FakeNewItemProbe {
    let mut probe = FakeNewItemProbe::default();
    probe.add_directory(r"C:\p");
    probe
}

fn plan_folder(probe: &FakeNewItemProbe, name: &str) -> NewItemPlan {
    NewItemPlanner::new(probe).plan(r"C:\p", name, NewItemKind::Folder, None)
}

fn plan_file(probe: &FakeNewItemProbe, name: &str, template: Option<&str>) -> NewItemPlan {
    NewItemPlanner::new(probe).plan(r"C:\p", name, NewItemKind::File, template)
}

fn reason_of(plan: &NewItemPlan) -> Option<NewItemRejection> {
    plan.rejected.as_ref().map(|r| r.reason)
}

// --- the ordinary cases ---

#[test]
fn a_free_name_is_planned() {
    let plan = plan_folder(&probe(), "Reports");
    assert!(plan.has_work());
    assert_eq!(r"C:\p\Reports", plan.target_path());
}

#[test]
fn trailing_dots_and_spaces_are_dropped_before_anything_sees_the_name() {
    // Windows drops them silently, so a plan that kept them would describe a path that never
    // appears on disk.
    let plan = plan_folder(&probe(), "Reports. ");
    assert!(plan.has_work());
    assert_eq!("Reports", plan.name);
}

// --- refusals ---

#[test]
fn a_name_held_by_a_file_is_refused() {
    let mut probe = probe();
    probe.add_file(r"C:\p\notes.txt");
    assert_eq!(
        Some(NewItemRejection::NameTaken),
        reason_of(&plan_file(&probe, "notes.txt", None))
    );
}

#[test]
fn a_name_held_by_a_folder_refuses_a_file_too_because_they_share_one_namespace() {
    let mut probe = probe();
    probe.add_directory(r"C:\p\notes.txt");
    assert_eq!(
        Some(NewItemRejection::NameTaken),
        reason_of(&plan_file(&probe, "notes.txt", None))
    );
}

#[test]
fn a_folder_that_has_gone_away_is_refused() {
    let probe = probe();
    let plan = NewItemPlanner::new(&probe).plan(r"C:\gone", "Reports", NewItemKind::Folder, None);
    assert_eq!(Some(NewItemRejection::ParentMissing), reason_of(&plan));
}

#[test]
fn a_template_that_is_not_there_is_refused_before_anything_is_written() {
    let plan = plan_file(&probe(), "letter.rtf", Some(r"C:\templates\letter.rtf"));
    assert_eq!(Some(NewItemRejection::TemplateMissing), reason_of(&plan));
}

#[test]
fn a_template_that_is_there_is_accepted() {
    let mut probe = probe();
    probe.add_file(r"C:\templates\letter.rtf");
    assert!(plan_file(&probe, "letter.rtf", Some(r"C:\templates\letter.rtf")).has_work());
}

#[test]
fn an_illegal_name_is_refused() {
    for name in ["", "   ", "a/b", "a:b", "a?b", "CON", "NUL.txt"] {
        assert_eq!(
            Some(NewItemRejection::InvalidName),
            reason_of(&plan_folder(&probe(), name)),
            "{name:?}"
        );
    }
}

#[test]
fn an_illegal_name_is_refused_in_the_patterns_own_words() {
    // Mutate NewItemPattern into a second implementation of the naming rule and this goes red —
    // which is the point of it delegating to RenamePattern rather than restating it.
    let plan = plan_folder(&probe(), "a?b");
    assert_eq!(
        RenamePattern::validate("a?b").unwrap(),
        plan.rejected.unwrap().message
    );
}

// --- the suggested name ---

#[test]
fn the_suggestion_is_the_base_name_when_nothing_is_in_the_way() {
    assert_eq!(
        "New folder",
        NewItemPlanner::new(&probe()).suggest_name(r"C:\p", "New folder", NewItemKind::Folder, "")
    );
}

#[test]
fn a_suggestion_that_is_taken_steps_aside_rather_than_opening_on_a_refusal() {
    let mut probe = probe();
    probe.add_directory(r"C:\p\New folder");
    assert_eq!(
        "New folder (2)",
        NewItemPlanner::new(&probe).suggest_name(r"C:\p", "New folder", NewItemKind::Folder, "")
    );
}

#[test]
fn a_suggested_file_numbers_before_its_extension() {
    let mut probe = probe();
    probe.add_file(r"C:\p\New Text Document.txt");
    assert_eq!(
        "New Text Document (2).txt",
        NewItemPlanner::new(&probe).suggest_name(
            r"C:\p",
            "New Text Document",
            NewItemKind::File,
            ".txt"
        )
    );
}
