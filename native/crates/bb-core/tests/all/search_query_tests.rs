// Port of tests/BertBrowser.Core.Tests/SearchQueryTests.cs: what a bare query has always meant. The
// point of the file is that adding ext: and friends did not quietly change what an ordinary search does.
use bb_core::search::SearchQuery;
use rusqlite::types::Value;

use crate::support::Entry;

fn q(text: &str) -> Option<SearchQuery> {
    SearchQuery::parse(text).query
}

fn file(name: &str) -> Entry {
    Entry::file(name).size(0)
}

#[test]
fn parse_rejects_empty_or_too_broad() {
    for text in ["", "   ", "a", "a*", "* ?"] {
        assert!(q(text).is_none(), "{text:?}");
    }
}

#[test]
fn parse_accepts_two_literal_chars() {
    for text in ["ab", ".c", "a b"] {
        assert!(q(text).is_some(), "{text:?}");
    }
}

#[test]
fn parse_uppercases_and_splits_on_whitespace() {
    let query = q("  proj\treport ").unwrap();
    assert!(query.matches(&file("Project-Report-2026.docx").candidate()));
    assert!(!query.matches(&file("Project-Plan.docx").candidate()));
}

#[test]
fn compile_wraps_in_stars_and_escapes_open_bracket() {
    let values = |text: &str| -> Vec<String> {
        q(text)
            .unwrap()
            .compile()
            .parameters
            .into_iter()
            .filter_map(|(_, v)| match v {
                Value::Text(s) => Some(s),
                _ => None,
            })
            .collect()
    };
    assert!(values("a[1]b").contains(&"*A[[]1]B*".to_string()));
    assert!(values("report").contains(&"*REPORT*".to_string()));
}

#[test]
fn matches_substring_wildcards_and_and() {
    for (query_text, name, expected) in [
        ("rep", "Report.docx", true), // substring, case-insensitive
        ("rep", "preparation.txt", true),
        ("rep", "notes.txt", false),
        ("proj rep", "Project-Report-2026.docx", true), // AND terms
        ("proj rep", "Project-Plan.docx", false),
        ("*.txt", "notes.txt", true),     // explicit star
        ("*.txt", "notes.txt.bak", true), // substring semantics: still contains ".txt"
        ("?eport", "Report.docx", true),  // single-char wildcard
        ("x?z", "xyz-file.bin", true),
        ("x?z", "xz-file.bin", false),
        ("a[1]", "a[1].txt", true), // literal brackets
    ] {
        assert_eq!(
            expected,
            q(query_text).unwrap().matches(&file(name).candidate()),
            "{query_text} {name}"
        );
    }
}

#[test]
fn matches_folds_non_ascii_like_to_upper_invariant() {
    // SQLite NOCASE would miss this; our folding must not.
    assert!(
        q("übung")
            .unwrap()
            .matches(&file("Übung-01.pdf").candidate())
    );
}

// --- What the filter syntax must NOT have taken away ---

#[test]
fn an_unrecognised_key_stays_a_literal_name_term() {
    for text in [r"C:\Users", "time:30", "ratio 16:9"] {
        let query = q(text).unwrap_or_else(|| panic!("{text}"));
        // It searches for the text as typed, colon and all.
        let a = file(&format!("{}.txt", text.replace(r"C:\", "C-")));
        let b = file(&format!("{text}.txt"));
        assert!(
            query.matches(&a.candidate()) || query.matches(&b.candidate()),
            "{text}"
        );
    }
}

#[test]
fn lowercase_or_is_a_word_not_an_operator() {
    let query = q("report or draft").unwrap();
    assert!(query.matches(&file("Report or Draft.txt").candidate()));
    assert!(!query.matches(&file("Report.txt").candidate()));
}

#[test]
fn uppercase_or_is_an_operator() {
    let query = q("report OR draft").unwrap();
    assert!(query.matches(&file("Report.txt").candidate()));
    assert!(query.matches(&file("Draft.txt").candidate()));
    assert!(!query.matches(&file("Notes.txt").candidate()));
}

#[test]
fn a_trailing_exclamation_mark_is_literal() {
    for text in ["report!", "hello!!"] {
        assert!(
            q(text)
                .unwrap()
                .matches(&file(&format!("{text}.txt")).candidate()),
            "{text}"
        );
    }
}

#[test]
fn a_quoted_phrase_keeps_its_spaces_and_drops_wildcards() {
    let query = q("\"annual report\"").unwrap();
    assert!(query.matches(&file("The annual report.docx").candidate()));
    assert!(!query.matches(&file("annual-report.docx").candidate()));

    let stars = q("\"a*b\"").unwrap();
    assert!(stars.matches(&file("a*b.txt").candidate()));
    assert!(!stars.matches(&file("axxb.txt").candidate()));
}
