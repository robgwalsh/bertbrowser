// Port of tests/BertBrowser.Core.Tests/ContentSearchAgreementTests.cs. The whole content pipeline
// against an independently computed answer, over a real tree. `search_agreement_tests` structurally
// cannot cover a content term: both of its sides run with no content read and agree by
// construction. Here the query goes through the real SearchService, and the expected answer is
// computed by reading every file and applying the same predicate by hand. Both branches of
// `search` are covered: unindexed (the walker, as an unelevated app really runs) and then again
// once the tree has been crawled into fs_entry.
use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::Arc;

use bb_core::FileTime;
use bb_core::cancel::CancelToken;
use bb_core::data::FsIndexRepository;
use bb_core::fs::{IndexCrawler, IndexWatcher, SearchProgress, SearchService};
use bb_core::mft::NullMftIndex;
use bb_core::models::SearchOutcome;
use bb_core::search::{ContentText, SearchCandidate, SearchMatch, SearchQuery};
use bb_paths::{PathKey, fold_upper};
use bb_test_support::{TempDb, unique_temp_path};

/// Name to contents. Deliberately adversarial about names versus contents.
const CORPUS: &[(&str, &str)] = &[
    (
        "notes.txt",
        "the annual report is late\nsecond line\nTODO tidy this",
    ),
    ("decoy.txt", "annual sales figures\nand a separate report"),
    ("code.cs", "// TODO: fix\npublic class Thing { }"),
    ("clean.cs", "public class Other { }"),
    ("sub/deep.md", "nothing interesting\nTODO nested"),
    ("sub/plain.md", "just prose about an annual report"),
    // The needle is in the *name* only: a content search must never match on this, which is what
    // proves the reader ran rather than the name term quietly answering.
    (
        "TODO.txt",
        "this file is named for it but does not say the word",
    ),
    ("MixedCase.txt", "Annual Report in title case"),
];

const QUERIES: &[&str] = &[
    "content:TODO",
    "content:todo",
    "content:\"annual report\"",
    "content:annual",
    "content:TODO ext:cs",
    "content:TODO ext:md",
    "content:annual !content:sales",
    "content:TODO OR content:annual",
    "ext:cs !content:TODO",
    "notes content:annual",
    "content:TODO OR ext:md",
    "content:nothingatallmatchesthis",
    "is:file content:TODO",
];

struct NoWatchers;

impl IndexWatcher for NoWatchers {
    fn is_watching(&self, _root_key: &str) -> bool {
        false
    }
    fn watch(&self, _root_key: &str, _display_path: &str) {}
}

struct Fixture {
    _db: TempDb,
    root: PathBuf,
    crawler: Arc<IndexCrawler>,
    service: SearchService,
}

impl Fixture {
    fn new() -> Fixture {
        let db = TempDb::new();
        let root = unique_temp_path("bb-cagree-tree", "");
        for (relative, body) in CORPUS {
            let full = root.join(relative.replace('/', "\\"));
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, body).unwrap();
        }
        let repo = Arc::new(FsIndexRepository::new(db.db()));
        let crawler = Arc::new(IndexCrawler::new(repo.clone()));
        let service = SearchService::new(
            repo,
            crawler.clone(),
            Arc::new(NoWatchers),
            Arc::new(NullMftIndex),
        );
        Fixture {
            _db: db,
            root,
            crawler,
            service,
        }
    }

    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    /// The answer, computed without going anywhere near the search stack.
    fn brute_force(&self, query_text: &str) -> Vec<String> {
        let query = SearchQuery::parse(query_text).query.expect("query");
        let mut names = Vec::new();
        for (relative, _) in CORPUS {
            let full = self.root.join(relative.replace('/', "\\"));
            let full_text = full.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&full).unwrap();
            let meta = std::fs::metadata(&full).unwrap();
            let name = bb_paths::file_name(&full_text).to_string();
            let content = ContentText::new(text, false);
            let candidate = SearchCandidate {
                name_key: &fold_upper(&name),
                path_key: &PathKey::canonicalize(&full_text).unwrap(),
                is_directory: false,
                size_bytes: meta.len() as i64,
                modified_utc: FileTime(meta.last_write_time() as i64),
                hidden: false,
                attributes: meta.file_attributes(),
                created_utc: FileTime(meta.creation_time() as i64),
                content: Some(&content),
            };
            if query.evaluate(&candidate) == SearchMatch::Yes {
                names.push(name);
            }
        }
        names.sort();
        names
    }

    fn search(&self, query_text: &str) -> SearchOutcome {
        self.service
            .search(
                &self.root(),
                query_text,
                true,
                SearchProgress::default(),
                &CancelToken::none(),
            )
            .unwrap()
            .expect("an outcome")
    }

    fn through_the_service(&self, query_text: &str) -> Vec<String> {
        let outcome = self.search(query_text);
        assert_eq!(outcome.problem, None);
        let mut names: Vec<String> = outcome
            .hits
            .iter()
            .filter(|h| !h.is_directory)
            .map(|h| h.name.clone())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Best effort: a live-scan query starts a background crawl, and the service's drop stops it.
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn the_live_scan_branch_agrees_with_reading_every_file() {
    for query in QUERIES {
        // Unindexed, so this takes the walker: the path an unelevated app really uses.
        let f = Fixture::new();
        assert_eq!(
            f.brute_force(query),
            f.through_the_service(query),
            "{query}"
        );
    }
}

#[test]
fn the_index_branch_agrees_too_once_the_tree_is_crawled() {
    for query in QUERIES {
        let f = Fixture::new();
        f.crawler
            .crawl(&f.root(), &CancelToken::none(), None)
            .unwrap();

        assert_eq!(
            f.brute_force(query),
            f.through_the_service(query),
            "{query}"
        );
    }
}

/// A meta-test: the comparison above has to be able to fail, or a green run means nothing.
#[test]
fn the_comparison_notices_a_missing_file() {
    let f = Fixture::new();
    let expected = f.brute_force("content:TODO");
    assert!(!expected.is_empty());

    let short = &expected[1..];
    assert_ne!(expected.as_slice(), short);
}

/// The assertion that proves the reader ran at all: nothing in these results is explicable by the
/// name. `TODO.txt` is named for the needle and does not contain it; the files that do contain it
/// are not named for it.
#[test]
fn a_content_search_matches_on_contents_and_not_on_names() {
    let f = Fixture::new();
    let names = f.through_the_service("content:TODO");

    assert!(!names.contains(&"TODO.txt".to_string()));
    assert!(names.contains(&"notes.txt".to_string()));
    assert!(names.contains(&"code.cs".to_string()));
    assert!(names.contains(&"deep.md".to_string()));
}

#[test]
fn every_content_hit_carries_the_line_it_matched_on() {
    let f = Fixture::new();
    let outcome = f.search("content:TODO");

    let hits: Vec<_> = outcome
        .hits
        .iter()
        .filter(|h| h.name == "code.cs")
        .collect();
    assert_eq!(hits.len(), 1);
    let m = hits[0].content_match.as_ref().expect("a match");
    assert_eq!(m.line_number, 1);
    assert_eq!(m.line, "// TODO: fix");
}

#[test]
fn an_ordinary_search_reports_no_content_scan_at_all() {
    // None is how the caller knows not to show the Match column, so it must stay None for every
    // query that never read a file.
    let f = Fixture::new();
    let outcome = f.search("notes");
    assert!(outcome.content_scan.is_none());

    let content = f.search("content:TODO");
    assert!(content.content_scan.is_some());
}
