// Port of tests/BertBrowser.Core.Tests/UniquePathTests.cs: the "name (2)" rule.
use std::collections::HashSet;

use bb_paths::UniquePath;

#[derive(Default)]
struct Disk {
    directories: HashSet<String>,
    files: HashSet<String>,
}

impl Disk {
    fn dir(&mut self, p: &str) {
        self.directories.insert(p.to_lowercase());
    }
    fn file(&mut self, p: &str) {
        self.files.insert(p.to_lowercase());
    }
    fn unique(&self, path: &str, is_directory: bool) -> String {
        UniquePath::for_path(
            path,
            is_directory,
            |p| self.directories.contains(&p.to_lowercase()),
            |p| self.files.contains(&p.to_lowercase()),
        )
    }
}

// --- the ordinary cases ---

#[test]
fn a_free_name_is_handed_back_unchanged() {
    let disk = Disk::default();
    assert_eq!(r"C:\dir\notes.txt", disk.unique(r"C:\dir\notes.txt", false));
}

#[test]
fn a_file_is_numbered_before_its_extension() {
    let mut disk = Disk::default();
    disk.file(r"C:\dir\notes.txt");
    assert_eq!(
        r"C:\dir\notes (2).txt",
        disk.unique(r"C:\dir\notes.txt", false)
    );
}

#[test]
fn a_folder_numbers_the_whole_name_because_it_has_no_extension_to_keep() {
    let mut disk = Disk::default();
    disk.dir(r"C:\dir\My.Project");
    assert_eq!(
        r"C:\dir\My.Project (2)",
        disk.unique(r"C:\dir\My.Project", true)
    );
}

#[test]
fn the_numbering_skips_past_names_that_are_also_taken() {
    let mut disk = Disk::default();
    disk.file(r"C:\dir\notes.txt");
    disk.file(r"C:\dir\notes (2).txt");
    disk.file(r"C:\dir\notes (3).txt");
    assert_eq!(
        r"C:\dir\notes (4).txt",
        disk.unique(r"C:\dir\notes.txt", false)
    );
}

// --- files and folders share one namespace ---

#[test]
fn a_folder_in_the_way_of_a_file_still_counts_as_taken() {
    let mut disk = Disk::default();
    disk.dir(r"C:\dir\notes.txt");
    assert_ne!(r"C:\dir\notes.txt", disk.unique(r"C:\dir\notes.txt", false));
}

#[test]
fn the_number_goes_by_what_is_being_placed_not_by_what_is_in_the_way() {
    // A folder named "notes.txt" blocking a new *file* called "notes.txt" must still give
    // "notes (2).txt". Probe the path instead of being told, and this comes back as
    // "notes.txt (2)".
    let mut disk = Disk::default();
    disk.dir(r"C:\dir\notes.txt");
    assert_eq!(
        r"C:\dir\notes (2).txt",
        disk.unique(r"C:\dir\notes.txt", false)
    );
}
