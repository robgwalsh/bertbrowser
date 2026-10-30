// Port of tests/BertBrowser.Core.Tests/IndexWatcherApplyTests.cs. Exercises the watcher's
// event-application logic directly: real directory-change tests are timing-flaky, and the
// interesting behaviour is how events mutate the index.
use std::path::PathBuf;
use std::sync::Arc;

use bb_core::FileTime;
use bb_core::data::{FsEntryRow, FsIndexRepository, SearchHit};
use bb_core::fs::{FsChange, FsChangeKind, IndexWatcherService};
use bb_core::search::SearchQuery;
use bb_paths::PathKey;
use bb_test_support::{TempDb, unique_temp_path};

struct Fixture {
    _db: TempDb,
    root: PathBuf,
    root_key: String,
    repo: Arc<FsIndexRepository>,
    service: IndexWatcherService,
}

impl Fixture {
    fn new() -> Fixture {
        let db = TempDb::new();
        let root = unique_temp_path("bertbrowser-tree", "");
        std::fs::create_dir_all(&root).unwrap();
        let root_key = PathKey::canonicalize(&root.to_string_lossy()).unwrap();
        let repo = Arc::new(FsIndexRepository::new(db.db()));
        let service = IndexWatcherService::new(repo.clone());
        repo.upsert_root(&root_key, &root.to_string_lossy(), FileTime::now(), true)
            .unwrap();
        Fixture {
            _db: db,
            root,
            root_key,
            repo,
            service,
        }
    }

    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn path(&self, relative: &str) -> String {
        self.root.join(relative).to_string_lossy().into_owned()
    }

    fn apply(&self, kind: FsChangeKind, full_path: &str, old_full_path: Option<&str>) {
        self.service.apply(&[FsChange {
            root_key: self.root_key.clone(),
            kind,
            full_path: full_path.to_string(),
            old_full_path: old_full_path.map(str::to_string),
        }]);
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
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn q(text: &str) -> SearchQuery {
    SearchQuery::parse(text).query.expect("query")
}

fn row(key: &str, name: &str, is_dir: bool, size: i64) -> FsEntryRow {
    FsEntryRow {
        path_key: key.to_string(),
        name: name.to_string(),
        is_directory: is_dir,
        size_bytes: size,
        modified_utc: FileTime::now(),
        hidden: false,
        attributes: 0,
        created_utc: FileTime::UNKNOWN,
    }
}

fn single(hits: Vec<SearchHit>) -> SearchHit {
    assert_eq!(hits.len(), 1, "expected one hit, got {hits:?}");
    hits.into_iter().next().unwrap()
}

#[test]
fn created_file_is_indexed() {
    let f = Fixture::new();
    let file = f.path("fresh.txt");
    std::fs::write(&file, [0u8; 7]).unwrap();

    f.apply(FsChangeKind::Created, &file, None);

    let hit = single(f.search("fresh"));
    assert_eq!(hit.size_bytes, 7);
    assert!(!hit.is_directory);
}

#[test]
fn created_directory_mini_crawls_its_contents() {
    // A folder moved into the tree raises one Created event for the top dir only.
    let f = Fixture::new();
    let moved = f.root.join("Moved");
    std::fs::create_dir_all(moved.join("Nested")).unwrap();
    std::fs::write(moved.join("Nested").join("payload.bin"), [0u8; 1]).unwrap();

    f.apply(FsChangeKind::Created, &moved.to_string_lossy(), None);

    assert_eq!(f.search("moved").len(), 1);
    let payload = single(f.search("payload"));
    assert_eq!(payload.relative_dir_display, r"Moved\Nested");
}

#[test]
fn created_file_hidden_state_is_relative_to_root() {
    // The test root lives under %TEMP% (inside the hidden %AppData% folder), so a naive
    // walk-to-drive-root would wrongly flag everything hidden. Effective-hidden must be measured
    // from the indexed root, matching a full crawl's seed.
    let f = Fixture::new();
    let visible = f.path("plain.txt");
    std::fs::write(&visible, [0u8; 1]).unwrap();
    let secret = f.path("secret.txt");
    std::fs::write(&secret, [0u8; 1]).unwrap();
    crate::index_crawler_tests::set_hidden(secret.as_ref());

    f.apply(FsChangeKind::Created, &visible, None);
    f.apply(FsChangeKind::Created, &secret, None);

    // Hidden excluded: the plain file survives, the hidden one does not.
    assert_eq!(f.search_visible("plain").len(), 1);
    assert!(f.search_visible("secret").is_empty());
    // The hidden file is still indexed (flagged) for when hidden is shown.
    assert!(single(f.search("secret")).hidden);
}

#[test]
fn deleted_entry_removes_subtree() {
    // Deletion applies to the index only: the entry is already gone from disk.
    let f = Fixture::new();
    let sub_key = PathKey::canonicalize(&f.path("Sub")).unwrap();
    f.repo
        .upsert_entries(
            &[
                row(&sub_key, "Sub", true, 0),
                row(&format!(r"{sub_key}\A.TXT"), "a.txt", false, 1),
            ],
            1,
        )
        .unwrap();

    f.apply(FsChangeKind::Deleted, &f.path("Sub"), None);

    assert!(f.search("a.txt").is_empty());
    assert!(f.search("sub").is_empty());
}

#[test]
fn renamed_directory_rewrites_descendants() {
    let f = Fixture::new();
    let old_key = PathKey::canonicalize(&f.path("Old")).unwrap();
    f.repo
        .upsert_entries(
            &[
                row(&old_key, "Old", true, 0),
                row(&format!(r"{old_key}\DOC.PDF"), "doc.pdf", false, 1),
            ],
            1,
        )
        .unwrap();

    f.apply(FsChangeKind::Renamed, &f.path("New"), Some(&f.path("Old")));

    let hit = single(f.search("doc.pdf"));
    assert_eq!(hit.relative_dir_display, "New");
    assert!(
        f.repo
            .search(&f.path("Old"), &q("doc"), 100, true)
            .unwrap()
            .0
            .is_empty()
    );
}

#[test]
fn changed_file_updates_size_and_timestamp() {
    let f = Fixture::new();
    let file = f.path("grow.txt");
    std::fs::write(&file, [0u8; 3]).unwrap();
    f.apply(FsChangeKind::Created, &file, None);

    std::fs::write(&file, [0u8; 9]).unwrap();
    f.apply(FsChangeKind::Changed, &file, None);

    assert_eq!(single(f.search("grow")).size_bytes, 9);
}

#[test]
fn created_then_vanished_treated_as_delete() {
    // Temp-file churn: the Created event arrives after the file is already gone.
    let f = Fixture::new();
    let file = f.path("ephemeral.tmp");
    let key = PathKey::canonicalize(&file).unwrap();
    f.repo
        .upsert_entries(&[row(&key, "ephemeral.tmp", false, 1)], 1)
        .unwrap();

    f.apply(FsChangeKind::Created, &file, None); // file does not exist on disk

    assert!(f.search("ephemeral").is_empty());
}
