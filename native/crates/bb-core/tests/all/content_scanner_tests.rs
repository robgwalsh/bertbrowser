// Port of tests/BertBrowser.Core.Tests/ContentScannerTests.cs. The reading pass: which candidates
// it opens, which it does not, and what it reports when it is stopped or runs out of budget. The
// reader is faked for determinism, not speed: a cancel has to land in the middle of the run every
// time, which against a real disk would mean a very large fixture and a race.
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use bb_core::FileTime;
use bb_core::cancel::{CancelToken, Cancelled};
use bb_core::data::SearchHit;
use bb_core::models::{ContentScanLimit, ContentScanProgress};
use bb_core::search::{
    ContentReader, ContentScanOutcome, ContentScanner, ContentSearchRules, ContentText, FileText,
    SearchQuery,
};

fn q(text: &str) -> SearchQuery {
    let parse = SearchQuery::parse(text);
    assert_eq!(parse.problem, None);
    parse.query.expect("query")
}

fn hit(name: &str) -> SearchHit {
    hit_kind(name, false)
}

fn hit_kind(name: &str, is_dir: bool) -> SearchHit {
    SearchHit {
        display_path: format!(r"C:\Data\{name}"),
        relative_dir_display: String::new(),
        name: name.to_string(),
        is_directory: is_dir,
        size_bytes: 100,
        modified_utc: FileTime::from_ymd(2026, 6, 1),
        hidden: false,
        attributes: 0,
        created_utc: FileTime::UNKNOWN,
        content_match: None,
    }
}

/// Runs before each file is answered: where a test plants its cancel.
type BeforeHook = Box<dyn Fn(&str) + Sync>;

/// A reader over a map, with a hook that runs before each file is answered. `None` for a name
/// means "not text"; a name absent from the map is unreadable.
struct FakeReader {
    files: HashMap<String, Option<String>>,
    before: Option<BeforeHook>,
    opened: Mutex<Vec<String>>,
}

impl FakeReader {
    fn new(files: &[(&str, Option<&str>)]) -> FakeReader {
        FakeReader {
            files: files
                .iter()
                .map(|(k, v)| (k.to_string(), v.map(str::to_string)))
                .collect(),
            before: None,
            opened: Mutex::new(Vec::new()),
        }
    }

    fn many(count: usize, name: impl Fn(usize) -> String, body: &str) -> FakeReader {
        FakeReader {
            files: (0..count)
                .map(|i| (name(i), Some(body.to_string())))
                .collect(),
            before: None,
            opened: Mutex::new(Vec::new()),
        }
    }

    fn before(mut self, hook: impl Fn(&str) + Sync + 'static) -> FakeReader {
        self.before = Some(Box::new(hook));
        self
    }

    fn opened(&self) -> Vec<String> {
        self.opened.lock().unwrap().clone()
    }
}

impl ContentReader for FakeReader {
    fn read(
        &self,
        path: &str,
        max_bytes: u64,
        cancel: &CancelToken,
    ) -> Result<FileText, Cancelled> {
        cancel.check()?;
        if let Some(before) = &self.before {
            before(path);
        }
        self.opened.lock().unwrap().push(path.to_string());

        let name = bb_paths::file_name(path);
        match self.files.get(name) {
            None => Ok(FileText::Unreadable),
            Some(None) => Ok(FileText::NotText),
            Some(Some(text)) => {
                let truncated = text.len() as u64 > max_bytes;
                let kept = if truncated {
                    text[..max_bytes as usize].to_string()
                } else {
                    text.clone()
                };
                Ok(FileText::Text(ContentText::new(kept, truncated)))
            }
        }
    }
}

fn run(
    reader: &dyn ContentReader,
    query: &SearchQuery,
    candidates: &[SearchHit],
) -> ContentScanOutcome {
    run_with(reader, query, candidates, 1000, false, &CancelToken::none())
}

fn run_with(
    reader: &dyn ContentReader,
    query: &SearchQuery,
    candidates: &[SearchHit],
    max_results: usize,
    candidates_truncated: bool,
    cancel: &CancelToken,
) -> ContentScanOutcome {
    ContentScanner::new(reader).scan(
        query,
        candidates,
        max_results,
        candidates_truncated,
        ContentSearchRules::MAX_BYTES_PER_FILE,
        None,
        None,
        cancel,
    )
}

fn names(outcome: &ContentScanOutcome) -> Vec<&str> {
    outcome.hits.iter().map(|h| h.name.as_str()).collect()
}

// --- what gets opened ---

#[test]
fn only_matching_files_survive() {
    let reader = FakeReader::new(&[("a.cs", Some("has TODO here")), ("b.cs", Some("clean"))]);
    let outcome = run(&reader, &q("content:TODO"), &[hit("a.cs"), hit("b.cs")]);

    assert_eq!(names(&outcome), ["a.cs"]);
    assert_eq!(outcome.report.files_read, 2);
}

