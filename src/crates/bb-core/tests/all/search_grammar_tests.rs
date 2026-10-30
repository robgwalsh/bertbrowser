// Port of tests/BertBrowser.Core.Tests/SearchGrammarTests.cs.
use bb_core::FileTime;
use bb_core::attributes::{ARCHIVE, COMPRESSED, HIDDEN, READONLY, REPARSE_POINT, SYSTEM};
use bb_core::search::SearchQuery;

use crate::support::Entry;

fn q(text: &str) -> SearchQuery {
    let parse = SearchQuery::parse(text);
    assert!(parse.problem.is_none(), "{text}: {:?}", parse.problem);
    parse.query.unwrap_or_else(|| panic!("{text}: no query"))
}

fn utc(y: i64, m: u32, d: u32) -> FileTime {
    FileTime::from_ymd_hms(y, m, d, 12, 0, 0)
}

// --- ext: ---

#[test]
fn extension() {
    for (query, name, expected) in [
        ("ext:jpg", "photo.jpg", true),
        ("ext:jpg", "photo.jpeg", false),
        ("ext:jpg", "photo.JPG", true),
        ("ext:jpg;png", "logo.png", true),
        ("ext:jpg,png", "logo.png", true),
        ("ext:.jpg", "photo.jpg", true), // a typed dot is tolerated
        ("ext:jpg", ".jpg", false),      // a dotted name is not an extension
    ] {
        assert_eq!(
            expected,
            q(query).matches(&Entry::file(name).candidate()),
            "{query} {name}"
        );
    }
}

/// A filter is specific enough to run with no text at all.
#[test]
fn a_filter_alone_clears_the_floor() {
    assert!(SearchQuery::parse("ext:jpg").query.is_some());
}

/// Half the disk is not a search result.
#[test]
fn a_kind_alone_does_not() {
    for text in [
        "is:dir",
        "is:file",
        "is:hidden",
        "is:readonly",
        "is:system",
        "is:archived",
        "is:link",
        "is:compressed",
    ] {
        let parse = SearchQuery::parse(text);
        assert!(parse.query.is_none(), "{text}");
        assert!(parse.problem.is_none(), "{text}"); // not an error, just not a search
    }
}

// --- size: ---

#[test]
fn size() {
    for (query, bytes, expected) in [
        ("size:>1kb", 2048, true),
        ("size:>1kb", 1024, false), // strictly greater
        ("size:>=1kb", 1024, true),
        ("size:<1kb", 1023, true),
        ("size:<1kb", 1024, false),
        ("size:<=1kb", 1024, true),
        ("size:=1kb", 1024, true),
        ("size:1kb", 1024, true),
        ("size:1kb", 1025, false),
        ("size:1kb..2kb", 1500, true),
        ("size:1kb..2kb", 2048, true), // inclusive at both ends
        ("size:1kb..2kb", 2049, false),
        ("size:empty", 0, true),
        ("size:empty", 1, false),
    ] {
        assert_eq!(
            expected,
            q(query).matches(&Entry::file("f.bin").size(bytes).candidate()),
            "{query} {bytes}"
        );
    }
}

/// A folder's indexed length is 0, so a size filter must not sweep every folder into a small-size query.
#[test]
fn size_never_matches_a_directory() {
    assert!(!q("size:<1kb").matches(&Entry::dir("Docs").candidate()));
}

// --- dm: ---

/// Mid-month bounds, because a calendar span means the user's local month and its edges sit at the
/// UTC offset.
#[test]
fn modified_range() {
    let query = q("dm:2026-06");
    assert!(query.matches(&Entry::file("a.txt").modified(utc(2026, 6, 15)).candidate()));
    assert!(!query.matches(&Entry::file("b.txt").modified(utc(2026, 7, 15)).candidate()));
    assert!(!query.matches(&Entry::file("c.txt").modified(utc(2026, 5, 15)).candidate()));
}

/// The sizeless index build writes no timestamp. Such a row must not satisfy a date filter, least of
/// all an open-ended one.
#[test]
fn a_row_with_no_timestamp_matches_no_date_filter() {
    let unmeasured = Entry::file("ghost.txt")
        .modified(FileTime::UNKNOWN)
        .created(FileTime::UNKNOWN);
    for text in [
        "dm:<2020",
        "dm:2026-06",
        "dm:>2000",
        "dc:<2020",
        "dc:2026-06",
        "dc:>2000",
    ] {
        assert!(!q(text).matches(&unmeasured.candidate()), "{text}");
    }
}

// --- dc: ---

#[test]
fn created_range() {
    let query = q("dc:2026-06");
    assert!(query.matches(&Entry::file("a.txt").created(utc(2026, 6, 15)).candidate()));
    assert!(!query.matches(&Entry::file("b.txt").created(utc(2026, 7, 15)).candidate()));
    assert!(!query.matches(&Entry::file("c.txt").created(utc(2026, 5, 15)).candidate()));
}

