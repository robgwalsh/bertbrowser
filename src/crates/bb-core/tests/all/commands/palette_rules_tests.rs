// Port of tests/BertBrowser.Core.Tests/PaletteRulesTests.cs.
use bb_core::commands::{
    CommandCatalog, CommandContext, CommandProminence, MatchRange, PaletteEntry, PaletteRow,
    PaletteRowKind, PaletteRules, PaletteState, PaletteSuggestions,
};

fn entry(id: &str, name: &str, category: &str) -> PaletteEntry {
    PaletteEntry::new(id, name, category)
}

fn gesture(mut e: PaletteEntry, g: &str) -> PaletteEntry {
    e.gesture = g.to_string();
    e
}

fn unavailable(mut e: PaletteEntry, why: &str) -> PaletteEntry {
    e.unavailable = Some(why.to_string());
    e
}

fn entries() -> Vec<PaletteEntry> {
    let mut new_tab = gesture(entry("tab.new", "New tab", "Tabs"), "Ctrl+T");
    new_tab.prominence = CommandProminence::Essential;
    let mut flat = gesture(entry("view.flat", "Toggle flat view", "View"), "Ctrl+B");
    flat.aliases = vec!["branch view".to_string()];
    vec![
        new_tab,
        gesture(entry("tab.close", "Close tab", "Tabs"), "Ctrl+W"),
        unavailable(
            entry("tab.close-others", "Close other tabs", "Tabs"),
            "This is the only tab.",
        ),
        unavailable(
            gesture(entry("file.rename", "Rename", "File"), "F2"),
            "Nothing is selected.",
        ),
        gesture(
            entry("file.new-folder", "New folder", "File"),
            "Ctrl+Shift+N",
        ),
        flat,
        gesture(
            entry("view.preview", "Toggle preview pane", "View"),
            "Ctrl+P",
        ),
        entry("workspace:Work", "Work", "Workspaces"),
        entry("bookmark:C:\\Photos", "Photos", "Bookmarks"),
    ]
}

fn nothing() -> PaletteState {
    PaletteState::default()
}

