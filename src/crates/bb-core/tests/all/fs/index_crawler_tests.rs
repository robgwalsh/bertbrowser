// Port of tests/BertBrowser.Core.Tests/IndexCrawlerTests.cs. Real trees under %TEMP%, crawled into
// a temp database.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use bb_core::cancel::CancelToken;
use bb_core::data::{FsIndexRepository, SearchHit};
use bb_core::fs::{DirectoryRemoval, IndexCrawler};
use bb_core::search::SearchQuery;
use bb_test_support::{TempDb, unique_temp_path};

struct Fixture {
    _db: TempDb,
    root: PathBuf,
    repo: Arc<FsIndexRepository>,
    crawler: IndexCrawler,
}

impl Fixture {
    fn new() -> Fixture {
        let db = TempDb::new();
        let repo = Arc::new(FsIndexRepository::new(db.db()));
        let crawler = IndexCrawler::new(repo.clone());
        Fixture {
            _db: db,
            root: unique_temp_path("bertbrowser-tree", ""),
            repo,
            crawler,
        }
    }

    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn create_file(&self, relative: &str, bytes: usize) {
        let full = self.root.join(relative);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, vec![0u8; bytes]).unwrap();
    }

    fn crawl(&self, cancel: &CancelToken) -> bool {
        self.crawler.crawl(&self.root(), cancel, None).unwrap()
    }

    fn search(&self, text: &str) -> Vec<SearchHit> {
        self.repo
            .search(&self.root(), &q(text), 100, true)
            .unwrap()
            .0
    }

    fn search_visible(&self, text: &str) -> Vec<SearchHit> {
        self.repo
            .search(&self.root(), &q(text), 100, false)
            .unwrap()
            .0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Junctions are removed as entries, never followed: the tree walk in DirectoryRemoval.
        let _ = DirectoryRemoval::remove_tree(&self.root());
    }
}

fn q(text: &str) -> SearchQuery {
    SearchQuery::parse(text).query.expect("query")
}

pub fn set_hidden(path: &Path) {
    let out = Command::new("attrib").arg("+h").arg(path).output().unwrap();
    assert!(out.status.success(), "attrib +h failed");
}

fn single(hits: Vec<SearchHit>) -> SearchHit {
    assert_eq!(hits.len(), 1, "expected one hit, got {hits:?}");
    hits.into_iter().next().unwrap()
}

#[test]
fn crawl_indexes_files_and_directories_recursively() {
    let f = Fixture::new();
    f.create_file("Alpha.txt", 100);
    f.create_file(r"Sub\Beta.txt", 1);
    f.create_file(r"Sub\Deep\Gamma.bin", 1);

    assert!(f.crawl(&CancelToken::none()));

    let root = f.repo.find_covering_root(&f.root()).unwrap().expect("root");
    assert!(root.complete);
    assert!(!root.stale);

    assert_eq!(single(f.search("alpha")).size_bytes, 100);
    assert_eq!(single(f.search("gamma")).relative_dir_display, r"Sub\Deep");

    // Directories are indexed too.
    assert!(single(f.search("deep")).is_directory);
}

#[test]
fn crawl_propagates_hidden_into_subtree() {
    let f = Fixture::new();
    f.create_file(r"Visible\keep.txt", 1);
    f.create_file(r"Secret\inside.txt", 1); // "inside" is not itself hidden...
    // ...but its parent folder is, so it must count as hidden.
    set_hidden(&f.root.join("Secret"));

    assert!(f.crawl(&CancelToken::none()));

    // With hidden included, the file inside the hidden folder is found and flagged.
    assert!(single(f.search("inside")).hidden);

    // With hidden excluded, nothing under the hidden folder surfaces...
    assert!(f.search_visible("inside").is_empty());
    // ...while a sibling under a visible folder still does.
    assert_eq!(f.search_visible("keep").len(), 1);
}

#[test]
fn crawl_emits_junction_but_does_not_descend() {
    let f = Fixture::new();
    f.create_file(r"real\inner.bin", 1);
    let junction = f.root.join("junction");
    assert!(
        crate::fs::directory_removal_tests::try_create_junction(&junction, &f.root.join("real")),
        "junction was not created"
    );

    f.crawl(&CancelToken::none());

    // The junction itself is searchable...
    assert!(single(f.search("junction")).is_directory);

    // ...but its contents were not double-indexed through it.
    assert_eq!(single(f.search("inner")).relative_dir_display, "real");
}

#[test]
fn crawl_cancelled_does_not_register_root() {
    let f = Fixture::new();
    f.create_file("a.txt", 1);
    let cancel = CancelToken::new();
    cancel.cancel();

    let completed = f.crawl(&cancel);

    assert!(!completed);
    assert!(f.repo.find_covering_root(&f.root()).unwrap().is_none());
}

#[test]
fn recrawl_sweeps_vanished_entries() {
    let f = Fixture::new();
    f.create_file("keep.txt", 1);
    f.create_file("gone.txt", 1);
    f.crawl(&CancelToken::none());
    assert_eq!(f.search("gone").len(), 1);

    std::fs::remove_file(f.root.join("gone.txt")).unwrap();
    f.crawl(&CancelToken::none());

    assert!(f.search("gone").is_empty());
    assert_eq!(f.search("keep").len(), 1);
}
