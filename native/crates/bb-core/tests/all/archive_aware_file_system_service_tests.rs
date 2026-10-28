// Port of tests/BertBrowser.Core.Tests/ArchiveAwareFileSystemServiceTests.cs.
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bb_core::FileTime;
use bb_core::archives::{ArchiveAwareFileSystemService, ArchiveBrowser, LibraryArchiveReader};
use bb_core::attributes::HIDDEN;
use bb_core::fs::{DriveInfo, FileSystemService, LocalFileSystem, SubdirectoryPresence};
use bb_core::models::FileEntry;
use bb_test_support::archive_support::{TempRoot, write_zip};

/// Records what it was asked, and answers nothing.
#[derive(Default)]
struct RecordingFileSystem {
    listed: Mutex<Vec<String>>,
    probed: Mutex<Vec<String>>,
    drive_calls: AtomicUsize,
}

impl FileSystemService for RecordingFileSystem {
    fn list_directory(&self, path: &str) -> io::Result<Vec<FileEntry>> {
        self.listed.lock().unwrap().push(path.to_string());
        Ok(Vec::new())
    }

    fn probe_subdirectories(&self, path: &str) -> SubdirectoryPresence {
        self.probed.lock().unwrap().push(path.to_string());
        SubdirectoryPresence::default()
    }

    fn drives(&self) -> Vec<DriveInfo> {
        self.drive_calls.fetch_add(1, Ordering::SeqCst);
        Vec::new()
    }
}

struct Fixture {
    root: TempRoot,
    inner: Arc<RecordingFileSystem>,
    service: ArchiveAwareFileSystemService,
}

impl Fixture {
    fn new() -> Fixture {
        let inner = Arc::new(RecordingFileSystem::default());
        let service = ArchiveAwareFileSystemService::new(
            inner.clone(),
            Arc::new(LibraryArchiveReader::new()),
        );
        Fixture {
            root: TempRoot::new("bertbrowser-archfs"),
            inner,
            service,
        }
    }

    fn zip(&self, name: &str, entries: &[(&str, &[u8])]) -> String {
        write_zip(&self.root.path(name), entries)
    }

    fn listed(&self) -> Vec<String> {
        self.inner.listed.lock().unwrap().clone()
    }
}

fn sorted_names(rows: &[FileEntry]) -> Vec<String> {
    let mut names: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
    names.sort();
    names
}

/// The reason the callers needed no changes: an ordinary path is simply passed on.
#[test]
fn an_ordinary_path_goes_straight_to_the_inner_service() {
    let f = Fixture::new();

    f.service
        .list_directory("C:\\Users\\Rob\\Documents")
        .unwrap();

    assert_eq!(vec!["C:\\Users\\Rob\\Documents"], f.listed());
}

/// And a virtual one never reaches it, so nothing tries to stat a path that is not there.
#[test]
fn a_virtual_path_never_reaches_the_inner_service() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("src/app.js", b"x")]);

    f.service.list_directory(&format!("{zip}\\src")).unwrap();

    assert!(f.listed().is_empty());
}

#[test]
fn lists_the_archive_root_as_if_it_were_a_folder() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("readme.txt", b"hello"), ("src/app.js", b"x")]);

    let rows = f.service.list_directory(&zip).unwrap();

    assert_eq!(vec!["readme.txt", "src"], sorted_names(&rows));
    let src = rows.iter().find(|r| r.name == "src").unwrap();
    assert_eq!(format!("{zip}\\src"), src.full_path);
    assert!(src.is_directory);
}

#[test]
fn lists_a_folder_inside_the_archive() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("src/app.js", b"x"), ("src/lib/util.js", b"y")]);

    let rows = f.service.list_directory(&format!("{zip}\\src")).unwrap();

    assert_eq!(vec!["app.js", "lib"], sorted_names(&rows));
}

/// A directory inside an archive carries its exact size, which is what stops the file list asking
/// dir_size_cache about a path that could never have a row.
#[test]
fn a_folder_inside_an_archive_carries_its_exact_size_rather_than_minus_one() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[
            ("src/a.txt", "x".repeat(40).as_bytes()),
            ("src/b.txt", "y".repeat(60).as_bytes()),
        ],
    );

    let rows = f.service.list_directory(&zip).unwrap();
    assert_eq!(1, rows.len());
    let src = &rows[0];

    assert!(src.is_directory);
    assert_eq!(100, src.size_bytes);
}

