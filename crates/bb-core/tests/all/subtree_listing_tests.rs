// Port of tests/BertBrowser.Core.Tests/SubtreeListingTests.cs: what the flat branch view lists.
// The fixture is SearchServiceTests' (a real temp database and a real directory tree) because the
// thing under test is a walk of an actual disk.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bb_core::cancel::CancelToken;
use bb_core::data::{FsIndexRepository, SearchHit};
use bb_core::fs::{IndexCrawler, IndexWatcher, SearchProgress, SearchService, is_held_path};
use bb_core::mft::NullMftIndex;
use bb_core::models::SearchOutcome;
use bb_core::search::LiveBatches;
use bb_paths::PathKey;
use bb_test_support::{TempDb, unique_temp_path};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW, SetFileAttributesW,
};

/// `DeleteExecutor.TrashFolderName`: the delete area's constant, parked in `is_held_path` until
/// that port lands. The guard below is what keeps this literal honest.
const TRASH_FOLDER_NAME: &str = ".bertbrowser-trash";

struct FakeWatchers;

impl IndexWatcher for FakeWatchers {
    fn is_watching(&self, _root_key: &str) -> bool {
        false
    }

    fn watch(&self, _root_key: &str, _display_path: &str) {}
}

struct Fixture {
    _db: TempDb,
    root: PathBuf,
    repo: Arc<FsIndexRepository>,
    service: SearchService,
}

impl Fixture {
    fn new() -> Fixture {
        let db = TempDb::new();
        let repo = Arc::new(FsIndexRepository::new(db.db()));
        let crawler = Arc::new(IndexCrawler::new(repo.clone()));
        let service = SearchService::new(
            repo.clone(),
            crawler,
            Arc::new(FakeWatchers),
            Arc::new(NullMftIndex),
        );
        let root = unique_temp_path("bertbrowser-flattree", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            _db: db,
            root,
            repo,
            service,
        }
    }

    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn create_file(&self, relative: &str) -> String {
        let full = self.root.join(relative);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, [0u8; 1]).unwrap();
        full.to_string_lossy().into_owned()
    }

    fn list(&self, include_hidden: bool, cap: usize, batches: LiveBatches<'_>) -> SearchOutcome {
        self.service
            .list_subtree(
                &self.root(),
                cap,
                include_hidden,
                SearchProgress {
                    live_batches: batches,
                    content_progress: None,
                },
                &CancelToken::none(),
            )
            .unwrap()
    }

    fn names(outcome: &SearchOutcome) -> Vec<String> {
        let mut names: Vec<String> = outcome.hits.iter().map(|h| h.name.clone()).collect();
        names.sort();
        names
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn set_hidden(path: &str) {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let attributes = GetFileAttributesW(wide.as_ptr());
        assert_ne!(u32::MAX, attributes, "{path}");
        assert_ne!(
            0,
            SetFileAttributesW(wide.as_ptr(), attributes | FILE_ATTRIBUTE_HIDDEN)
        );
    }
}

#[test]
fn lists_descendants_at_every_depth() {
    let f = Fixture::new();
    f.create_file("top.txt");
    f.create_file(r"Sub\middle.txt");
    f.create_file(r"Sub\Deep\bottom.txt");

    let outcome = f.list(false, 1000, None);

    // The whole point: a one-level listing can only ever show top.txt.
    assert_eq!(
        vec!["bottom.txt", "middle.txt", "top.txt"],
        Fixture::names(&outcome)
    );
    assert!(!outcome.truncated);
}

#[test]
fn relative_dir_is_empty_for_a_direct_child_and_the_subpath_below() {
    // The Folder column's contract. Everything a flat list is readable at all by is this string.
    let f = Fixture::new();
    f.create_file("top.txt");
    f.create_file(r"Sub\Deep\bottom.txt");

    let outcome = f.list(false, 1000, None);
    let hit = |name: &str| outcome.hits.iter().find(|h| h.name == name).unwrap();

    assert_eq!("", hit("top.txt").relative_dir_display);
    assert_eq!(r"Sub\Deep", hit("bottom.txt").relative_dir_display);
}

#[test]
fn descends_into_the_folders_it_does_not_show() {
    let f = Fixture::new();
    f.create_file(r"Sub\Deep\bottom.txt");

    let files = f.list(false, 1000, None);

    assert_eq!(vec!["bottom.txt"], Fixture::names(&files));
    assert!(!files.hits.iter().any(|h| h.is_directory));
}

#[test]
fn hidden_drops_the_entry_and_everything_under_a_hidden_folder() {
    let f = Fixture::new();
    f.create_file("visible.txt");
    let secret = f.create_file("secret.txt");
    let buried = f.create_file(r"Private\buried.txt");
    set_hidden(&secret);
    set_hidden(&f.root.join("Private").to_string_lossy());

    let without = f.list(false, 1000, None);
    let with = f.list(true, 1000, None);

    assert_eq!(vec!["visible.txt"], Fixture::names(&without));
    assert!(with.hits.iter().any(|h| h.name == "secret.txt"));
    let buried_name = bb_paths::file_name(&buried);
    assert!(with.hits.iter().any(|h| h.name == buried_name)); // the whole subtree, not just the folder
}

