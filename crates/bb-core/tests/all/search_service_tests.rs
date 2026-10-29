// Port of tests/BertBrowser.Core.Tests/SearchServiceTests.cs. No NTFS volumes in tests, so nothing
// is MFT-indexed and the crawl / live-scan fallbacks are exercised exactly as they are on a non-NTFS
// machine or a declined elevation prompt.
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bb_core::cancel::CancelToken;
use bb_core::data::{FsIndexRepository, SearchHit};
use bb_core::fs::{IndexCrawler, IndexWatcher, SearchProgress, SearchService};
use bb_core::mft::NullMftIndex;
use bb_core::models::{SearchOutcome, SearchResultSource};
use bb_paths::PathKey;
use bb_test_support::{TempDb, unique_temp_path};

pub struct FakeWatcherService {
    watching: Mutex<HashSet<String>>,
}

impl FakeWatcherService {
    pub fn new() -> FakeWatcherService {
        FakeWatcherService {
            watching: Mutex::new(HashSet::new()),
        }
    }
}

impl IndexWatcher for FakeWatcherService {
    fn is_watching(&self, root_key: &str) -> bool {
        self.watching.lock().unwrap().contains(root_key)
    }

    fn watch(&self, root_key: &str, _display_path: &str) {
        self.watching.lock().unwrap().insert(root_key.to_string());
    }
}

struct Fixture {
    _db: TempDb,
    root: PathBuf,
    repo: Arc<FsIndexRepository>,
    crawler: Arc<IndexCrawler>,
    watchers: Arc<FakeWatcherService>,
    service: SearchService,
}

impl Fixture {
    fn new() -> Fixture {
        let db = TempDb::new();
        let repo = Arc::new(FsIndexRepository::new(db.db()));
        let crawler = Arc::new(IndexCrawler::new(repo.clone()));
        let watchers = Arc::new(FakeWatcherService::new());
        let service = SearchService::new(
            repo.clone(),
            crawler.clone(),
            watchers.clone(),
            Arc::new(NullMftIndex),
        );
        Fixture {
            _db: db,
            root: unique_temp_path("bertbrowser-tree", ""),
            repo,
            crawler,
            watchers,
            service,
        }
    }

    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn create_file(&self, relative: &str) {
        let full = self.root.join(relative);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, [0u8; 1]).unwrap();
    }

    fn search(&self, root: &str, text: &str) -> Option<SearchOutcome> {
        self.service
            .search(
                root,
                text,
                true,
                SearchProgress::default(),
                &CancelToken::none(),
            )
            .unwrap()
    }

    /// A channel that receives the canonical key of the next completed (re)crawl.
    fn next_index_refreshed(&self) -> Receiver<String> {
        let (tx, rx) = channel();
        self.service.on_index_refreshed(Box::new(move |key| {
            let _ = tx.send(key.to_string());
        }));
        rx
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn with_timeout(rx: &Receiver<String>) -> String {
    rx.recv_timeout(Duration::from_secs(30))
        .expect("the background crawl did not report in time")
}

#[test]
fn too_short_query_returns_null() {
    let f = Fixture::new();
    std::fs::create_dir_all(&f.root).unwrap();
    assert!(f.search(&f.root(), "a").is_none());
    assert!(f.search(&f.root(), "").is_none());
}

#[test]
fn uncovered_streams_live_scan_and_indexes_in_background() {
    let f = Fixture::new();
    f.create_file("Report.docx");
    f.create_file(r"Sub\Report-2026.pdf");
    f.create_file(r"Sub\notes.txt");

    let refreshed = f.next_index_refreshed();
    let streamed: Mutex<Vec<SearchHit>> = Mutex::new(Vec::new());
    let collect = |batch: &[SearchHit]| streamed.lock().unwrap().extend_from_slice(batch);

    let outcome = f
        .service
        .search(
            &f.root(),
            "report",
            true,
            SearchProgress {
                live_batches: Some(&collect),
                content_progress: None,
            },
            &CancelToken::none(),
        )
        .unwrap()
        .expect("an outcome");

    assert_eq!(outcome.source, SearchResultSource::LiveScan);
    assert!(outcome.refresh_pending);
    assert_eq!(outcome.hits.len(), 2);
    assert_eq!(outcome.hits.len(), streamed.lock().unwrap().len()); // everything streamed

    // The lazy background crawl completes and registers the root.
    let refreshed_key = with_timeout(&refreshed);
    assert_eq!(refreshed_key, PathKey::canonicalize(&f.root()).unwrap());
    assert!(f.repo.find_covering_root(&f.root()).unwrap().is_some());

    // Second search: watcher attached during ensure_indexed, so a fresh index hit.
    let second = f.search(&f.root(), "report").expect("an outcome");
    assert_eq!(second.source, SearchResultSource::Index);
    assert!(!second.refresh_pending);
    assert_eq!(second.hits.len(), 2);
}

#[test]
fn covered_but_unwatched_serves_stale_index_and_revalidates() {
    let f = Fixture::new();
    f.create_file("cached.txt");
    f.crawler
        .crawl(&f.root(), &CancelToken::none(), None)
        .unwrap(); // indexed, but no watcher attached

    let refreshed = f.next_index_refreshed();
    let outcome = f.search(&f.root(), "cached").expect("an outcome");

    assert_eq!(outcome.source, SearchResultSource::StaleIndex); // instant results from cache
    assert!(outcome.refresh_pending);
    assert_eq!(outcome.hits.len(), 1);

    with_timeout(&refreshed); // background re-crawl attached the watcher

    let second = f.search(&f.root(), "cached").expect("an outcome");
    assert_eq!(second.source, SearchResultSource::Index);
}

#[test]
fn search_under_covered_root_uses_index_with_subtree_scope() {
    let f = Fixture::new();
    f.create_file(r"Sub\inner.txt");
    f.create_file("outer.txt");
    f.crawler
        .crawl(&f.root(), &CancelToken::none(), None)
        .unwrap();
    f.watchers
        .watch(&PathKey::canonicalize(&f.root()).unwrap(), &f.root());

    // Searching a subfolder of the indexed root: fresh index, scoped to the subtree.
    let sub = f.root.join("Sub").to_string_lossy().into_owned();
    let outcome = f.search(&sub, "txt").expect("an outcome");

    assert_eq!(outcome.source, SearchResultSource::Index);
    assert_eq!(outcome.hits.len(), 1);
    assert_eq!(outcome.hits[0].name, "inner.txt");
}
