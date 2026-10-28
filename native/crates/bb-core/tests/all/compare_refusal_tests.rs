// Port of tests/BertBrowser.Core.Tests/CompareRefusalTests.cs
//
// What a comparison refuses before it reads anything. Each of these is a data-safety rule rather
// than a convenience: a comparison run on the wrong pair is what makes "delete what is only on the
// right" delete the wrong thing, and it does it silently.
use std::collections::{HashMap, HashSet};

use bb_core::compare::{CompareProbe, CompareRefusal};
use bb_paths::fold_upper;

const LEFT: &str = r"C:\Work\left";
const RIGHT: &str = r"C:\Work\right";

#[derive(Default)]
struct FakeProbe {
    dirs: HashSet<String>,
    files: HashSet<String>,
    links: HashMap<String, String>,
}

impl FakeProbe {
    fn add_directory(&mut self, path: &str) {
        self.dirs.insert(fold_upper(path));
    }
    fn add_file(&mut self, path: &str) {
        self.files.insert(fold_upper(path));
    }
    fn add_link(&mut self, link: &str, target: &str) {
        self.links.insert(fold_upper(link), target.to_string());
    }
}

impl CompareProbe for FakeProbe {
    fn directory_exists(&self, path: &str) -> bool {
        self.dirs.contains(&fold_upper(path))
    }
    fn file_exists(&self, path: &str) -> bool {
        self.files.contains(&fold_upper(path))
    }
    fn resolve_final_path(&self, path: &str) -> String {
        self.links
            .get(&fold_upper(path))
            .cloned()
            .unwrap_or_else(|| path.to_string())
    }
}

fn probe() -> FakeProbe {
    let mut probe = FakeProbe::default();
    probe.add_directory(LEFT);
    probe.add_directory(RIGHT);
    probe
}

#[test]
fn two_ordinary_folders_are_accepted() {
    assert_eq!(None, CompareRefusal::check(LEFT, RIGHT, &probe()));
}

#[test]
fn an_empty_side_is_refused() {
    assert!(CompareRefusal::check("", RIGHT, &probe()).is_some());
    assert!(CompareRefusal::check(LEFT, "   ", &probe()).is_some());
}

#[test]
fn a_path_that_is_not_a_folder_is_refused() {
    let mut probe = probe();
    probe.add_file(r"C:\Work\notes.txt");

    assert!(CompareRefusal::check(LEFT, r"C:\Work\notes.txt", &probe).is_some());
}

/// A path inside an archive is a real Windows path as far as string handling is concerned, so it
/// canonicalizes happily and lands inside the range scan for the container's own folder. Every
/// other subtree feature in the app refuses it by name for the same reason.
#[test]
fn a_folder_inside_an_archive_is_refused() {
    let mut probe = probe();
    probe.add_file(r"C:\Work\bundle.zip");
    probe.add_directory(r"C:\Work\bundle.zip\src");

    assert!(CompareRefusal::check(LEFT, r"C:\Work\bundle.zip\src", &probe).is_some());
    assert!(CompareRefusal::check(r"C:\Work\bundle.zip\src", RIGHT, &probe).is_some());
}

#[test]
fn a_folder_compared_with_itself_is_refused() {
    assert!(CompareRefusal::check(LEFT, LEFT, &probe()).is_some());
    assert!(CompareRefusal::check(LEFT, r"c:\work\LEFT", &probe()).is_some());
}

/// Every file under the inner folder would appear on both sides at two different relative paths,
/// and the outer folder would be "only on the left" of itself.
#[test]
fn a_folder_compared_with_its_own_subtree_is_refused() {
    let mut probe = probe();
    probe.add_directory(r"C:\Work\left\inner");

    assert!(CompareRefusal::check(LEFT, r"C:\Work\left\inner", &probe).is_some());
    assert!(CompareRefusal::check(r"C:\Work\left\inner", LEFT, &probe).is_some());
}

/// The check neither path admits to. Both look like unrelated folders and only resolving the
/// junction shows that one is inside the other, which is exactly why the transfer planner checks
/// containment twice, and why this does too.
#[test]
fn a_junction_that_puts_one_side_inside_the_other_is_refused() {
    let mut probe = probe();
    probe.add_directory(r"C:\Work\mirror");
    probe.add_link(r"C:\Work\mirror", r"C:\Work\left\inner");

    assert!(CompareRefusal::check(LEFT, r"C:\Work\mirror", &probe).is_some());
}
