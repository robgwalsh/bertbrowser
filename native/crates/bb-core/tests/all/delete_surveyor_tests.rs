// Port of tests/BertBrowser.Core.Tests/DeleteSurveyorTests.cs. What the confirmation says is about
// to go. These numbers are the whole point of the "are you sure" — "delete 1 item" and "delete 1
// folder holding 4,000 files" are different questions — so they are asserted against real trees
// rather than assumed.
use std::cell::RefCell;
use std::path::{Path, PathBuf};

use bb_core::cancel::CancelToken;
use bb_core::delete::{
    DeleteMeasurement, DeleteMode, DeletePlan, DeletePlanner, DeleteSource, DeleteSurvey,
    DeleteSurveyor, FileSystemDeleteProbe,
};
use bb_test_support::unique_temp_path;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-survey", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn p(&self, parts: &[&str]) -> PathBuf {
        let mut p = self.root.clone();
        for part in parts {
            p.push(part);
        }
        p
    }

    fn file(&self, content: &str, parts: &[&str]) -> PathBuf {
        let p = self.p(parts);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    fn plan(&self, sources: &[(PathBuf, bool)]) -> DeletePlan {
        let sources: Vec<DeleteSource> = sources
            .iter()
            .map(|(p, is_dir)| DeleteSource::new(&s(p), *is_dir))
            .collect();
        DeletePlanner::new(&FileSystemDeleteProbe)
            .protecting(&[])
            .plan(&sources, DeleteMode::Staged)
    }

    fn survey(&self, sources: &[PathBuf]) -> DeleteSurvey {
        let sources: Vec<(PathBuf, bool)> =
            sources.iter().map(|p| (p.clone(), p.is_dir())).collect();
        DeleteSurveyor.survey(&self.plan(&sources), &CancelToken::none(), None)
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

#[test]
fn a_file_is_one_file_and_its_own_length() {
    let f = Fixture::new();
    let file = f.file("hello", &["a.txt"]);

    let survey = f.survey(std::slice::from_ref(&file));

    assert_eq!(survey.files(), 1);
    assert_eq!(survey.directories(), 0);
    assert_eq!(survey.bytes(), std::fs::metadata(&file).unwrap().len());
    assert!(!survey.incomplete());
}

#[test]
fn a_folder_counts_itself_and_everything_under_it() {
    let f = Fixture::new();
    f.file("one", &["tree", "a.txt"]);
    f.file("two", &["tree", "deep", "b.txt"]);
    f.file("three", &["tree", "deep", "deeper", "c.txt"]);

    let survey = f.survey(&[f.p(&["tree"])]);

    assert_eq!(survey.files(), 3);
    assert_eq!(survey.directories(), 3); // tree, deep, deeper
    assert_eq!(survey.bytes(), 11); // "one" + "two" + "three"
}

#[test]
fn an_empty_folder_is_one_directory_and_nothing_else() {
    let f = Fixture::new();
    let empty = f.p(&["empty"]);
    std::fs::create_dir_all(&empty).unwrap();

    let survey = f.survey(&[empty]);

    assert_eq!(survey.files(), 0);
    assert_eq!(survey.directories(), 1);
    assert_eq!(survey.bytes(), 0);
}

#[test]
fn several_items_add_up() {
    let f = Fixture::new();
    let a = f.file("aaaa", &["a.txt"]);
    f.file("bb", &["tree", "b.txt"]);

    let survey = f.survey(&[a, f.p(&["tree"])]);

    assert_eq!(survey.items.len(), 2);
    assert_eq!(survey.files(), 2);
    assert_eq!(survey.directories(), 1);
    assert_eq!(survey.bytes(), 6);
}

#[test]
fn each_item_is_reported_as_it_is_finished() {
    let f = Fixture::new();
    let a = f.file("a", &["a.txt"]);
    let b = f.file("b", &["b.txt"]);

    let reports: RefCell<Vec<DeleteMeasurement>> = RefCell::new(Vec::new());
    let plan = f.plan(&[(a.clone(), false), (b.clone(), false)]);
    DeleteSurveyor.survey(
        &plan,
        &CancelToken::none(),
        Some(&|m: &DeleteMeasurement| reports.borrow_mut().push(m.clone())),
    );

    let reported: Vec<String> = reports
        .into_inner()
        .into_iter()
        .map(|r| r.source_path)
        .collect();
    assert_eq!(reported, vec![s(&a), s(&b)]);
}

#[test]
fn an_item_that_has_gone_is_marked_incomplete_rather_than_throwing() {
    let f = Fixture::new();
    let file = f.file("hello", &["a.txt"]);
    let plan = f.plan(&[(file.clone(), false)]);
    std::fs::remove_file(&file).unwrap();

    let survey = DeleteSurveyor.survey(&plan, &CancelToken::none(), None);

    assert!(survey.incomplete());
    assert_eq!(survey.bytes(), 0);
}

#[test]
fn cancellation_stops_the_walk_and_says_so() {
    let f = Fixture::new();
    f.file("one", &["tree", "a.txt"]);
    let plan = f.plan(&[(f.p(&["tree"]), true)]);

    let cancel = CancelToken::new();
    cancel.cancel();
    let survey = DeleteSurveyor.survey(&plan, &cancel, None);

    assert!(survey.items.is_empty());
}