#[test]
fn a_candidate_the_name_already_settled_is_never_opened() {
    // The budget argument, asserted. In "content:zzz OR ext:md" the .md is a hit on its name, and
    // opening it would spend the file ceiling re-establishing what was already known. Make the OR
    // node return NeedsContent past a settled Yes and this goes red.
    let reader = FakeReader::new(&[("a.cs", Some("nothing"))]);
    let outcome = run(
        &reader,
        &q("content:zzz OR ext:md"),
        &[hit("notes.md"), hit("a.cs")],
    );

    assert_eq!(names(&outcome), ["notes.md"]);
    assert!(!reader.opened().iter().any(|p| p.ends_with("notes.md")));
    assert_eq!(outcome.report.files_read, 1);
}

#[test]
fn a_directory_is_never_handed_to_the_reader() {
    // Directories are settled No by the term itself, so the reader must not see one at all.
    let reader = FakeReader::new(&[("a.cs", Some("TODO"))]);
    let outcome = run(
        &reader,
        &q("content:TODO"),
        &[hit_kind("sub", true), hit("a.cs")],
    );

    assert_eq!(names(&outcome), ["a.cs"]);
    assert!(!reader.opened().iter().any(|p| p.ends_with("sub")));
}

#[test]
fn a_conjunction_that_is_already_refused_opens_nothing_at_all() {
    // "is:dir content:x": every candidate is settled No before any I/O.
    let reader = FakeReader::new(&[("a.cs", Some("TODO"))]);
    let outcome = run(
        &reader,
        &q("is:dir content:TODO"),
        &[hit_kind("sub", true), hit("a.cs")],
    );

    assert!(outcome.hits.is_empty());
    assert!(reader.opened().is_empty());
}

#[test]
fn a_negated_content_term_is_answered_by_reading_too() {
    let reader = FakeReader::new(&[("a.cs", Some("has TODO")), ("b.cs", Some("clean"))]);
    let outcome = run(
        &reader,
        &q("ext:cs !content:TODO"),
        &[hit("a.cs"), hit("b.cs")],
    );

    assert_eq!(names(&outcome), ["b.cs"]);
}

// --- the snippet ---

#[test]
fn a_hit_carries_the_line_it_matched_on() {
    let reader = FakeReader::new(&[("a.cs", Some("one\ntwo\nthree TODO four"))]);
    let outcome = run(&reader, &q("content:TODO"), &[hit("a.cs")]);

    let m = outcome.hits[0].content_match.as_ref().expect("a match");
    assert_eq!(m.line_number, 3);
    assert_eq!(m.line, "three TODO four");
}

#[test]
fn a_hit_settled_by_its_name_carries_no_line() {
    // There is no line to point at: the file was never opened. Inventing one would be a lie about
    // where the match was.
    let reader = FakeReader::new(&[("a.cs", Some("x"))]);
    let outcome = run(&reader, &q("content:zzz OR ext:md"), &[hit("notes.md")]);

    assert!(outcome.hits[0].content_match.is_none());
}

// --- failures vs cancels ---

#[test]
fn one_unreadable_file_costs_the_others_nothing() {
    let reader = FakeReader::new(&[("b.cs", Some("TODO here"))]); // a.cs missing => unreadable
    let outcome = run(&reader, &q("content:TODO"), &[hit("a.cs"), hit("b.cs")]);

    assert_eq!(names(&outcome), ["b.cs"]);
    assert_eq!(outcome.report.unreadable, 1);
    assert!(outcome.report.incomplete());
    assert!(!outcome.cancelled);
}

#[test]
fn a_binary_candidate_is_not_counted_as_unreadable() {
    // Nothing went wrong: it simply has no text. Counting it would make every search of a folder
    // with images report itself incomplete, and the word would stop meaning anything.
    let reader = FakeReader::new(&[("a.png", None), ("b.cs", Some("TODO"))]);
    let outcome = run(&reader, &q("content:TODO"), &[hit("a.png"), hit("b.cs")]);

    assert_eq!(names(&outcome), ["b.cs"]);
    assert_eq!(outcome.report.unreadable, 0);
}