#[test]
fn created_operators_and_aliases() {
    for (query, expected) in [
        ("dc:>2026-06-01", true),
        ("dc:<2026-06-01", false),
        ("dc:2026-01-01..2026-12-31", true),
        ("dc:2025", false),
        ("datecreated:2026-06", true),
        ("created:2026-06", true),
    ] {
        assert_eq!(
            expected,
            q(query).matches(&Entry::file("a.txt").created(utc(2026, 6, 15)).candidate()),
            "{query}"
        );
    }
}

/// The two dates are separate columns. A file copied in is the everyday case: created now, modified
/// years ago.
#[test]
fn created_and_modified_are_not_the_same_column() {
    let copied = Entry::file("old.txt")
        .modified(utc(2020, 6, 15))
        .created(utc(2026, 6, 15));
    assert!(q("dc:2026-06").matches(&copied.candidate()));
    assert!(!q("dm:2026-06").matches(&copied.candidate()));
    assert!(q("dm:2020-06").matches(&copied.candidate()));
    assert!(!q("dc:2020-06").matches(&copied.candidate()));
}

/// A bad value names the key the user actually typed, not the one it shares code with.
#[test]
fn a_created_problem_says_created() {
    let problem = SearchQuery::parse("dc:banana").problem.expect("problem");
    assert!(problem.starts_with("dc:"));
    assert!(!problem.contains("dm:"));
}

// --- is: attributes ---

#[test]
fn attributes() {
    for (query, attributes, expected) in [
        ("is:readonly", READONLY, true),
        ("is:readonly", ARCHIVE, false),
        ("is:read-only", READONLY, true),
        ("is:ro", READONLY, true),
        ("is:system", SYSTEM, true),
        ("is:system", ARCHIVE, false),
        ("is:archived", ARCHIVE, true),
        ("is:archive", ARCHIVE, true), // the Opus/XYplorer spelling
        ("is:archived", READONLY, false),
        ("is:link", REPARSE_POINT, true),
        ("is:junction", REPARSE_POINT, true),
        ("is:symlink", REPARSE_POINT, true),
        ("is:reparse", REPARSE_POINT, true),
        ("is:link", ARCHIVE, false),
        ("is:compressed", COMPRESSED, true),
        ("is:compressed", ARCHIVE, false),
        ("IS:READONLY", READONLY, true), // the value folds, like every other
    ] {
        let text = format!("{query} report");
        assert_eq!(
            expected,
            q(&text).matches(&Entry::file("report.txt").attributes(attributes).candidate()),
            "{text}"
        );
    }
}

/// One bit of a mask, not the whole mask: a file is normally several things at once.
#[test]
fn an_attribute_term_tests_one_bit_of_many() {
    let entry = Entry::file("app.dll").attributes(READONLY | SYSTEM | ARCHIVE);
    assert!(q("app is:readonly").matches(&entry.candidate()));
    assert!(q("app is:system").matches(&entry.candidate()));
    assert!(q("app is:archived").matches(&entry.candidate()));
    assert!(!q("app is:compressed").matches(&entry.candidate()));
    assert!(q("app is:readonly is:system").matches(&entry.candidate()));
    assert!(!q("app is:readonly is:compressed").matches(&entry.candidate()));
}

/// Zero must read as "no" for every bit rather than as an answer about the file.
#[test]
fn a_row_with_no_attributes_matches_no_attribute_filter() {
    let unrecorded = Entry::file("ghost.txt").attributes(0);
    for text in [
        "ghost is:readonly",
        "ghost is:system",
        "ghost is:archived",
        "ghost is:link",
        "ghost is:compressed",
    ] {
        assert!(!q(text).matches(&unrecorded.candidate()), "{text}");
    }
}

/// `is:hidden` stays on the effective (inherited) column: a file inside a hidden folder is hidden,
/// and its own bit is clear.
#[test]
fn hidden_still_reads_the_effective_flag_not_the_mask() {
    assert!(
        q("report is:hidden").matches(
            &Entry::file("report.txt")
                .hidden(true)
                .attributes(ARCHIVE)
                .candidate()
        )
    );
    assert!(
        !q("report is:hidden").matches(
            &Entry::file("report.txt")
                .hidden(false)
                .attributes(HIDDEN)
                .candidate()
        )
    );
}

/// An unknown value is still an error, and the message lists what is on offer.
#[test]
fn an_unknown_is_value_is_refused() {
    let problem = SearchQuery::parse("is:sparkly").problem.expect("problem");
    assert!(problem.contains("is:readonly"));
    assert!(problem.contains("sparkly"));
}

// --- operators ---

#[test]
fn not_excludes() {
    let query = q("report !draft");
    assert!(query.matches(&Entry::file("Report.txt").candidate()));
    assert!(!query.matches(&Entry::file("Report-draft.txt").candidate()));
}

#[test]
fn and_binds_tighter_than_or() {
    // "a b OR c" is "(a AND b) OR c".
    let query = q("alpha beta OR gamma");
    assert!(query.matches(&Entry::file("alpha-beta.txt").candidate()));
    assert!(!query.matches(&Entry::file("alpha.txt").candidate()));
    assert!(query.matches(&Entry::file("gamma.txt").candidate()));
}

