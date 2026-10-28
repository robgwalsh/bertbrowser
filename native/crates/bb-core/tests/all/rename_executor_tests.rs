// Port of tests/BertBrowser.Core.Tests/RenameExecutorTests.cs. Renames real files and folders on
// disk. Every test asserts on file *contents*, so a rename that overwrites or loses data fails
// loudly rather than merely producing the right names.
use std::path::{Path, PathBuf};

use bb_core::fs::DirectoryRemoval;
use bb_core::rename::{RenameExecutor, RenameOutcome, RenamePlanner, RenameSource};
use bb_test_support::unique_temp_path;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-rename", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn p(&self, parts: &[&str]) -> String {
        let mut p = self.root.clone();
        for part in parts {
            p.push(part);
        }
        p.to_string_lossy().into_owned()
    }

    fn file(&self, content: &str, parts: &[&str]) -> String {
        let path = self.p(parts);
        std::fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    fn dir(&self, parts: &[&str]) -> String {
        let path = self.p(parts);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn run(&self, sources: &[RenameSource], pattern: &str) -> RenameOutcome {
        RenameExecutor.execute(&RenamePlanner::local().plan_simple(sources, pattern))
    }

    /// The temporary names staging uses must never outlive the rename.
    fn nothing_stranded(&self) -> bool {
        !std::fs::read_dir(&self.root).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".bertbrowser-rename-")
        })
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = DirectoryRemoval::remove_tree(&self.root.to_string_lossy());
    }
}

fn f(path: &str) -> RenameSource {
    RenameSource::file(path)
}

fn d(path: &str) -> RenameSource {
    RenameSource::dir(path)
}

fn assert_content(path: &str, expected: &str) {
    assert!(Path::new(path).is_file(), "expected a file at {path}");
    assert_eq!(expected, std::fs::read_to_string(path).unwrap(), "{path}");
}

// --- the ordinary cases ---

#[test]
fn one_file_is_renamed_in_place() {
    let x = Fixture::new();
    let source = x.file("hello", &["a.txt"]);

    let outcome = x.run(&[f(&source)], "b.txt");

    assert!(outcome.failed.is_empty());
    assert_eq!(1, outcome.completed.len());
    assert_content(&x.p(&["b.txt"]), "hello");
    assert!(!Path::new(&source).exists());
}

#[test]
fn one_folder_is_renamed_with_its_contents() {
    let x = Fixture::new();
    let source = x.dir(&["stuff"]);
    x.file("inner", &["stuff", "deep", "file.txt"]);

    let outcome = x.run(&[d(&source)], "things");

    assert!(outcome.failed.is_empty());
    assert_content(&x.p(&["things", "deep", "file.txt"]), "inner");
    assert!(!Path::new(&source).exists());
}

#[test]
fn several_files_are_numbered_and_keep_their_contents() {
    let x = Fixture::new();
    let a = x.file("one", &["a.jpg"]);
    let b = x.file("two", &["b.png"]);

    let outcome = x.run(&[f(&a), f(&b)], "Trip");

    assert!(outcome.failed.is_empty());
    assert_content(&x.p(&["Trip 1.jpg"]), "one");
    assert_content(&x.p(&["Trip 2.png"]), "two");
    assert!(x.nothing_stranded());
}

#[test]
fn changing_only_the_casing_works_for_a_file() {
    let x = Fixture::new();
    let source = x.file("hello", &["readme.md"]);

    let outcome = x.run(&[f(&source)], "README.md");

    assert!(outcome.failed.is_empty());
    assert_content(&x.p(&["README.md"]), "hello");
    assert_eq!(vec!["README.md".to_string()], x.names());
    assert!(x.nothing_stranded());
}

