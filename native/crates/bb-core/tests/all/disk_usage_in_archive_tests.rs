// Port of tests/BertBrowser.Core.Tests/DiskUsageInArchiveTests.cs.
//
// Disk usage pointed at an archive. The point of these is the availability, not the arithmetic:
// every size inside a container is exact, so this is the one place the view is never approximate,
// and the way that could regress is silently, with a breakdown asking dir_size_cache, missing on
// every folder, and then blaming the index.
use std::sync::Arc;

use bb_core::archives::{ArchiveAwareFileSystemService, LibraryArchiveReader};
use bb_core::cancel::CancelToken;
use bb_core::data::{DirSizeRepository, FsIndexRepository};
use bb_core::diskusage::{ArchiveUsage, DiskUsageAvailability, DiskUsageService};
use bb_core::fs::LocalFileSystem;
use bb_core::mft::NullMftIndex;
use bb_test_support::TempDb;
use bb_test_support::archive_support::{TempRoot, write_zip};

struct Fixture {
    db: TempDb,
    _root: TempRoot,
    zip: String,
    service: DiskUsageService,
}

impl Fixture {
    fn new() -> Fixture {
        let root = TempRoot::new("bertbrowser-duarch");
        let zip = write_zip(
            &root.path("a.zip"),
            &[
                ("readme.txt", "r".repeat(41).as_bytes()),
                ("src/app.js", "a".repeat(22).as_bytes()),
                ("src/lib/util.js", "u".repeat(22).as_bytes()),
                ("docs/guide.md", "g".repeat(33).as_bytes()),
            ],
        );
        let db = TempDb::new();
        let archives = Arc::new(ArchiveAwareFileSystemService::new(
            Arc::new(LocalFileSystem),
            Arc::new(LibraryArchiveReader::new()),
        ));
        let service = DiskUsageService::new(
            Arc::new(FsIndexRepository::new(db.db())),
            Arc::new(DirSizeRepository::new(db.db())),
            archives.clone(),
            Arc::new(NullMftIndex),
            Some(Arc::new(ArchiveUsage(archives))),
        );
        Fixture {
            db,
            _root: root,
            zip,
            service,
        }
    }

    fn fs_entry_rows(&self) -> i64 {
        let db = self.db.db();
        let conn = db.open().unwrap();
        conn.query_row("SELECT COUNT(*) FROM fs_entry", [], |r| r.get(0))
            .unwrap()
    }
}

/// Ready, not NoSizeData, and every folder sized. Nothing here depends on the MFT index, so
/// there is no "not indexed yet" state for this view to be in.
#[test]
fn a_breakdown_of_an_archive_is_ready_and_fully_sized() {
    let f = Fixture::new();
    let breakdown = f
        .service
        .breakdown(&f.zip, true, &CancelToken::none())
        .unwrap();

    assert_eq!(DiskUsageAvailability::Ready, breakdown.availability);
    assert_eq!(0, breakdown.unknown_child_count);
    assert!(breakdown.children.iter().all(|c| c.size_bytes.is_some()));

    let size = |name: &str| {
        breakdown
            .children
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("{name}"))
            .size_bytes
    };
    assert_eq!(Some(44), size("src"));
    assert_eq!(Some(41), size("readme.txt"));
    assert_eq!(Some(33), size("docs"));
    assert_eq!(Some(118), breakdown.total_bytes);
}

/// With every child measured there is nothing left over, so the remainder is a real number
/// rather than the None that "some child is unknown" produces.
#[test]
fn nothing_is_unaccounted_for_in_an_archive() {
    let f = Fixture::new();
    let breakdown = f
        .service
        .breakdown(&f.zip, true, &CancelToken::none())
        .unwrap();

    assert_eq!(Some(0), breakdown.unaccounted_bytes);
}

#[test]
fn drilling_into_a_folder_inside_the_archive_works() {
    let f = Fixture::new();
    let breakdown = f
        .service
        .breakdown(&format!("{}\\src", f.zip), true, &CancelToken::none())
        .unwrap();

    assert_eq!(DiskUsageAvailability::Ready, breakdown.availability);
    let mut names: Vec<&str> = breakdown.children.iter().map(|c| c.name.as_str()).collect();
    names.sort();
    assert_eq!(vec!["app.js", "lib"], names);
    assert_eq!(
        Some(22),
        breakdown
            .children
            .iter()
            .find(|c| c.name == "lib")
            .unwrap()
            .size_bytes
    );
}

/// Answered from the container's own index rather than fs_entry, which knows nothing about
/// archive contents, and must never be taught, since its rows are PathKey-keyed.
#[test]
fn largest_files_come_from_the_archive_rather_than_the_index() {
    let f = Fixture::new();
    let outcome = f
        .service
        .largest_files(Some(&f.zip), 10, true, &CancelToken::none())
        .unwrap();

    assert_eq!(DiskUsageAvailability::Ready, outcome.availability);
    let names: Vec<&str> = outcome.files.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(vec!["readme.txt", "guide.md", "util.js", "app.js"], names);
    assert_eq!(41, outcome.files[0].size_bytes);
}

#[test]
fn the_limit_is_honoured() {
    let f = Fixture::new();
    let outcome = f
        .service
        .largest_files(Some(&f.zip), 2, true, &CancelToken::none())
        .unwrap();

    assert_eq!(2, outcome.files.len());
}

/// The hard invariant: nothing about this may write a virtual path into the index.
#[test]
fn no_virtual_path_reaches_the_index() {
    let f = Fixture::new();
    f.service
        .breakdown(&f.zip, true, &CancelToken::none())
        .unwrap();
    f.service
        .largest_files(Some(&f.zip), 10, true, &CancelToken::none())
        .unwrap();

    assert_eq!(0, f.fs_entry_rows());
}