fn show(rows: &[PaletteRow]) -> String {
    rows.iter()
        .map(|r| match r.kind {
            PaletteRowKind::Heading => format!("# {}", r.text),
            PaletteRowKind::Category => format!("{}{}", r.token, r.count),
            PaletteRowKind::Entry => {
                if r.entry.as_ref().unwrap().is_available() {
                    r.text.clone()
                } else {
                    format!("({})", r.text)
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn search(query: &str, state: &PaletteState) -> String {
    show(&PaletteRules::build(query, &entries(), state))
}

// --- Nothing typed ---

#[test]
fn an_empty_box_never_dumps_the_catalogue() {
    let rows = PaletteRules::build("", &entries(), &nothing());
    assert_eq!(
        "# All commands | tabs:3 | file:2 | view:2 | workspaces:1 | bookmarks:1",
        show(&rows)
    );
    assert!(!rows.iter().any(|r| r.kind == PaletteRowKind::Entry));
}

#[test]
fn pins_then_recents_then_suggestions_then_the_categories() {
    let state = PaletteState::new(
        &["view.flat"],
        &["tab.close", "view.flat"],
        &["tab.new", "tab.close", "file.new-folder"],
    );
    assert!(search("", &state).starts_with(
        "# Pinned | Toggle flat view | # Recent | Close tab | # Suggested | New tab | New folder | # All commands"
    ));
}

#[test]
fn a_pin_stays_put_when_it_cannot_run_but_a_recent_or_a_suggestion_does_not_appear() {
    let state = PaletteState::new(
        &["file.rename"],
        &["tab.close-others"],
        &["tab.close-others", "tab.new"],
    );
    assert!(
        search("", &state)
            .starts_with("# Pinned | (Rename) | # Suggested | New tab | # All commands")
    );
}

#[test]
fn recents_and_suggestions_are_capped() {
    let many: Vec<PaletteEntry> = (0..30)
        .map(|i| entry(&format!("x.{i}"), &format!("Thing {i}"), "X"))
        .collect();
    let ids: Vec<&str> = many.iter().map(|e| e.id.as_str()).collect();
    let rows = PaletteRules::build("", &many, &PaletteState::new(&[], &ids, &ids));
    assert_eq!(
        PaletteRules::MAX_RECENT + PaletteRules::MAX_SUGGESTED,
        rows.iter()
            .filter(|r| r.kind == PaletteRowKind::Entry)
            .count()
    );
}

#[test]
fn an_id_nothing_answers_to_is_skipped_not_fatal() {
    // A pin for a workspace since deleted, or a command a later build removed.
    let state = PaletteState::new(&["workspace:Gone", "tab.new"], &[], &[]);
    assert!(search("", &state).starts_with("# Pinned | New tab | # All commands"));
}

// --- Something typed ---

#[test]
fn the_best_match_is_first() {
    assert_eq!("New tab | New folder", search("new", &nothing()));
    // The same word in three names: the two that can run lead, the commoner of them first.
    assert_eq!(
        "New tab | Close tab | (Close other tabs)",
        search("tab", &nothing())
    );
}

#[test]
fn a_command_that_cannot_run_is_listed_under_one_that_matched_as_well_and_can() {
    assert_eq!(
        "Close tab | (Close other tabs)",
        search("close", &nothing())
    );
    assert_eq!("(Rename)", search("rename", &nothing()));
    let rows = PaletteRules::build("rename", &entries(), &nothing());
    assert_eq!(1, rows.len());
    assert_eq!(
        Some("Nothing is selected.".to_string()),
        rows[0].entry.as_ref().unwrap().unavailable
    );
}

#[test]
fn a_better_match_that_cannot_run_still_outranks_a_worse_one_that_can() {
    let entries = vec![
        entry("a", "Reopen closed tab", "Tabs"),
        unavailable(entry("b", "Open", "File"), "Nothing is selected."),
    ];
    // "open" is the whole of one name and only the middle of the other.
    assert_eq!(
        "(Open) | Reopen closed tab",
        show(&PaletteRules::build("open", &entries, &nothing()))
    );
}

#[test]
fn pins_and_recents_order_equals() {
    assert_eq!(
        "Toggle flat view | Toggle preview pane",
        search("toggle", &nothing())
    );
    assert_eq!(
        "Toggle preview pane | Toggle flat view",
        search("toggle", &PaletteState::new(&[], &["view.preview"], &[]))
    );
    assert_eq!(
        "Toggle preview pane | Toggle flat view",
        search("toggle", &PaletteState::new(&["view.preview"], &[], &[]))
    );
}

#[test]
fn found_by_another_word_for_it_and_by_its_shortcut() {
    assert_eq!("Toggle flat view", search("branch", &nothing()));
    assert_eq!("Toggle flat view", search("ctrl+b", &nothing()));
    assert_eq!("New folder", search("ctrl+shift", &nothing()));
}

#[test]
fn named_things_are_found_like_commands() {
    assert_eq!("Work", search("work", &nothing()));
    assert_eq!("Photos", search("photo", &nothing()));
}

#[test]
fn highlights_say_where_the_name_matched() {
    let rows = PaletteRules::build("close", &entries(), &nothing());
    assert_eq!("Close tab", rows[0].text);
    assert_eq!(vec![MatchRange::new(0, 5)], rows[0].highlights);
}

// --- Narrowing ---

#[test]
fn a_category_token_lists_that_category_whole_in_its_own_order() {
    assert_eq!(
        "New tab | Close tab | (Close other tabs)",
        search("tabs:", &nothing())
    );
    assert_eq!("Work", search("workspaces:", &nothing()));
}

#[test]
fn and_text_after_it_searches_inside() {
    assert_eq!(
        "Close tab | (Close other tabs)",
        search("tabs: close", &nothing())
    );
    assert_eq!("", search("view: close", &nothing()));
}

#[test]
fn any_unambiguous_start_of_a_category_will_do() {
    for (query, expected) in [
        ("book:", "Photos"),
        ("wo:", "Work"),
        ("VIEW:", "Toggle flat view | Toggle preview pane"),
    ] {
        assert_eq!(expected, search(query, &nothing()), "{query}");
    }
}

#[test]
fn a_drive_letter_is_never_a_category() {
    let (category, rest) =
        PaletteRules::split_category("c:\\windows", &["Custom".to_string(), "Tabs".to_string()]);
    assert_eq!(None, category);
    assert_eq!("c:\\windows", rest);
}

#[test]
fn a_start_shared_by_two_categories_narrows_to_neither() {
    let (category, _) =
        PaletteRules::split_category("ta: x", &["Tabs".to_string(), "Tags".to_string()]);
    assert_eq!(None, category);
    // Unless it is one of them exactly.
    assert_eq!(
        Some("Tabs".to_string()),
        PaletteRules::split_category("tabs: x", &["Tabs".to_string(), "Tabs extra".to_string()]).0
    );
}

#[test]
fn a_category_row_types_its_own_token() {
    assert_eq!("saved-searches:", PaletteRules::token("Saved searches"));
    let cats = vec!["Saved searches".to_string()];
    assert_eq!(
        Some("Saved searches".to_string()),
        PaletteRules::split_category("saved-searches: x", &cats).0
    );
    assert_eq!(
        Some("Saved searches".to_string()),
        PaletteRules::split_category("saved searches: x", &cats).0
    );
}

// --- Suggestions ---

#[test]
fn every_suggestion_is_a_real_command() {
    for id in PaletteSuggestions::all() {
        assert!(CommandCatalog::is_known(id), "{id}");
    }
}

#[test]
fn suggestions_follow_what_is_selected_and_where_the_window_is() {
    assert!(PaletteSuggestions::for_state(2, false).contains(&"file.rename"));
    assert!(!PaletteSuggestions::for_state(0, false).contains(&"file.rename"));
    assert!(PaletteSuggestions::for_state(0, false).contains(&"nav.address-bar"));
    assert!(!PaletteSuggestions::for_state(0, true).contains(&"tab.new"));
}

#[test]
fn every_suggestion_on_the_settings_page_works_there() {
    for id in PaletteSuggestions::for_state(0, true) {
        assert_eq!(
            CommandContext::App,
            CommandCatalog::find(id).unwrap().context,
            "{id}"
        );
    }
}