#[test]
fn changing_only_the_casing_works_for_a_folder() {
    let x = Fixture::new();
    let source = x.dir(&["stuff"]);
    x.file("inner", &["stuff", "file.txt"]);

    let outcome = x.run(&[d(&source)], "STUFF");

    assert!(outcome.failed.is_empty());
    assert_eq!(vec!["STUFF".to_string()], x.names());
    assert_content(&x.p(&["STUFF", "file.txt"]), "inner");
    assert!(x.nothing_stranded());
}

// --- names that overlap inside the batch ---

#[test]
fn renaming_a_set_onto_its_own_names_rotates_without_losing_anything() {
    // Every target is a name the batch already holds: "Trip 2" -> "Trip 1" and back.
    let x = Fixture::new();
    let first = x.file("one", &["Trip 1.jpg"]);
    let second = x.file("two", &["Trip 2.jpg"]);

    let outcome = x.run(&[f(&second), f(&first)], "Trip");

    assert!(outcome.failed.is_empty());
    assert_eq!(2, outcome.completed.len());
    assert_content(&x.p(&["Trip 1.jpg"]), "two");
    assert_content(&x.p(&["Trip 2.jpg"]), "one");
    assert!(x.nothing_stranded());
}

#[test]
fn shifting_names_along_keeps_every_file() {
    // a -> "Note 1", and the file already called "Note 1" moves on to "Note 2".
    let x = Fixture::new();
    let a = x.file("alpha", &["a.txt"]);
    let note1 = x.file("beta", &["Note 1.txt"]);

    let outcome = x.run(&[f(&a), f(&note1)], "Note");

    assert!(outcome.failed.is_empty());
    assert_content(&x.p(&["Note 1.txt"]), "alpha");
    assert_content(&x.p(&["Note 2.txt"]), "beta");
    assert!(x.nothing_stranded());
}

#[test]
fn renaming_folders_onto_each_others_names_keeps_both_trees() {
    let x = Fixture::new();
    let a = x.dir(&["Work 1"]);
    let b = x.dir(&["Work 2"]);
    x.file("in-a", &["Work 1", "a.txt"]);
    x.file("in-b", &["Work 2", "b.txt"]);

    let outcome = x.run(&[d(&b), d(&a)], "Work");

    assert!(outcome.failed.is_empty());
    assert_content(&x.p(&["Work 1", "b.txt"]), "in-b");
    assert_content(&x.p(&["Work 2", "a.txt"]), "in-a");
    assert!(x.nothing_stranded());
}

// --- nothing is ever overwritten ---

#[test]
fn a_name_taken_by_something_unselected_is_refused_and_leaves_both_files_alone() {
    let x = Fixture::new();
    let source = x.file("mine", &["a.txt"]);
    x.file("theirs", &["taken.txt"]);

    let outcome = x.run(&[f(&source)], "taken.txt");

    assert!(outcome.completed.is_empty());
    assert_content(&x.p(&["taken.txt"]), "theirs"); // untouched
    assert_content(&source, "mine");
}

#[test]
fn a_name_taken_after_the_plan_was_made_fails_that_item_without_overwriting() {
    // The plan is built while the dialog is open; disk can change under it, so the executor
    // checks again rather than trusting what the planner saw.
    let x = Fixture::new();
    let source = x.file("mine", &["a.txt"]);
    let plan = RenamePlanner::local().plan_simple(&[f(&source)], "b.txt");
    x.file("theirs", &["b.txt"]); // appears after planning

    let outcome = RenameExecutor.execute(&plan);

    assert!(outcome.completed.is_empty());
    assert_eq!(1, outcome.failed.len());
    assert_content(&x.p(&["b.txt"]), "theirs");
    assert_content(&source, "mine");
}

#[test]
fn a_missing_item_fails_without_stopping_the_rest() {
    let x = Fixture::new();
    let a = x.file("one", &["a.txt"]);
    let b = x.file("two", &["b.txt"]);
    let plan = RenamePlanner::local().plan_simple(&[f(&a), f(&b)], "Note");
    std::fs::remove_file(&a).unwrap(); // disappears after planning

    let outcome = RenameExecutor.execute(&plan);

    assert_eq!(1, outcome.failed.len());
    assert_content(&x.p(&["Note 2.txt"]), "two");
    assert!(x.nothing_stranded());
}

