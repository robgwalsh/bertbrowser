// Port of tests/BertBrowser.Core.Tests/SearchAgreementTests.cs: the test that makes the two faces of
// a query impossible to drift apart. Every query runs twice over one corpus, through SQLite and
// directly through the matcher, and the names must be identical.
use bb_core::FileTime;
use bb_core::attributes::{
    ARCHIVE, COMPRESSED, DIRECTORY, HIDDEN, READONLY, REPARSE_POINT, SYSTEM,
};
use bb_core::data::{FsEntryRow, FsIndexRepository};
use bb_core::search::{SearchCandidate, SearchQuery};
use bb_paths::{PathKey, fold_upper};
use bb_test_support::TempDb;

const ROOT: &str = r"C:\Corpus";

fn base() -> FileTime {
    FileTime::from_ymd_hms(2026, 6, 15, 12, 0, 0)
}

struct Entry {
    path: &'static str,
    is_dir: bool,
    size: i64,
    modified: FileTime,
    hidden: bool,
    attributes: u32,
    created: FileTime,
}

fn e(
    path: &'static str,
    is_dir: bool,
    size: i64,
    modified: FileTime,
    hidden: bool,
    attributes: u32,
    created: FileTime,
) -> Entry {
    Entry {
        path,
        is_dir,
        size,
        modified,
        hidden,
        attributes,
        created,
    }
}

/// Attributes and creation dates are picked to exercise the shapes that break things: a row with
/// neither, a file created long after it was modified (a copy), and every attribute bit at least once.
fn corpus() -> Vec<Entry> {
    let b = base();
    vec![
        e(r"C:\Corpus\report.txt", false, 1024, b, false, ARCHIVE, b),
        e(
            r"C:\Corpus\report.docx",
            false,
            2_000_000,
            b.add_days(-1),
            false,
            ARCHIVE | READONLY,
            b.add_days(-1),
        ),
        e(
            r"C:\Corpus\Report-DRAFT.txt",
            false,
            10,
            b.add_days(-40),
            false,
            ARCHIVE,
            b.add_days(-40),
        ),
        e(
            r"C:\Corpus\notes.txt",
            false,
            0,
            b.add_days(-731),
            false,
            ARCHIVE,
            b.add_days(-731),
        ),
        e(
            r"C:\Corpus\photo.jpg",
            false,
            5_000_000,
            b,
            false,
            ARCHIVE | COMPRESSED,
            b,
        ),
        e(
            r"C:\Corpus\photo.jpeg",
            false,
            4_000_000,
            b,
            false,
            ARCHIVE,
            b,
        ),
        e(
            r"C:\Corpus\IMG_0042.jpg",
            false,
            3_000_000,
            b.add_days(-3),
            false,
            ARCHIVE,
            b.add_days(-3),
        ),
        e(r"C:\Corpus\IMG_7.png", false, 900, b, false, ARCHIVE, b),
        e(
            r"C:\Corpus\.gitignore",
            false,
            40,
            b,
            true,
            ARCHIVE | HIDDEN,
            b,
        ),
        e(r"C:\Corpus\a[1].txt", false, 12, b, false, ARCHIVE, b),
        e(r"C:\Corpus\a*b.txt", false, 13, b, false, ARCHIVE, b),
        e(
            r"C:\Corpus\Übung-01.pdf",
            false,
            700_000,
            b,
            false,
            ARCHIVE,
            b,
        ),
        e(r"C:\Corpus\Projects", true, 0, b, false, DIRECTORY, b),
        // Copied in: modified two years ago, created today.
        e(
            r"C:\Corpus\Projects\alpha.txt",
            false,
            1_048_576,
            b.add_days(-731),
            false,
            ARCHIVE,
            b,
        ),
        e(
            r"C:\Corpus\Projects\beta.log",
            false,
            20_000_000,
            b.add_days(-200),
            false,
            ARCHIVE | SYSTEM,
            b.add_days(-200),
        ),
        e(
            r"C:\Corpus\Projects\gamma.log",
            false,
            100,
            b,
            true,
            ARCHIVE | HIDDEN,
            b,
        ),
        e(
            r"C:\Corpus\Projects\report final.txt",
            false,
            4096,
            b,
            false,
            ARCHIVE,
            b,
        ),
        e(
            r"C:\Corpus\Projects\Linked",
            true,
            0,
            b,
            false,
            DIRECTORY | REPARSE_POINT,
            b,
        ),
        e(r"C:\Corpus\Archive", true, 0, b, false, DIRECTORY, b),
        e(
            r"C:\Corpus\Archive\report.txt",
            false,
            512,
            b.add_days(-1096),
            false,
            ARCHIVE | READONLY,
            b.add_days(-1096),
        ),
        // The fallback build's shape: no size, no timestamps, no attributes.
        e(
            r"C:\Corpus\Archive\ghost.bin",
            false,
            0,
            FileTime::UNKNOWN,
            false,
            0,
            FileTime::UNKNOWN,
        ),
    ]
}

