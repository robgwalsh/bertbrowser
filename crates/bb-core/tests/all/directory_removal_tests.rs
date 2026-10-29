// Port of tests/BertBrowser.Core.Tests/DirectoryRemovalTests.cs. Erasing a tree, on real files. The
// reason this exists at all is a junction: swap `remove_tree` for `remove_dir_all` and every
// junction test below goes red, the first because that call fails and the rest because of what it
// has already destroyed by the time it does.
use std::path::{Path, PathBuf};
use std::process::Command;

use bb_core::attributes::{DIRECTORY, REPARSE_POINT};
use bb_core::fs::DirectoryRemoval;
use bb_test_support::unique_temp_path;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-rm", "");
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = DirectoryRemoval::remove_tree(&self.root.to_string_lossy());
    }
}

/// A directory junction, which, unlike a symlink, an ordinary account may create. Shelled out
/// because the standard library exposes no API for one; `index_crawler_tests` does the same.
pub fn try_create_junction(link: &Path, target: &Path) -> bool {
    let _ = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output();
    match std::fs::symlink_metadata(link) {
        Ok(meta) => {
            use std::os::windows::fs::MetadataExt;
            // The attribute bits: `FileType::is_dir` is false for a junction, which it reads as
            // a symlink.
            meta.file_attributes() & (DIRECTORY | REPARSE_POINT) == (DIRECTORY | REPARSE_POINT)
        }
        Err(_) => false,
    }
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn an_ordinary_tree_goes_entirely() {
    let f = Fixture::new();
    let tree = f.dir(&["tree"]);
    f.file("a", &["tree", "a.txt"]);
    f.file("b", &["tree", "one", "b.txt"]);
    f.file("c", &["tree", "one", "two", "three", "c.txt"]);

    DirectoryRemoval::remove_tree(&s(&tree)).unwrap();

    assert!(!tree.exists());
}

#[test]
fn a_tree_containing_a_junction_goes_entirely_and_the_target_survives() {
    let f = Fixture::new();
    let outside = f.dir(&["outside"]);
    let bystander = f.file("keep me", &["outside", "bystander.txt"]);

    let tree = f.dir(&["tree"]);
    f.file("a", &["tree", "a.txt"]);
    f.file("b", &["tree", "deep", "b.txt"]);
    assert!(
        try_create_junction(&f.p(&["tree", "deep", "link"]), &outside),
        "junction was not created"
    );

    DirectoryRemoval::remove_tree(&s(&tree)).unwrap();

    assert!(
        !tree.exists(),
        "the tree survived a removal that reported no error"
    );
    assert!(
        outside.exists(),
        "the junction was followed and its target removed"
    );
    assert_eq!(std::fs::read_to_string(bystander).unwrap(), "keep me");
}

#[test]
fn a_junction_given_directly_is_removed_as_one_entry() {
    let f = Fixture::new();
    let outside = f.dir(&["outside"]);
    let bystander = f.file("keep me", &["outside", "bystander.txt"]);
    let link = f.p(&["link"]);
    assert!(
        try_create_junction(&link, &outside),
        "junction was not created"
    );

    DirectoryRemoval::remove_tree(&s(&link)).unwrap();

    assert!(!link.exists());
    assert_eq!(std::fs::read_to_string(bystander).unwrap(), "keep me");
}

/// Meta-test: proves the junctions above are real and traversable, so the assertions are passing
/// because the link was declined rather than because it never worked.
#[test]
fn meta_a_junction_is_real_and_leads_somewhere() {
    let f = Fixture::new();
    let outside = f.dir(&["outside"]);
    f.file("keep me", &["outside", "bystander.txt"]);
    f.dir(&["tree"]);
    assert!(
        try_create_junction(&f.p(&["tree", "link"]), &outside),
        "junction was not created"
    );

    let meta = std::fs::symlink_metadata(f.p(&["tree", "link"])).unwrap();
    assert!(DirectoryRemoval::is_link(&meta));
    let through = std::fs::read_dir(f.p(&["tree", "link"]))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(through.contains(&"bystander.txt".to_string()));
}

#[test]
fn a_missing_tree_throws_rather_than_pretending_to_have_worked() {
    let f = Fixture::new();
    assert!(DirectoryRemoval::remove_tree(&s(&f.p(&["never-existed"]))).is_err());
}
