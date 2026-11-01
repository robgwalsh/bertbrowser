use std::collections::HashSet;

use bb_core::settings::{SettingsEntry, SettingsIndex};

const PAGES: [(&str, &str); 13] = [
    ("general", "General"),
    ("appearance", "Appearance"),
    ("preview", "Preview"),
    ("search-index", "Search index"),
    ("history", "History"),
    ("undo", "Undo/Redo"),
    ("new-items", "New items"),
    ("columns", "Columns"),
    ("saved-searches", "Saved searches"),
    ("workspaces", "Workspaces"),
    ("context-menu", "Context menu"),
    ("command-bars", "Menus and toolbars"),
    ("keyboard", "Keyboard"),
];

fn page_name(id: &str) -> &'static str {
    PAGES
        .iter()
        .find(|p| p.0 == id)
        .map(|p| p.1)
        .unwrap_or_else(|| panic!("no page '{id}'"))
}

fn find(query: &str) -> Vec<&'static SettingsEntry> {
    SettingsIndex::find(query, page_name)
}

fn labels(query: &str) -> Vec<&'static str> {
    find(query).iter().map(|e| e.label).collect()
}

fn position(found: &[&str], label: &str) -> usize {
    found
        .iter()
        .position(|l| *l == label)
        .unwrap_or_else(|| panic!("'{label}' is not in {found:?}"))
}

#[test]
fn a_blank_query_finds_nothing() {
    assert!(find("").is_empty());
    assert!(find("   ").is_empty());
}

#[test]
fn nothing_matching_finds_nothing() {
    assert!(find("zzzzqqq").is_empty());
}

#[test]
fn the_label_is_found_whatever_the_case() {
    assert_eq!(labels("hidden")[0], "Show hidden items");
    assert_eq!(labels("HIDDEN")[0], "Show hidden items");
}

#[test]
fn every_word_typed_has_to_be_found() {
    assert_eq!(labels("show hidden")[0], "Show hidden items");
    assert!(find("hidden zzzzqqq").is_empty());
}

#[test]
fn another_name_for_a_setting_finds_it() {
    assert_eq!(labels("dark mode")[0], "Match Windows light/dark");
    assert_eq!(labels("hotkeys")[0], "Keyboard");
    assert_eq!(labels("dotfiles")[0], "Show hidden items");
    assert_eq!(labels("right click")[0], "Context menu");
}

#[test]
fn a_label_beats_another_name_and_another_name_beats_the_page_name() {
    // "Keyboard" is one row's label and the other Keyboard rows' page.
    let found = labels("keyboard");
    assert_eq!(found[0], "Keyboard");
    assert!(found.contains(&"Reset all"));

    // In "History" the word is the label; "Undo/Redo" only has it as another name; the History
    // page's rows only have it as their page's name.
    let found = labels("history");
    assert!(position(&found, "History") < position(&found, "Undo/Redo"));
    assert!(position(&found, "Undo/Redo") < position(&found, "Keep changes for"));
}

#[test]
fn every_page_is_indexed_and_every_entry_is_on_a_page() {
    for (id, _) in PAGES {
        assert!(
            SettingsIndex::ENTRIES.iter().any(|e| e.page == id),
            "nothing on the '{id}' page is indexed"
        );
    }
    // Every entry's page is a real page: this panics otherwise.
    for e in SettingsIndex::ENTRIES {
        page_name(e.page);
    }
}

#[test]
fn no_page_lists_a_label_twice() {
    let mut seen = HashSet::new();
    for e in SettingsIndex::ENTRIES {
        assert!(
            seen.insert((e.page, e.label)),
            "'{}' is listed twice",
            e.label
        );
    }
}
