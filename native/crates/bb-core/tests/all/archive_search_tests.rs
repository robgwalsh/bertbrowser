// Port of tests/BertBrowser.Core.Tests/ArchiveSearchTests.cs.
use bb_core::archives::{ArchiveReader, ArchiveSearchScanner, LibraryArchiveReader};
use bb_core::cancel::CancelToken;
use bb_core::data::SearchHit;
use bb_core::search::{SearchQuery, SearchSyntax};
use bb_test_support::archive_support::{TempRoot, write_zip};

struct Fixture {
    root: TempRoot,
    reader: LibraryArchiveReader,
    zip: String,
}

impl Fixture {
    fn new() -> Fixture {
        let root = TempRoot::new("bertbrowser-archsearch");
        let zip = write_zip(
            &root.path("a.zip"),
            &[
                ("readme.txt", b"hello"),
                ("src/app.js", "x".repeat(500).as_bytes()),
                ("src/lib/util.js", b"util"),
                ("docs/guide.md", b"guide"),
                ("docs/notes.txt", b"notes"),
            ],
        );
        Fixture {
            root,
            reader: LibraryArchiveReader::new(),
            zip,
        }
    }

    fn search(&self, query: &SearchQuery, relative_to: &str, limit: usize) -> Vec<SearchHit> {
        ArchiveSearchScanner::search(
            &self.reader.read(&self.zip, None, &CancelToken::none()),
            &self.zip,
            relative_to,
            Some(query),
            limit,
            &CancelToken::none(),
            true,
        )
        .unwrap()
    }

    fn find(&self, query_text: &str, relative_to: &str) -> Vec<String> {
        let parse = SearchQuery::parse(query_text);
        let query = parse.query.as_ref().expect("query parses");
        let mut names: Vec<String> = self
            .search(query, relative_to, 1000)
            .into_iter()
            .map(|h| h.name)
            .collect();
        names.sort();
        names
    }
}

#[test]
fn a_plain_word_matches_entry_names() {
    let f = Fixture::new();
    assert_eq!(vec!["util.js"], f.find("util", ""));
}

/// The strongest evidence the shape is right: the scanner reuses `SearchQuery::matches` verbatim,
/// so every filter the box understands works inside a container without being reimplemented.
#[test]
fn every_filter_the_box_understands_works_in_here() {
    let f = Fixture::new();
    let rows: &[(&str, &[&str])] = &[
        ("ext:js", &["app.js", "util.js"]),
        ("ext:md;txt", &["guide.md", "notes.txt", "readme.txt"]),
        // A size bound excludes directories, here as everywhere else. Folders inside a container
        // do carry an exact recursive total, so this *could* have been special-cased, but the term
        // means the same thing wherever it is typed, and one filter behaving differently inside
        // an archive is a worse surprise than a folder it does not reach.
        ("size:>100", &["app.js"]),
        ("re:^app", &["app.js"]),
        ("notes OR guide", &["guide.md", "notes.txt"]),
        ("ext:txt !readme", &["notes.txt"]),
        // is:dir carries no filter of its own ("every folder" is not a search), so it is paired
        // with one that matches any name, exactly as it would have to be in the box.
        ("is:dir re:.", &["docs", "lib", "src"]),
    ];
    for (query, expected) in rows {
        assert_eq!(expected.to_vec(), f.find(query, ""), "{query}");
    }
}

#[test]
fn searching_from_inside_a_folder_only_looks_below_it() {
    let f = Fixture::new();
    assert_eq!(vec!["guide.md", "notes.txt"], f.find("ext:md;txt", "docs"));
    assert!(
        !f.find("ext:md;txt", "docs")
            .iter()
            .any(|n| n == "readme.txt")
    );
}

/// A container gives no timestamp for a folder it never listed explicitly, and the date terms
/// already apply a 1601 floor, so such an entry satisfies no date filter rather than every
/// open-ended one.
#[test]
fn an_entry_with_no_timestamp_satisfies_no_date_filter() {
    let f = Fixture::new();
    // "src" is synthesized from a path prefix and carries no date of its own.
    assert!(!f.find("dm:2000-01..2100-01", "").iter().any(|n| n == "src"));
}