/// The attribute field holds a DOS byte or a Unix mode depending on the writing tool, and "Show
/// hidden items" filters on this. Map it and most of a tarball disappears under the default
/// setting; the payoff for getting it right is only a ghosted icon.
#[test]
fn nothing_inside_an_archive_is_hidden() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[
            (".gitignore", b"node_modules"),
            (".config/settings.json", b"{}"),
        ],
    );

    let rows = f.service.list_directory(&zip).unwrap();

    assert_eq!(vec![".config", ".gitignore"], sorted_names(&rows));
    assert!(rows.iter().all(|r| r.attributes & HIDDEN == 0));
}

#[test]
fn an_entry_with_no_timestamp_renders_blank_rather_than_sixteen_oh_one() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("src/app.js", b"x")]);

    // "src" is synthesized from a path prefix, so it carries no timestamp of its own.
    let rows = f.service.list_directory(&zip).unwrap();
    assert_eq!(1, rows.len());

    assert_eq!(FileTime::UNKNOWN, rows[0].modified_utc);
}

#[test]
fn a_damaged_archive_throws_the_error_the_file_list_already_renders() {
    let f = Fixture::new();
    let path = f.root.make("broken.zip", b"this is not a zip file at all");

    let error = f.service.list_directory(&path).unwrap_err();
    assert!(error.to_string().to_lowercase().contains("archive"));
}

#[test]
fn a_folder_that_is_not_in_the_archive_is_not_found() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("src/app.js", b"x")]);

    let error = f
        .service
        .list_directory(&format!("{zip}\\nope"))
        .unwrap_err();
    assert_eq!(io::ErrorKind::NotFound, error.kind());
}

#[test]
fn can_list_answers_for_both_kinds_of_path() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("src/app.js", b"x")]);

    assert!(f.service.can_list(&f.root.root_str()));
    assert!(f.service.can_list(&zip));
    assert!(f.service.can_list(&format!("{zip}\\src")));
    assert!(!f.service.can_list(&format!("{zip}\\nope")));
    assert!(!f.service.can_list(&f.root.path("missing")));
}

/// A damaged archive is still navigable: the list shows the banner. Refusing at the gate would
/// mean an archive open on the UI thread, which is what the two-stage gate exists to avoid.
#[test]
fn a_damaged_archive_is_still_navigable_so_the_banner_can_explain() {
    let f = Fixture::new();
    let path = f.root.make("broken.zip", b"not a zip");

    assert!(f.service.can_list(&path));
}

/// The probe stays consistent with the listing rather than being made to lie in order to keep
/// archives out of the folder tree. What actually keeps them out is structural: the tree builds
/// nodes from the listing filtered on is_directory, and an archive is a file.
#[test]
fn probe_answers_for_the_archive_file_the_way_the_listing_does() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("src/app.js", b"x")]);

    assert!(f.service.probe_subdirectories(&zip).any);
    assert!(f.inner.probed.lock().unwrap().is_empty());
}

/// The structural guarantee itself: an archive is never a directory row.
#[test]
fn an_archive_is_a_file_in_its_parent_listing_so_the_tree_cannot_node_it() {
    let f = Fixture::new();
    f.zip("a.zip", &[("src/app.js", b"x")]);
    let real = LocalFileSystem;

    let rows = real.list_directory(&f.root.root_str()).unwrap();
    let row = rows.iter().find(|e| e.name == "a.zip").unwrap();

    assert!(!row.is_directory);
}

#[test]
fn probe_answers_from_the_index_inside_an_archive() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("src/lib/util.js", b"x"), ("src/app.js", b"y")]);

    let presence = f.service.probe_subdirectories(&format!("{zip}\\src"));

    assert!(presence.any);
    assert!(presence.any_visible);
}

#[test]
fn drives_always_come_from_the_inner_service() {
    let f = Fixture::new();
    f.service.drives();
    assert_eq!(1, f.inner.drive_calls.load(Ordering::SeqCst));
}
