// Port of tests/BertBrowser.Core.Tests/DiskUsageServiceTests.cs
//
// Real temp database, real temp directory tree. The point of these is the seam between the two:
// file sizes come from the enumeration and folder totals come from dir_size_cache, and a folder
// with no cached row has to survive all the way out as None.
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use bb_core::FileTime;
use bb_core::cancel::CancelToken;
use bb_core::data::{DirSizeRepository, DirSizeResult, FsEntryRow, FsIndexRepository};
use bb_core::diskusage::{DiskUsageAvailability, DiskUsageIndex, DiskUsageService};
use bb_core::fs::LocalFileSystem;
use bb_core::mft::NullMftIndex;
use bb_paths::PathKey;
use bb_test_support::{TempDb, unique_temp_path};

struct Fixture {
    _db: TempDb,
    root: PathBuf,
    index: Arc<FsIndexRepository>,
    dir_sizes: Arc<DirSizeRepository>,
}

impl Fixture {
    fn new() -> Fixture {
        let db = TempDb::new();
        let index = Arc::new(FsIndexRepository::new(db.db()));
        let dir_sizes = Arc::new(DirSizeRepository::new(db.db()));
        let root = unique_temp_path("bertbrowser-du", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            _db: db,
            root,
            index,
            dir_sizes,
        }
    }

    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn service(&self, mft: Option<Arc<dyn DiskUsageIndex>>) -> DiskUsageService {
        DiskUsageService::new(
            self.index.clone(),
            self.dir_sizes.clone(),
            Arc::new(LocalFileSystem),
            mft.unwrap_or_else(|| Arc::new(NullMftIndex)),
            None,
        )
    }