#[test]
fn hits_carry_virtual_paths_and_their_folder_column() {
    let f = Fixture::new();
    let parse = SearchQuery::parse("util");
    let hits = f.search(parse.query.as_ref().unwrap(), "", 1000);

    assert_eq!(1, hits.len());
    assert_eq!(
        format!("{}\\src\\lib\\util.js", f.zip),
        hits[0].display_path
    );
    assert_eq!("src\\lib", hits[0].relative_dir_display);
}

#[test]
fn the_limit_stops_the_walk() {
    let f = Fixture::new();
    let parse = SearchQuery::parse("ext:js;md;txt");
    let hits = f.search(parse.query.as_ref().unwrap(), "", 2);

    assert_eq!(2, hits.len());
}

#[test]
fn a_damaged_archive_yields_nothing_rather_than_throwing() {
    let f = Fixture::new();
    let broken = f.root.make("broken.zip", b"not a zip");

    let parse = SearchQuery::parse("anything");
    let hits = ArchiveSearchScanner::search(
        &f.reader.read(&broken, None, &CancelToken::none()),
        &broken,
        "",
        parse.query.as_ref(),
        100,
        &CancelToken::none(),
        true,
    )
    .unwrap();

    assert!(hits.is_empty());
}

// --- the in:archives scope ---

// Paired with something, because a scope on its own is not a search; see the floor rule below.
#[test]
fn in_archives_asks_for_the_second_pass() {
    for query_text in [
        "report in:archives",
        "report in:archive",
        "ext:txt in:archives",
        "(a OR b) in:archives",
    ] {
        let parse = SearchQuery::parse(query_text);

        assert!(parse.query.is_some(), "{query_text}");
        assert!(
            parse.query.as_ref().unwrap().wants_archives(),
            "{query_text}"
        );
    }
}

/// An exclusion asks to leave containers out, which is already what happens; widening the scan
/// would be the opposite of what was typed. The same rule `!is:hidden` follows.
#[test]
fn not_in_archives_does_not_widen_the_scan() {
    let parse = SearchQuery::parse("report !in:archives");

    assert!(parse.query.is_some());
    assert!(!parse.query.as_ref().unwrap().wants_archives());
}

#[test]
fn an_ordinary_query_never_asks_for_it() {
    assert!(
        !SearchQuery::parse("report ext:txt")
            .query
            .unwrap()
            .wants_archives()
    );
}

#[test]
fn an_unknown_scope_is_a_problem_rather_than_a_name_term() {
    let parse = SearchQuery::parse("in:everything");

    assert!(parse.query.is_none());
    assert!(parse.problem.is_some());
    assert!(parse.problem.as_deref().unwrap().contains("in:archives"));
}

/// The marker matches everything and emits `1`, so on its own it is not a search: the floor rules
/// must still refuse it, or typing the scope alone would list the whole disk.
#[test]
fn the_scope_alone_is_not_a_search() {
    assert!(SearchQuery::parse("in:archives").query.is_none());
}

/// The card cannot advertise a filter the parser does not implement.
#[test]
fn the_syntax_card_advertises_it() {
    assert!(SearchSyntax::entries().any(|e| e.example.contains("in:archives")));
}

/// `content:` and `in:archives` together are refused by name rather than answered badly, because
/// there is no file on disk behind an archive entry for the reading pass to open.
#[test]
fn content_cannot_be_combined_with_looking_inside_archives() {
    for query_text in ["content:util in:archives", "in:archives content:util"] {
        let parse = SearchQuery::parse(query_text);

        assert!(parse.query.is_none(), "{query_text}");
        assert!(parse.problem.is_some(), "{query_text}");
        assert!(
            parse
                .problem
                .as_deref()
                .unwrap()
                .to_lowercase()
                .contains("archive"),
            "{query_text}"
        );
    }
}

/// And the scanner refuses one itself, rather than trusting its two callers to have checked. An
/// archive entry's candidate carries no content, and an unread candidate counts as a *possible*
/// match by design, so a scanner that ran the walk anyway would return *every entry in the
/// container*. Delete the guard and this goes red with five hits instead of none.
#[test]
fn the_scanner_refuses_a_content_query_rather_than_matching_every_entry() {
    let f = Fixture::new();
    // Built directly, since the grammar refuses this combination before a caller could form it.
    let query = SearchQuery::parse("content:util").query.unwrap();

    let hits = f.search(&query, "", 1000);

    assert!(hits.is_empty());
}
