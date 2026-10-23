// Port of tests/BertBrowser.Core.Tests/SavedCommandBarRulesTests.cs.
use bb_core::commandbars::{
    BarItem, CommandBarDefaults, CommandBarRules, CommandBarSet, SavedCommandBarRules,
    SavedCommandBarSet, ToolbarSpec,
};

fn no_name_taken(_: &str) -> bool {
    false
}

fn with(command: &str) -> CommandBarSet {
    CommandBarSet {
        revision: 1,
        menus: vec![BarItem::menu("&File", vec![BarItem::command(command)])],
        title_bar: ToolbarSpec::new(vec![]),
        rows: vec![],
    }
}

fn saved(name: &str) -> SavedCommandBarSet {
    saved_with(name, "tab.new")
}

fn saved_with(name: &str, command: &str) -> SavedCommandBarSet {
    SavedCommandBarSet::new(name, with(command))
}

fn names(list: &[SavedCommandBarSet]) -> Vec<&str> {
    list.iter().map(|s| s.name.as_str()).collect()
}

#[test]
fn a_name_is_required_and_kept_short() {
    assert!(SavedCommandBarRules::validate("   ", no_name_taken).is_some());
    assert!(
        SavedCommandBarRules::validate(
            &"x".repeat(SavedCommandBarRules::MAX_NAME_LENGTH + 1),
            no_name_taken
        )
        .is_some()
    );
    assert_eq!(
        None,
        SavedCommandBarRules::validate(" Minimal ", no_name_taken)
    );
}

#[test]
fn a_taken_name_is_refused_by_its_trimmed_spelling() {
    let error = SavedCommandBarRules::validate(" Minimal ", |name| name == "Minimal").unwrap();

    assert!(error.contains("\"Minimal\""), "{error}");
}

#[test]
fn saving_under_a_name_already_there_replaces_it_and_takes_the_new_casing() {
    let list = SavedCommandBarRules::upsert(
        &[saved("work"), saved("Alpha")],
        saved_with(" WORK ", "tab.close"),
    );

    assert_eq!(vec!["Alpha", "WORK"], names(&list));
    assert!(CommandBarRules::same(&with("tab.close"), &list[1].set));
}

#[test]
fn a_rename_moves_the_arrangement_and_refuses_another_ones_name() {
    let list = [saved("Alpha"), saved("Work")];

    assert_eq!(
        vec!["Alpha", "Zed"],
        names(&SavedCommandBarRules::rename(&list, "work", " Zed ").unwrap())
    );
    assert_eq!(
        vec!["Alpha", "WORK"],
        names(&SavedCommandBarRules::rename(&list, "Work", "WORK").unwrap())
    );
    assert_eq!(None, SavedCommandBarRules::rename(&list, "Work", "alpha"));
    assert_eq!(None, SavedCommandBarRules::rename(&list, "Missing", "Zed"));
}

#[test]
fn removing_ignores_case() {
    assert_eq!(
        vec!["Alpha"],
        names(&SavedCommandBarRules::remove(
            &[saved("Alpha"), saved("Work")],
            "WORK"
        ))
    );
}

#[test]
fn the_current_arrangement_is_recognised_as_the_saved_one_it_equals() {
    let list = [
        saved_with("Alpha", "tab.new"),
        saved_with("Work", "tab.close"),
    ];

    assert_eq!(
        Some("Work"),
        SavedCommandBarRules::name_matching(&list, &with("tab.close"))
    );
    assert_eq!(
        None,
        SavedCommandBarRules::name_matching(&list, &with("nav.up"))
    );
}

#[test]
fn only_an_arrangement_kept_nowhere_else_needs_asking_about() {
    let list = [saved_with("Work", "tab.close")];

    assert!(!SavedCommandBarRules::needs_confirm_before_replace(
        CommandBarDefaults::default_set(),
        &list
    ));
    assert!(!SavedCommandBarRules::needs_confirm_before_replace(
        &with("tab.close"),
        &list
    ));
    assert!(SavedCommandBarRules::needs_confirm_before_replace(
        &with("nav.up"),
        &list
    ));
}

#[test]
fn the_shape_counts_the_title_bar_as_a_toolbar() {
    assert_eq!(
        "6 menus, 1 toolbar",
        SavedCommandBarRules::shape_text(CommandBarDefaults::default_set())
    );
    assert_eq!(
        "1 menu, 2 toolbars",
        SavedCommandBarRules::shape_text(&CommandBarSet {
            rows: vec![ToolbarSpec::new(vec![])],
            ..with("tab.new")
        })
    );
}