#[test]
fn brackets_group() {
    let query = q("(alpha OR gamma) ext:txt");
    assert!(query.matches(&Entry::file("alpha.txt").candidate()));
    assert!(query.matches(&Entry::file("gamma.txt").candidate()));
    assert!(!query.matches(&Entry::file("alpha.bin").candidate()));
    assert!(!query.matches(&Entry::file("delta.txt").candidate()));
}

#[test]
fn an_unclosed_bracket_closes_at_the_end() {
    // What a group looks like half-way through being typed.
    let query = q("(alpha OR gamma");
    assert!(query.matches(&Entry::file("alpha.txt").candidate()));
    assert!(!query.matches(&Entry::file("delta.txt").candidate()));
}

// --- path: and re: ---

#[test]
fn path_matches_the_folder_not_the_name() {
    let query = q("path:projects");
    assert!(query.matches(&Entry::in_dir("notes.txt", r"C:\Projects\Alpha").candidate()));
    assert!(!query.matches(&Entry::in_dir("notes.txt", r"C:\Archive").candidate()));
}

#[test]
fn regex() {
    let query = q(r"re:^img_\d+");
    assert!(query.matches(&Entry::file("IMG_0042.jpg").candidate()));
    assert!(!query.matches(&Entry::file("photo.jpg").candidate()));
}

/// A regular expression is the one term with no SQL, so it compiles to a superset and the caller
/// must be told it cannot trust the row set.
#[test]
fn a_regex_compiles_to_an_incomplete_predicate() {
    assert!(!q(r"re:^img_\d+").compile().complete);
}

#[test]
fn everything_else_compiles_to_an_exact_predicate() {
    assert!(
        q("report ext:txt size:>1kb dm:2026-06 !draft")
            .compile()
            .complete
    );
}

/// Negating a superset gives a subset, which would drop rows that really match. The exclusion of a
/// regex must therefore widen to "everything" and let the row re-check do the work.
#[test]
fn negating_a_regex_widens_rather_than_inverting() {
    let predicate = q("report !re:^img_").compile();
    assert!(!predicate.complete);
    assert!(!predicate.sql.contains("NOT"));

    // And the matcher, the definition, still excludes it.
    let query = q("report !re:^img_");
    assert!(query.matches(&Entry::file("report.txt").candidate()));
    assert!(!query.matches(&Entry::file("img_report.txt").candidate()));
}

// --- is:hidden ---

#[test]
fn only_a_hidden_query_asks_for_hidden_entries() {
    assert!(q("report is:hidden").wants_hidden());
    assert!(!q("report").wants_hidden());
    assert!(!q("report !is:hidden").wants_hidden());
}

// --- metadata dependence ---

#[test]
fn needs_metadata_tracks_size_and_date_terms() {
    for (text, expected) in [
        ("size:>1mb", true),
        ("dm:today", true),
        ("report ext:txt", false),
        ("is:dir report", false),
    ] {
        assert_eq!(expected, q(text).needs_metadata(), "{text}");
    }
}

// --- problems ---

#[test]
fn unusable_queries_come_back_as_a_message() {
    for text in [
        "size:>",
        "size:banana",
        "size:",
        "dm:banana",
        "dc:banana",
        "dc:",
        "ext:",
        "is:sideways",
        "re:(",
    ] {
        let parse = SearchQuery::parse(text);
        assert!(parse.query.is_none(), "{text}");
        assert!(
            parse
                .problem
                .as_deref()
                .is_some_and(|p| !p.trim().is_empty()),
            "{text}"
        );
    }
}

/// A key that plainly means something this index will not answer is refused rather than degraded
/// to a name term. `da:` is refused because accessed time is frozen on Windows, so the message must
/// talk about Windows rather than about what is indexed.
#[test]
fn known_but_unanswerable_keys_say_why() {
    for text in ["da:today", "dateaccessed:2026-06"] {
        let parse = SearchQuery::parse(text);
        assert!(parse.query.is_none(), "{text}");
        let problem = parse.problem.expect("problem");
        assert!(problem.to_lowercase().contains("accessed"), "{problem}");
        assert!(!problem.contains("isn't indexed"), "{problem}");
    }
}

/// The refusal list is not a place keys go to stay: `dc:` was on it, and is a working filter now.
#[test]
fn created_is_no_longer_refused() {
    assert!(SearchQuery::parse("dc:today").problem.is_none());
}

/// A pattern three characters away from catastrophic backtracking must not take the window with it.
#[test]
fn a_catastrophic_regex_is_budgeted_not_fatal() {
    let query = q("re:(a+)+$");
    let evil = format!("{}!", "a".repeat(40));
    let started = std::time::Instant::now();
    assert!(!query.matches(&Entry::file(&evil).candidate()));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn parsing_never_throws() {
    for text in [
        "((((",
        "))))",
        "!",
        "!!",
        "\"",
        "\"unclosed",
        "OR",
        "OR OR",
        "a OR",
        "size:>>1",
        "dm:..",
        "ext:;;",
        "re:",
        "re:[",
        ":",
        "::",
        "a:",
        "  :  ",
    ] {
        let _ = SearchQuery::parse(text); // must not panic
    }
}