fn setup() -> (TempDb, FsIndexRepository) {
    let t = TempDb::new();
    let repo = FsIndexRepository::new(t.db());
    let rows: Vec<FsEntryRow> = corpus()
        .iter()
        .map(|c| FsEntryRow {
            path_key: PathKey::canonicalize(c.path).unwrap(),
            name: bb_paths::file_name(c.path).to_string(),
            is_directory: c.is_dir,
            size_bytes: c.size,
            modified_utc: c.modified,
            hidden: c.hidden,
            attributes: c.attributes,
            created_utc: c.created,
        })
        .collect();
    repo.upsert_entries(&rows, 1).unwrap();
    (t, repo)
}

fn from_sql(repo: &FsIndexRepository, text: &str) -> Vec<String> {
    let query = SearchQuery::parse(text)
        .query
        .unwrap_or_else(|| panic!("{text}"));
    let mut names: Vec<String> = repo
        .search(ROOT, &query, 1000, true)
        .unwrap()
        .0
        .into_iter()
        .map(|h| fold_upper(&h.display_path))
        .collect();
    names.sort();
    names
}

fn from_matcher(text: &str) -> Vec<String> {
    let query = SearchQuery::parse(text)
        .query
        .unwrap_or_else(|| panic!("{text}"));
    let mut names: Vec<String> = corpus()
        .iter()
        .filter(|c| {
            let name_key = fold_upper(bb_paths::file_name(c.path));
            let path_key = PathKey::canonicalize(c.path).unwrap();
            query.matches(&SearchCandidate {
                name_key: &name_key,
                path_key: &path_key,
                is_directory: c.is_dir,
                size_bytes: c.size,
                modified_utc: c.modified,
                hidden: c.hidden,
                attributes: c.attributes,
                created_utc: c.created,
                content: None,
            })
        })
        .map(|c| PathKey::canonicalize(c.path).unwrap())
        .collect();
    names.sort();
    names
}

/// A content query's first pass must return everything the name and metadata allow. The agreement
/// theory cannot catch this: both sides run with no file read and agree by construction.
#[test]
fn a_content_query_shortlists_every_candidate_the_name_allows() {
    let (_t, repo) = setup();
    let query = SearchQuery::parse("content:anything ext:txt")
        .query
        .unwrap();
    let hits = repo.search(ROOT, &query, 1000, true).unwrap().0;
    let expected = corpus()
        .iter()
        .filter(|c| !c.is_dir && c.path.to_ascii_lowercase().ends_with(".txt"))
        .count();
    assert_eq!(expected, hits.len());
}

