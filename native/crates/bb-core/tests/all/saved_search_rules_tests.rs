// Port of tests/BertBrowser.Core.Tests/SavedSearchRulesTests.cs.
use std::cell::RefCell;

use bb_core::data::{SavedSearch, SavedSearchScope};
use bb_core::rules::{SavedSearchRules, SavedSearchRun};
use bb_core::search::SearchGrammar;

struct Case {
    name: &'static str,
    query: &'static str,
    scope: SavedSearchScope,
    scope_path: Option<&'static str>,
}

fn case() -> Case {
    Case {
        name: "Docs",
        query: "ext:docx",
        scope: SavedSearchScope::CurrentFolder,
        scope_path: None,
    }
}

impl Case {
    fn validate(&self) -> Option<String> {
        self.validate_with(|_| false, |_| false)
    }

    fn validate_with(
        &self,
        name_taken: impl Fn(&str) -> bool,
        is_archive: impl Fn(&str) -> bool,
    ) -> Option<String> {
        SavedSearchRules::validate(
            self.name,
            self.query,
            self.scope,
            self.scope_path,
            name_taken,
            is_archive,
        )
    }
}

fn zip_is_archive(p: &str) -> bool {
    p.to_ascii_lowercase().ends_with(".zip")
}

// --- Validate ---

#[test]
fn a_valid_search_has_no_problem() {
    assert_eq!(None, case().validate());
}

#[test]
fn a_blank_name_is_refused() {
    for name in ["", "   "] {
        let mut c = case();
        c.name = name;
        assert!(
            c.validate().unwrap().to_lowercase().contains("name"),
            "{name:?}"
        );
    }
}

#[test]
fn a_taken_name_is_refused() {
    let problem = case().validate_with(|n| n == "Docs", |_| false).unwrap();
    assert!(problem.contains("Docs"));
}

#[test]
fn the_name_is_trimmed_before_the_taken_check() {
    let asked = RefCell::new(Vec::new());
    let mut c = case();
    c.name = "  Docs  ";
    c.validate_with(
        |n| {
            asked.borrow_mut().push(n.to_string());
            false
        },
        |_| false,
    );
    assert_eq!(vec!["Docs".to_string()], *asked.borrow());
}

#[test]
fn a_blank_query_is_refused_as_blank_not_as_too_broad() {
    for query in ["", "   "] {
        let mut c = case();
        c.query = query;
        assert!(c.validate().unwrap().contains("Type a search"), "{query:?}");
    }
}

#[test]
fn a_grammar_problem_is_passed_through_verbatim() {
    let expected = SearchGrammar::parse("da:2026", true)
        .problem
        .expect("problem");
    let mut c = case();
    c.query = "da:2026";
    assert_eq!(Some(expected), c.validate());
}

#[test]
fn a_too_broad_query_is_refused() {
    for query in ["a", "is:dir"] {
        let mut c = case();
        c.query = query;
        assert!(c.validate().unwrap().contains("two characters"), "{query}");
    }
}

#[test]
fn folder_scope_without_a_path_is_refused() {
    let mut c = case();
    c.scope = SavedSearchScope::Folder;
    assert!(c.validate().is_some());
}

#[test]
fn folder_scope_with_a_blank_path_is_refused() {
    let mut c = case();
    c.scope = SavedSearchScope::Folder;
    c.scope_path = Some("  ");
    assert!(c.validate().is_some());
}

#[test]
fn a_folder_inside_an_archive_cannot_be_pinned() {
    let mut c = case();
    c.scope = SavedSearchScope::Folder;
    c.scope_path = Some(r"C:\x\a.zip\src");
    let problem = c.validate_with(|_| false, zip_is_archive).unwrap();
    assert!(problem.to_lowercase().contains("archive"));
}

#[test]
fn a_real_folder_whose_name_looks_like_an_archive_can_be_pinned() {
    // The probe says C:\x.zip is not a file, so it is a folder that happens to be called that.
    let mut c = case();
    c.scope = SavedSearchScope::Folder;
    c.scope_path = Some(r"C:\x.zip\real");
    assert_eq!(None, c.validate());
}

#[test]
fn only_folder_scope_carries_a_path() {
    for scope in [SavedSearchScope::CurrentFolder, SavedSearchScope::ThisPc] {
        let mut c = case();
        c.scope = scope;
        c.scope_path = Some(r"C:\x");
        assert!(c.validate().is_some(), "{scope:?}");
    }
}

#[test]
fn the_first_problem_wins_in_order() {
    // Blank name outranks a bad query; a bad query outranks a missing folder.
    let mut c = case();
    c.name = "";
    c.query = "da:2026";
    c.scope = SavedSearchScope::Folder;
    assert!(c.validate().unwrap().to_lowercase().contains("name"));
    let mut d = case();
    d.query = "da:2026";
    d.scope = SavedSearchScope::Folder;
    assert!(!d.validate().unwrap().to_lowercase().contains("folder"));
}

// --- default_name ---

#[test]
fn default_name_is_the_trimmed_query() {
    assert_eq!(
        "ext:jpg size:>1mb",
        SavedSearchRules::default_name("  ext:jpg   size:>1mb ")
    );
}

#[test]
fn default_name_is_capped() {
    let name = SavedSearchRules::default_name(&"x".repeat(200));
    assert!(name.chars().count() <= 40, "{} chars", name.chars().count());
}

#[test]
fn default_name_falls_back_when_the_query_is_blank() {
    assert_eq!("Saved search", SavedSearchRules::default_name("   "));
}

// --- plan ---

#[test]
fn a_template_runs_in_the_current_folder() {
    let run = SavedSearchRules::plan(
        &SavedSearch::new("T", "ext:txt", SavedSearchScope::CurrentFolder, None),
        r"C:\here",
    );
    assert_eq!(Some(SavedSearchRun::new(r"C:\here", "ext:txt", false)), run);
}

#[test]
fn a_pinned_search_runs_in_its_folder_wherever_you_are() {
    let run = SavedSearchRules::plan(
        &SavedSearch::new("P", "ext:txt", SavedSearchScope::Folder, Some(r"C:\pinned")),
        r"C:\here",
    );
    assert_eq!(
        Some(SavedSearchRun::new(r"C:\pinned", "ext:txt", false)),
        run
    );
}

#[test]
fn a_this_pc_search_is_global_but_still_needs_the_tab_to_have_a_folder() {
    let run = SavedSearchRules::plan(
        &SavedSearch::new("G", "ext:txt", SavedSearchScope::ThisPc, None),
        r"C:\here",
    );
    assert_eq!(Some(SavedSearchRun::new(r"C:\here", "ext:txt", true)), run);
}

#[test]
fn with_no_current_folder_there_is_nowhere_to_run() {
    for scope in [SavedSearchScope::CurrentFolder, SavedSearchScope::ThisPc] {
        assert_eq!(
            None,
            SavedSearchRules::plan(&SavedSearch::new("X", "ext:txt", scope, None), ""),
            "{scope:?}"
        );
    }
}

#[test]
fn a_pinned_search_still_plans_with_no_current_folder() {
    let run = SavedSearchRules::plan(
        &SavedSearch::new("P", "ext:txt", SavedSearchScope::Folder, Some(r"C:\pinned")),
        "",
    );
    assert_eq!(Some(r"C:\pinned".to_string()), run.map(|r| r.navigate_to));
}