#[test]
fn a_cancel_mid_run_gives_back_what_was_already_found() {
    // A floor, not nothing. Return an empty list on a cancel and the results the user was watching
    // fill up are blanked at the moment they press Escape.
    let cancel = CancelToken::new();
    let seen = AtomicUsize::new(0);
    let reader = {
        let cancel = cancel.clone();
        FakeReader::many(200, |i| format!("f{i}.cs"), "TODO here").before(move |_| {
            if seen.fetch_add(1, Ordering::SeqCst) + 1 == 20 {
                cancel.cancel();
            }
        })
    };

    let candidates: Vec<SearchHit> = (0..200).map(|i| hit(&format!("f{i}.cs"))).collect();
    let outcome = run_with(
        &reader,
        &q("content:TODO"),
        &candidates,
        1000,
        false,
        &cancel,
    );

    assert!(outcome.cancelled);
    assert!(!outcome.hits.is_empty());
    assert!(
        outcome.hits.len() < 200,
        "a cancelled run must be short of the whole set"
    );
}

#[test]
fn a_cancel_is_not_reported_as_a_pile_of_unreadable_files() {
    let cancel = CancelToken::new();
    let reader = {
        let cancel = cancel.clone();
        FakeReader::many(100, |i| format!("f{i}.cs"), "TODO").before(move |_| cancel.cancel())
    };
    let candidates: Vec<SearchHit> = (0..100).map(|i| hit(&format!("f{i}.cs"))).collect();

    let outcome = run_with(
        &reader,
        &q("content:TODO"),
        &candidates,
        1000,
        false,
        &cancel,
    );

    assert!(outcome.cancelled);
    assert_eq!(outcome.report.unreadable, 0);
}

// --- ceilings ---

#[test]
fn reaching_the_candidate_ceiling_is_reported_rather_than_looking_like_no_more_results() {
    let reader = FakeReader::new(&[("a.cs", Some("TODO"))]);
    let outcome = run_with(
        &reader,
        &q("content:TODO"),
        &[hit("a.cs")],
        1000,
        true,
        &CancelToken::none(),
    );

    assert_eq!(outcome.report.limit, ContentScanLimit::Candidates);
    assert!(outcome.report.incomplete());
}

#[test]
fn the_report_names_where_it_got_to() {
    // "Searched the first 50,000 files" is true and still reads as "your PC has no such file".
    // Naming the last path examined is what makes it a bound rather than a lie.
    let reader = FakeReader::new(&[("a.cs", Some("TODO")), ("b.cs", Some("TODO"))]);
    let outcome = run(&reader, &q("content:TODO"), &[hit("a.cs"), hit("b.cs")]);

    assert!(outcome.report.last_path_examined.is_some());
}

#[test]
fn a_file_longer_than_its_budget_is_counted_so_the_answer_is_known_to_be_a_floor() {
    let body = "x".repeat(ContentSearchRules::MAX_BYTES_PER_FILE as usize + 10);
    let reader = FakeReader::new(&[("big.cs", Some(&body))]);
    let outcome = run(&reader, &q("content:TODO"), &[hit("big.cs")]);

    assert_eq!(outcome.report.truncated, 1);
    assert!(outcome.report.incomplete());
}

#[test]
fn the_result_cap_is_separate_from_the_file_ceiling() {
    let reader = FakeReader::many(50, |i| format!("f{i}.cs"), "TODO");
    let candidates: Vec<SearchHit> = (0..50).map(|i| hit(&format!("f{i}.cs"))).collect();

    let outcome = run_with(
        &reader,
        &q("content:TODO"),
        &candidates,
        10,
        false,
        &CancelToken::none(),
    );

    assert_eq!(outcome.hits.len(), 10);
    assert!(outcome.truncated);
    assert_eq!(outcome.report.files_read, 50); // every candidate was still examined
}

// --- determinism ---

#[test]
fn two_identical_runs_return_the_same_order() {
    // The workers finish in whatever order the disk answers. A result list that reshuffled between
    // two identical searches would be impossible to act on.
    let name = |i: usize| format!("f{i:02}.cs");
    let candidates: Vec<SearchHit> = (0..60).map(|i| hit(&name(i))).collect();
    let reversed: Vec<SearchHit> = candidates.iter().rev().cloned().collect();

    let first = run(
        &FakeReader::many(60, name, "TODO"),
        &q("content:TODO"),
        &candidates,
    );
    let second = run(
        &FakeReader::many(60, name, "TODO"),
        &q("content:TODO"),
        &reversed,
    );

    assert_eq!(names(&first), names(&second));
}

// --- progress ---

#[test]
fn progress_always_reports_at_the_finish() {
    let reports: Mutex<Vec<ContentScanProgress>> = Mutex::new(Vec::new());
    let reader = FakeReader::new(&[("a.cs", Some("TODO"))]);

    ContentScanner::new(&reader).scan(
        &q("content:TODO"),
        &[hit("a.cs")],
        1000,
        false,
        ContentSearchRules::MAX_BYTES_PER_FILE,
        None,
        Some(&|p| reports.lock().unwrap().push(p)),
        &CancelToken::none(),
    );

    assert!(!reports.lock().unwrap().is_empty());
}