/// Queries chosen to exercise every term type, and every way of combining them.
const QUERIES: &[&str] = &[
    "report",
    "rep ort",
    "REPORT",
    "übung",
    "*.txt",
    "?eport",
    "a[1]",
    "\"report final\"",
    "\"a*b\"",
    "ext:txt",
    "ext:jpg",
    "ext:jpg;png",
    "ext:log ext:log",
    "size:>1mb",
    "size:<1kb",
    "size:>=1024",
    "size:<=1024",
    "size:1kb..2mb",
    "size:empty",
    "size:=100",
    "dm:2026-06",
    "dm:>2026-06-01",
    "dm:<2025-01-01",
    "dm:2024..2026",
    "dc:2026-06",
    "dc:>2026-06-01",
    "dc:<2025-01-01",
    "dc:2024..2026",
    "dc:2026-06 dm:<2025",
    "dc:2026 OR dm:2026",
    "is:dir report",
    "is:file report",
    "report is:hidden",
    "report is:readonly",
    "log is:system",
    "report is:archived",
    "is:link Linked",
    "photo is:compressed",
    "report !is:readonly",
    "!is:archived ext:txt",
    "report is:readonly OR log is:system",
    "report is:archived is:readonly",
    "report is:readonly dc:<2025",
    "ext:txt is:archived dm:2026-06",
    "path:projects",
    "path:archive report",
    "re:^report",
    r"re:^img_\d+",
    "re:log$",
    "report !draft",
    "!draft ext:txt",
    "report !ext:docx",
    "report OR photo",
    "ext:jpg OR ext:png",
    "(report OR photo) ext:txt",
    "report !re:draft",
    "!re:^img ext:jpg",
    "report ext:txt size:>100 dm:>2020",
    "path:corpus !path:archive ext:txt",
    // A content term reads the disk, so here, where no file is ever opened, it is a pure superset on
    // both sides. The first pass must not narrow.
    "content:report",
    "content:report ext:txt",
    "report !content:draft",
    "content:alpha OR ext:jpg",
    "is:dir content:alpha",
];

#[test]
fn sql_and_the_matcher_return_the_same_entries() {
    let (_t, repo) = setup();
    for text in QUERIES {
        assert_eq!(from_matcher(text), from_sql(&repo, text), "query {text:?}");
    }
}

/// Meta-test: the comparison above can actually fail.
#[test]
fn the_comparison_notices_a_disagreement() {
    let (_t, repo) = setup();
    let from_sql = from_sql(&repo, "report");
    assert!(!from_sql.is_empty());
    assert_ne!(from_sql, from_sql[1..].to_vec());
}

/// A regex compiles to a superset, so the row set SQLite returns is filtered again. If that re-check
/// were dropped, this query would return every entry in the subtree.
#[test]
fn an_incomplete_predicate_still_returns_only_real_matches() {
    let (_t, repo) = setup();
    let query = SearchQuery::parse(r"re:^img_\d+").query.unwrap();
    let mut names: Vec<String> = repo
        .search(ROOT, &query, 1000, true)
        .unwrap()
        .0
        .into_iter()
        .map(|h| h.name)
        .collect();
    names.sort();
    assert_eq!(vec!["IMG_0042.jpg", "IMG_7.png"], names);
}

/// Rows with a length are what tells a measured volume from one built by the names-only fallback.
#[test]
fn size_data_is_detected_per_scope() {
    let (_t, repo) = setup();
    assert!(repo.has_size_data(Some(ROOT)).unwrap());
    assert!(repo.has_size_data(None).unwrap());
    // Archive holds a 512-byte file and a zero-byte one, so it is measured...
    assert!(repo.has_size_data(Some(r"C:\Corpus\Archive")).unwrap());
    // ...whereas a subtree of nothing but sizeless rows looks exactly like the fallback build.
    assert!(!repo.has_size_data(Some(r"C:\Corpus\Nowhere")).unwrap());
}

/// The cap counts rows that matched, not rows the scan happened to read.
#[test]
fn the_cap_counts_matches_even_when_the_predicate_is_a_superset() {
    let (_t, repo) = setup();
    let query = SearchQuery::parse("re:txt$").query.unwrap();
    let (hits, truncated) = repo.search(ROOT, &query, 2, true).unwrap();
    assert_eq!(2, hits.len());
    assert!(truncated);
    assert!(
        hits.iter()
            .all(|h| h.name.to_ascii_lowercase().ends_with(".txt"))
    );
}