#[test]
fn stops_at_the_cap_and_says_so() {
    let f = Fixture::new();
    for i in 0..12 {
        f.create_file(&format!("file{i:02}.txt"));
    }

    let outcome = f.list(false, 5, None);

    assert!(outcome.truncated);
    assert_eq!(5, outcome.hits.len());
}

#[test]
fn everything_streamed_is_also_in_the_final_list() {
    // Nothing appears mid-listing that the finished listing then contradicts, and nothing the
    // finished listing holds was withheld from the stream.
    let f = Fixture::new();
    for i in 0..30 {
        f.create_file(&format!("Sub{}\\file{i:02}.txt", i % 3));
    }
    let streamed: Mutex<Vec<SearchHit>> = Mutex::new(Vec::new());
    let collect = |batch: &[SearchHit]| streamed.lock().unwrap().extend_from_slice(batch);

    let outcome = f.list(false, 1000, Some(&collect));

    let mut final_paths: Vec<String> = outcome
        .hits
        .iter()
        .map(|h| h.display_path.clone())
        .collect();
    let mut streamed_paths: Vec<String> = streamed
        .lock()
        .unwrap()
        .iter()
        .map(|h| h.display_path.clone())
        .collect();
    final_paths.sort();
    streamed_paths.sort();
    assert_eq!(final_paths, streamed_paths);
}

#[test]
fn a_file_held_by_an_undoable_delete_is_not_listed() {
    // Still on disk (that is what makes the delete undoable) but deleted as far as anyone looking
    // at the list is concerned.
    let f = Fixture::new();
    f.create_file("kept.txt");
    let gone = f.create_file(&format!("{TRASH_FOLDER_NAME}\\gone.txt"));
    assert!(
        is_held_path(&gone),
        "the parked rule must still know the holding folder"
    );

    let hits = f.list(true, 1000, None).hits;

    assert!(hits.iter().any(|h| h.name == "kept.txt"));
    assert!(!hits.iter().any(|h| h.name == "gone.txt"));
}

#[test]
fn an_archive_root_is_refused_rather_than_walked() {
    // The invariant, not the feature: a virtual path that reached a PathKey-keyed table would land
    // inside PrefixBounds of the container's own folder and poison every subtree scan over it. An
    // interior is listed from the loaded ArchiveIndex instead.
    let f = Fixture::new();
    let zip = f.create_file("bundle.zip");

    let outcome = f
        .service
        .list_subtree(
            &format!("{zip}\\inside"),
            1000,
            false,
            SearchProgress::default(),
            &CancelToken::none(),
        )
        .unwrap();

    assert!(outcome.problem.is_some());
    assert!(outcome.hits.is_empty());
}

#[test]
fn streams_a_small_first_batch_then_doubling_ones() {
    // Every batch is a post to the UI thread with a layout pass after it, so a fast walk must not
    // cut one every fifty rows, nor save everything for one batch at the end.
    let f = Fixture::new();
    for i in 0..1_000 {
        f.create_file(&format!("D{}\\f{i:04}.txt", i % 10));
    }
    let sizes: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    let record = |batch: &[SearchHit]| sizes.lock().unwrap().push(batch.len());

    let started = Instant::now();
    let outcome = f.list(false, 1000, Some(&record));
    let ticks = (started.elapsed().as_secs_f64() / SearchService::LIVE_BATCH_INTERVAL.as_secs_f64())
        as usize;

    let sizes = sizes.lock().unwrap().clone();
    assert_eq!(1_000, outcome.hits.len());
    assert_eq!(1_000, sizes.iter().sum::<usize>());
    assert_eq!(SearchService::FIRST_LIVE_BATCH_SIZE, sizes[0]);
    if ticks == 0 {
        // Nothing can have gone on the clock, so the sizes are the doubling alone.
        assert_eq!(vec![50, 100, 200, 400, 250], sizes);
    } else {
        // A slow machine: each tick of the clock may cut one more.
        assert!((1..=5 + ticks).contains(&sizes.len()), "{sizes:?}");
    }
}

#[test]
fn listing_never_enrols_the_folder_in_the_index() {
    // The other promise in list_subtree's remarks. This is the test that goes red if someone
    // helpfully adds an ensure-indexed call to make the second press faster.
    let f = Fixture::new();
    f.create_file(r"Sub\file.txt");

    f.list(false, 1000, None);
    std::thread::sleep(std::time::Duration::from_millis(250)); // a crawl, if one had been started, is single-flight and async

    assert!(
        f.repo
            .find_covering_root(&PathKey::canonicalize(&f.root()).unwrap())
            .unwrap()
            .is_none()
    );
}
