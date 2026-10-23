// Port of tests/BertBrowser.Core.Tests/ContentTermTests.cs: the content: term and the three-valued
// logic it exists for. Collapse NeedsContent to either boolean in AND, OR or NOT and something here
// goes red.
use bb_core::search::{SearchMatch, SearchQuery};

use crate::support::Entry;

fn q(text: &str) -> SearchQuery {
    let parse = SearchQuery::parse(text);
    assert!(parse.problem.is_none(), "{text}: {:?}", parse.problem);
    parse.query.unwrap_or_else(|| panic!("{text}"))
}

fn file(name: &str) -> Entry {
    Entry::file(name)
}

fn read(name: &str, content: &str) -> Entry {
    Entry::file(name).content(content)
}

// --- the term itself ---

#[test]
fn an_unread_file_is_undecided_rather_than_a_miss_or_a_hit() {
    // Answer No here and the first pass yields nothing for every content query.
    assert_eq!(
        SearchMatch::NeedsContent,
        q("content:todo").evaluate(&file("a.cs").candidate())
    );
}

#[test]
fn an_unread_file_still_counts_as_a_possible_match_for_the_shortlist() {
    assert!(q("content:todo").matches(&file("a.cs").candidate()));
}

#[test]
fn a_read_file_is_settled_either_way() {
    for (content, expected) in [
        ("hello TODO world", true),
        ("hello todo world", true),
        ("nothing to see", false),
        ("", false),
    ] {
        let verdict = q("content:TODO").evaluate(&read("a.cs", content).candidate());
        assert_eq!(
            if expected {
                SearchMatch::Yes
            } else {
                SearchMatch::No
            },
            verdict,
            "{content:?}"
        );
    }
}

#[test]
fn the_needle_keeps_the_case_it_was_typed_in() {
    assert_eq!(vec!["MixedCase"], q("content:MixedCase").content_needles());
}

#[test]
fn every_needle_is_collected_wherever_it_sits_in_the_tree() {
    assert_eq!(
        vec!["alpha", "beta", "gamma"],
        q("(content:alpha OR content:beta) !content:gamma").content_needles()
    );
}

#[test]
fn a_directory_is_settled_no_without_ever_being_read() {
    assert_eq!(
        SearchMatch::No,
        q("content:todo").evaluate(&Entry::dir("sub").candidate())
    );
}

// --- AND ---

#[test]
fn one_settled_no_refuses_the_whole_conjunction_without_reading() {
    assert_eq!(
        SearchMatch::No,
        q("is:dir content:todo").evaluate(&Entry::dir("sub").candidate())
    );
    assert_eq!(
        SearchMatch::No,
        q("is:dir content:todo").evaluate(&file("a.cs").candidate())
    );
}

#[test]
fn a_conjunction_is_undecided_only_when_everything_else_agrees() {
    assert_eq!(
        SearchMatch::NeedsContent,
        q("report content:todo").evaluate(&file("report.cs").candidate())
    );
    assert_eq!(
        SearchMatch::No,
        q("report content:todo").evaluate(&file("other.cs").candidate())
    );
}

// --- OR ---

#[test]
fn one_settled_yes_satisfies_the_disjunction_without_reading() {
    assert_eq!(
        SearchMatch::Yes,
        q("content:todo OR ext:md").evaluate(&file("notes.md").candidate())
    );
}

#[test]
fn a_disjunction_with_nothing_settled_is_still_undecided() {
    assert_eq!(
        SearchMatch::NeedsContent,
        q("content:todo OR ext:md").evaluate(&file("code.cs").candidate())
    );
}

#[test]
fn a_disjunction_settles_no_only_when_every_branch_does() {
    assert_eq!(
        SearchMatch::No,
        q("content:todo OR ext:md").evaluate(&read("code.cs", "clean").candidate())
    );
}

// --- NOT ---

#[test]
fn negation_leaves_an_undecided_child_undecided() {
    assert_eq!(
        SearchMatch::NeedsContent,
        q("report !content:todo").evaluate(&file("report.cs").candidate())
    );
}

#[test]
fn negation_flips_a_settled_child() {
    assert_eq!(
        SearchMatch::No,
        q("report !content:todo").evaluate(&read("report.cs", "a TODO here").candidate())
    );
    assert_eq!(
        SearchMatch::Yes,
        q("report !content:todo").evaluate(&read("report.cs", "clean").candidate())
    );
}

#[test]
fn two_content_terms_both_have_to_be_satisfied() {
    let query = q("content:alpha content:beta");
    assert_eq!(
        SearchMatch::NeedsContent,
        query.evaluate(&file("a.cs").candidate())
    );
    assert_eq!(
        SearchMatch::Yes,
        query.evaluate(&read("a.cs", "alpha and beta").candidate())
    );
    assert_eq!(
        SearchMatch::No,
        query.evaluate(&read("a.cs", "alpha only").candidate())
    );
}

// --- the query-level flag ---

#[test]
fn needs_content_is_what_decides_whether_the_second_pass_runs() {
    for (text, expected) in [
        ("content:todo", true),
        ("report content:todo", true),
        ("content:todo OR report", true),
        ("report !content:todo", true), // deliberately unlike wants_hidden/wants_archives
        ("report", false),
        ("ext:md size:>1kb", false),
    ] {
        assert_eq!(expected, q(text).needs_content(), "{text}");
    }
}

/// `!content:x` propagates where `!is:hidden` and `!in:archives` do not.
#[test]
fn negation_propagates_needs_content_but_not_the_other_two_scopes() {
    assert!(q("report !content:todo").needs_content());
    assert!(!q("report !is:hidden").wants_hidden());
    assert!(!q("report !in:archives").wants_archives());
}

// --- the SQL face ---

#[test]
fn a_content_term_compiles_to_an_incomplete_predicate() {
    assert!(!q("content:todo").compile().complete);
}

#[test]
fn negating_a_content_term_does_not_invert_its_sql() {
    let predicate = q("report !content:todo").compile();
    assert!(!predicate.complete);
    assert!(!predicate.sql.contains("NOT"));
}