#[test]
fn a_no_op_rename_writes_nothing() {
    let x = Fixture::new();
    let source = x.file("hello", &["a.txt"]);

    let outcome = x.run(&[f(&source)], "a.txt");

    assert!(outcome.completed.is_empty());
    assert!(outcome.failed.is_empty());
    assert_content(&source, "hello");
}

// --- undo ---

#[test]
fn undo_puts_every_name_back() {
    let x = Fixture::new();
    let a = x.file("one", &["a.jpg"]);
    let b = x.file("two", &["b.png"]);
    let outcome = x.run(&[f(&a), f(&b)], "Trip");

    let undo = RenameExecutor.undo(&outcome);

    assert!(undo.failed.is_empty());
    assert_content(&a, "one");
    assert_content(&b, "two");
    assert!(!Path::new(&x.p(&["Trip 1.jpg"])).exists());
    assert!(x.nothing_stranded());
}

#[test]
fn undo_reverses_a_rotation() {
    // The contents swapped places; undoing has to swap them back, staging and all.
    let x = Fixture::new();
    let first = x.file("one", &["Trip 1.jpg"]);
    let second = x.file("two", &["Trip 2.jpg"]);
    let outcome = x.run(&[f(&second), f(&first)], "Trip");
    assert_content(&x.p(&["Trip 1.jpg"]), "two");

    let undo = RenameExecutor.undo(&outcome);

    assert!(undo.failed.is_empty());
    assert_content(&x.p(&["Trip 1.jpg"]), "one");
    assert_content(&x.p(&["Trip 2.jpg"]), "two");
    assert!(x.nothing_stranded());
}

#[test]
fn undo_puts_a_folder_back_with_its_contents() {
    let x = Fixture::new();
    let source = x.dir(&["stuff"]);
    x.file("inner", &["stuff", "deep", "file.txt"]);
    let outcome = x.run(&[d(&source)], "things");

    let undo = RenameExecutor.undo(&outcome);

    assert!(undo.failed.is_empty());
    assert_content(&x.p(&["stuff", "deep", "file.txt"]), "inner");
    assert!(!Path::new(&x.p(&["things"])).exists());
}

#[test]
fn undo_refuses_to_overwrite_something_that_took_the_old_name() {
    let x = Fixture::new();
    let source = x.file("mine", &["a.txt"]);
    let outcome = x.run(&[f(&source)], "b.txt");
    x.file("someone else's", &["a.txt"]); // the old name is taken again

    let undo = RenameExecutor.undo(&outcome);

    assert!(undo.completed.is_empty());
    assert_eq!(1, undo.failed.len());
    assert_content(&x.p(&["a.txt"]), "someone else's");
    assert_content(&x.p(&["b.txt"]), "mine");
}

#[test]
fn undo_reports_an_item_that_is_no_longer_there() {
    let x = Fixture::new();
    let source = x.file("mine", &["a.txt"]);
    let outcome = x.run(&[f(&source)], "b.txt");
    std::fs::remove_file(x.p(&["b.txt"])).unwrap();

    let undo = RenameExecutor.undo(&outcome);

    assert!(undo.completed.is_empty());
    assert_eq!(1, undo.failed.len());
}

#[test]
fn undo_of_a_no_op_rename_has_nothing_to_do() {
    assert!(!RenameOutcome::empty().can_undo());
}

// --- meta-test: the stranding check can actually fail ---

#[test]
fn the_staging_check_notices_a_leftover_temporary_name() {
    let x = Fixture::new();
    x.file("x", &[".bertbrowser-rename-deadbeef"]);

    assert!(!x.nothing_stranded());
}
