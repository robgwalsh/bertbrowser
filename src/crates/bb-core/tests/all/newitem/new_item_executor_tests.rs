// Port of tests/BertBrowser.Core.Tests/NewItemExecutorTests.cs. Creates real folders and files on
// a real disk. The failure worth catching here is a silent one: `create_dir` has a sibling that
// succeeds on a folder already there, so a create that merely reported success could hand the user
// somebody else's folder. Every test asserts on what is on disk afterwards rather than on the
// outcome record alone.
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use bb_core::attributes::{HIDDEN, READONLY};
use bb_core::fs::DirectoryRemoval;
use bb_core::newitem::{NewItemExecutor, NewItemKind, NewItemOutcome, NewItemPlanner};
use bb_test_support::unique_temp_path;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-newitem", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
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

    fn run(&self, name: &str, kind: NewItemKind, template: Option<&str>) -> NewItemOutcome {
        NewItemExecutor.execute(&NewItemPlanner::local().plan(&self.root(), name, kind, template))
    }

    /// The names the root holds, sorted.
    fn root_holds(&self) -> Vec<String> {
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
        // A test that made something read-only would otherwise strand the temp tree.
        clear_read_only(&self.root);
        let _ = DirectoryRemoval::remove_tree(&self.root.to_string_lossy());
    }
}

fn clear_read_only(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            clear_read_only(&path);
        } else if let Ok(meta) = std::fs::metadata(&path) {
            let mut permissions = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(&path, permissions);
        }
    }
}

fn assert_content(path: &str, expected: &str) {
    assert_eq!(expected, std::fs::read_to_string(path).unwrap(), "{path}");
}

fn names(list: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

// --- the ordinary cases ---

#[test]
fn a_folder_is_created() {
    let x = Fixture::new();
    let outcome = x.run("Reports", NewItemKind::Folder, None);
    assert_eq!(Some(x.p(&["Reports"])), outcome.created_path);
    assert!(Path::new(&x.p(&["Reports"])).is_dir());
    assert_eq!(names(&["Reports"]), x.root_holds());
}

#[test]
fn a_new_file_is_created_and_is_actually_empty() {
    let x = Fixture::new();
    let outcome = x.run("notes.txt", NewItemKind::File, None);
    assert_eq!(Some(x.p(&["notes.txt"])), outcome.created_path);
    assert_content(&x.p(&["notes.txt"]), "");
    // Created and closed — nothing may still hold a handle on it.
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(x.p(&["notes.txt"]))
        .unwrap()
        .write_all(b"x")
        .unwrap();
}

#[test]
fn a_template_is_copied_contents_and_all() {
    let x = Fixture::new();
    x.file("Dear {name},", &["templates", "letter.rtf"]);
    let template = x.p(&["templates", "letter.rtf"]);
    let outcome = x.run("letter.rtf", NewItemKind::File, Some(&template));
    assert!(outcome.created_path.is_some());
    assert_content(&x.p(&["letter.rtf"]), "Dear {name},");
}

#[test]
fn a_copied_template_does_not_inherit_being_hidden_or_read_only() {
    // The shipped templates live under %APPDATA%\Microsoft\Windows\Templates and are commonly
    // both; the file the user asked for should be neither.
    let x = Fixture::new();
    x.file("x", &["templates", "letter.rtf"]);
    let template = x.p(&["templates", "letter.rtf"]);
    set_attributes(&template, HIDDEN | READONLY);

    x.run("letter.rtf", NewItemKind::File, Some(&template));

    let attributes = std::fs::metadata(x.p(&["letter.rtf"]))
        .unwrap()
        .file_attributes();
    assert_eq!(0, attributes & HIDDEN);
    assert_eq!(0, attributes & READONLY);
}

fn set_attributes(path: &str, attributes: u32) {
    use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: a NUL-terminated path.
    assert_ne!(0, unsafe { SetFileAttributesW(wide.as_ptr(), attributes) });
}

// --- nothing is overwritten, and nothing existing is adopted ---

#[test]
fn a_folder_that_appeared_since_the_plan_fails_rather_than_being_adopted() {
    // The plan is built while the dialog sits open, so it is advisory. Drop the executor's
    // existence check and this goes green with the user's folder silently handed back.
    let x = Fixture::new();
    let plan = NewItemPlanner::local().plan(&x.root(), "Reports", NewItemKind::Folder, None);
    std::fs::create_dir_all(x.p(&["Reports"])).unwrap();
    x.file("theirs", &["Reports", "already-here.txt"]);

    let outcome = NewItemExecutor.execute(&plan);

    assert_eq!(None, outcome.created_path);
    assert!(outcome.failed.is_some());
    assert_content(&x.p(&["Reports", "already-here.txt"]), "theirs");
}

#[test]
fn a_file_that_appeared_since_the_plan_keeps_its_contents() {
    let x = Fixture::new();
    let plan = NewItemPlanner::local().plan(&x.root(), "notes.txt", NewItemKind::File, None);
    x.file("theirs", &["notes.txt"]);

    let outcome = NewItemExecutor.execute(&plan);

    assert_eq!(None, outcome.created_path);
    assert!(outcome.failed.is_some());
    assert_content(&x.p(&["notes.txt"]), "theirs");
}

#[test]
fn a_template_that_went_missing_since_the_plan_fails_and_writes_nothing() {
    let x = Fixture::new();
    x.file("x", &["templates", "letter.rtf"]);
    let template = x.p(&["templates", "letter.rtf"]);
    let plan =
        NewItemPlanner::local().plan(&x.root(), "letter.rtf", NewItemKind::File, Some(&template));
    std::fs::remove_file(&template).unwrap();

    let outcome = NewItemExecutor.execute(&plan);

    assert!(outcome.failed.is_some());
    assert!(!Path::new(&x.p(&["letter.rtf"])).exists());
}

#[test]
fn a_refused_plan_leaves_the_folder_exactly_as_it_was() {
    let x = Fixture::new();
    x.file("mine", &["notes.txt"]);
    let plan = NewItemPlanner::local().plan(&x.root(), "notes.txt", NewItemKind::File, None);
    assert!(!plan.has_work());

    NewItemExecutor.execute(&plan);

    assert_eq!(names(&["notes.txt"]), x.root_holds());
    assert_content(&x.p(&["notes.txt"]), "mine");
}

// --- meta-test: the "exactly these names" check can actually fail ---

#[test]
fn the_root_listing_check_notices_an_extra_entry() {
    let x = Fixture::new();
    x.file("x", &["notes.txt"]);
    x.file("y", &["unexpected.txt"]);
    assert_ne!(names(&["notes.txt"]), x.root_holds());
}