    fn make_file(&self, name: &str, bytes: usize) -> String {
        let path = self.root.join(name);
        std::fs::write(&path, vec![0u8; bytes]).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn make_dir(&self, name: &str) -> String {
        let path = self.root.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn cache_size(&self, path: &str, bytes: i64, incomplete: bool) {
        self.dir_sizes
            .upsert_many(&[DirSizeResult {
                path_key: PathKey::canonicalize(path).unwrap(),
                size_bytes: bytes,
                file_count: 1,
                dir_count: 0,
                incomplete,
                computed_utc: FileTime::now(),
            }])
            .unwrap();
    }

    fn breakdown(&self, include_hidden: bool) -> bb_core::diskusage::DiskUsageBreakdown {
        self.service(None)
            .breakdown(&self.root(), include_hidden, &CancelToken::none())
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn set_hidden(path: &str) {
    let out = Command::new("attrib").arg("+h").arg(path).output().unwrap();
    assert!(out.status.success(), "attrib +h failed");
}

fn names(breakdown: &bb_core::diskusage::DiskUsageBreakdown) -> Vec<&str> {
    breakdown.children.iter().map(|c| c.name.as_str()).collect()
}

/// The invariant this whole feature is shaped around: a folder nobody measured comes back as
/// None, not 0. Anything that renders it as a number is claiming it is empty.
#[test]
fn an_unmeasured_folder_is_unknown_not_zero() {
    let f = Fixture::new();
    f.make_dir("Unmeasured");

    let breakdown = f.breakdown(true);

    assert_eq!(1, breakdown.children.len());
    let child = &breakdown.children[0];
    assert_eq!("Unmeasured", child.name);
    assert_eq!(None, child.size_bytes);
    assert_eq!(1, breakdown.unknown_child_count);
}

/// File sizes come from the enumeration, so the file half of a breakdown is complete on a volume
/// nothing has ever indexed — which is what NullMftIndex is here.
#[test]
fn file_sizes_need_no_index_at_all() {
    let f = Fixture::new();
    f.make_file("big.bin", 4_096);
    f.make_file("small.bin", 16);

    let breakdown = f.breakdown(true);

    assert_eq!(vec!["big.bin", "small.bin"], names(&breakdown));
    assert_eq!(Some(4_096), breakdown.children[0].size_bytes);
    assert_eq!(Some(16), breakdown.children[1].size_bytes);
    assert_eq!(DiskUsageAvailability::Ready, breakdown.availability);
}

#[test]
fn cached_folder_totals_are_used_and_sort_largest_first() {
    let f = Fixture::new();
    f.cache_size(&f.make_dir("Small"), 500, false);
    f.cache_size(&f.make_dir("Large"), 900_000, false);
    f.make_file("middling.bin", 1_000);

    let breakdown = f.breakdown(true);

    assert_eq!(vec!["Large", "middling.bin", "Small"], names(&breakdown));
    assert_eq!(Some(900_000), breakdown.children[0].size_bytes);
}

/// Unknowns sort last: they have no area to give, so they belong out of the way rather than
/// interleaved as though they were zero-sized.
#[test]
fn unknown_folders_sort_after_everything_measured() {
    let f = Fixture::new();
    f.make_dir("Unmeasured");
    f.cache_size(&f.make_dir("Measured"), 10, false);
    f.make_file("empty.bin", 0);

    let breakdown = f.breakdown(true);

    assert_eq!("Unmeasured", breakdown.children.last().unwrap().name);
}

#[test]
fn incomplete_totals_are_propagated() {
    let f = Fixture::new();
    f.cache_size(&f.make_dir("Partial"), 42, true);

    let breakdown = f.breakdown(true);

    assert_eq!(1, breakdown.children.len());
    assert!(breakdown.children[0].incomplete);
}

#[test]
fn hidden_children_are_excluded_when_not_requested() {
    let f = Fixture::new();
    let hidden = f.make_file("hidden.bin", 8);
    set_hidden(&hidden);
    f.make_file("visible.bin", 8);

    let all = f.breakdown(true);
    let visible = f.breakdown(false);

    assert_eq!(2, all.children.len());
    assert_eq!(vec!["visible.bin"], names(&visible));
}

/// An unknown child makes the remainder incomputable, and the breakdown has to carry that through
/// rather than quietly counting it as nothing.
#[test]
fn unaccounted_is_null_when_a_child_is_unknown() {
    let f = Fixture::new();
    f.cache_size(&f.root(), 10_000, false);
    f.make_dir("Unmeasured");

    let breakdown = f.breakdown(true);

    assert_eq!(Some(10_000), breakdown.total_bytes);
    assert_eq!(None, breakdown.unaccounted_bytes);
}

#[test]
fn unaccounted_is_the_remainder_when_everything_is_measured() {
    let f = Fixture::new();
    f.cache_size(&f.root(), 10_000, false);
    f.cache_size(&f.make_dir("Sub"), 6_000, false);
    f.make_file("loose.bin", 1_000);

    let breakdown = f.breakdown(true);

    assert_eq!(Some(3_000), breakdown.unaccounted_bytes);
}

// --- Largest files ---

fn row(path: &str, name: &str, size: i64) -> FsEntryRow {
    FsEntryRow {
        path_key: PathKey::canonicalize(path).unwrap(),
        name: name.to_string(),
        is_directory: false,
        size_bytes: size,
        modified_utc: FileTime::now(),
        hidden: false,
        attributes: 0,
        created_utc: FileTime::UNKNOWN,
    }
}

#[test]
fn largest_files_comes_back_ordered_with_real_sizes() {
    let f = Fixture::new();
    f.index
        .upsert_entries(
            &[
                row(r"C:\Data\a.bin", "a.bin", 10),
                row(r"C:\Data\b.bin", "b.bin", 9_000),
            ],
            1,
        )
        .unwrap();

    let outcome = f
        .service(Some(Arc::new(FakeIndexedMft)))
        .largest_files(Some(r"C:\Data"), 10, true, &CancelToken::none())
        .unwrap();

    assert_eq!(DiskUsageAvailability::Ready, outcome.availability);
    assert_eq!("b.bin", outcome.files[0].name);
}

/// The sizeless FSCTL_ENUM_USN_DATA shape, end to end: rows exist and every one is zero. The
/// service must hand back nothing at all rather than leaving each caller to re-decide whether a
/// screenful of "0 B" is worth drawing.
#[test]
fn a_sizeless_volume_yields_no_files_rather_than_a_list_of_zeros() {
    let f = Fixture::new();
    f.index
        .upsert_entries(
            &[
                row(r"C:\Data\a.bin", "a.bin", 0),
                row(r"C:\Data\b.bin", "b.bin", 0),
            ],
            1,
        )
        .unwrap();

    let outcome = f
        .service(Some(Arc::new(FakeIndexedMft)))
        .largest_files(Some(r"C:\Data"), 10, true, &CancelToken::none())
        .unwrap();

    assert_eq!(DiskUsageAvailability::NoSizeData, outcome.availability);
    assert!(outcome.files.is_empty());
}

#[test]
fn nothing_indexed_is_reported_as_such_so_the_view_can_offer_a_retry() {
    let f = Fixture::new();

    let outcome = f
        .service(None)
        .largest_files(Some(r"C:\Data"), 10, true, &CancelToken::none())
        .unwrap();

    assert_eq!(DiskUsageAvailability::NotIndexed, outcome.availability);
    assert!(outcome.files.is_empty());
}

/// Reports every volume complete — the "index is ready" half of the matrix that NullMftIndex
/// deliberately never covers.
struct FakeIndexedMft;

impl DiskUsageIndex for FakeIndexedMft {
    fn is_building(&self) -> bool {
        false
    }
    fn is_indexed(&self, _path_key: &str) -> bool {
        true
    }
    fn any_indexed(&self) -> bool {
        true
    }
}
