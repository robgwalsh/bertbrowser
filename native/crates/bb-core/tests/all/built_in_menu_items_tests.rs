// Port of tests/BertBrowser.Core.Tests/BuiltInMenuItemsTests.cs (BuiltInMenuItemsTests and
// MenuSeparatorRulesTests, which share that file).
use std::collections::HashSet;

use bb_core::commands::CommandCatalog;
use bb_core::shellmenu::{
    BuiltInMenuItems, BuiltInMenuPlaces, MenuSeparatorRules, MenuSlot, ShellMenuRules,
};

fn strings(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// --- BuiltInMenuItemsTests ---

#[test]
fn ids_are_unique_and_lowercase() {
    let ids: Vec<&str> = BuiltInMenuItems::all().iter().map(|i| i.id).collect();

    let distinct: HashSet<String> = ids.iter().map(|i| i.to_lowercase()).collect();
    assert_eq!(ids.len(), distinct.len());
    for id in ids {
        assert_eq!(id.to_lowercase(), id);
    }
}

#[test]
fn every_row_is_in_at_least_one_menu() {
    for i in BuiltInMenuItems::all() {
        assert!(
            i.is_in(BuiltInMenuPlaces::FileList) || i.is_in(BuiltInMenuPlaces::FolderTree),
            "{}",
            i.id
        );
    }
}

#[test]
fn icons_are_named_resources_and_found_by_id() {
    for i in BuiltInMenuItems::all() {
        if let Some(icon) = i.icon {
            assert!(icon.starts_with("Icon."), "{}", i.id);
        }
    }
    assert_eq!(
        Some("edit.cut"),
        BuiltInMenuItems::find("CUT").and_then(|i| i.command_id)
    );
    assert!(BuiltInMenuItems::find("no-such-item").is_none());
}

#[test]
fn every_command_a_row_names_is_in_the_catalogue() {
    // The menu prints the shortcut of the command a row names; a row naming one that does not
    // exist would print nothing, for ever, and nobody would be told.
    for i in BuiltInMenuItems::all() {
        if let Some(command_id) = i.command_id {
            assert!(
                CommandCatalog::is_known(command_id),
                "{} names {command_id}",
                i.id
            );
        }
    }
}

#[test]
fn group_rows_name_no_single_command() {
    for id in ["new", "open-new-pane", "extract"] {
        assert_eq!(None, BuiltInMenuItems::find(id).unwrap().command_id, "{id}");
    }
}

#[test]
fn shown_unless_hidden() {
    let hidden = strings(&["copy-path"]);

    assert!(!BuiltInMenuItems::is_shown("copy-path", &hidden));
    assert!(!BuiltInMenuItems::is_shown("Copy-Path", &hidden));
    assert!(BuiltInMenuItems::is_shown("open", &hidden));
}

#[test]
#[should_panic(expected = "'no-such-item' is not one of the app's menu entries.")]
fn an_unknown_id_is_a_bug_not_a_preference() {
    BuiltInMenuItems::is_shown("no-such-item", &HashSet::new());
}

#[test]
fn hidden_list_survives_a_save_through_the_shared_rule() {
    // The same rule the shell extensions use: an id this build does not know (an older or newer
    // version's) stays hidden rather than being dropped on the first save.
    let rows: Vec<(&str, bool)> = BuiltInMenuItems::all()
        .iter()
        .map(|i| (i.id, i.id != "cut"))
        .collect();
    let hidden = ShellMenuRules::hidden_after_save(["from-another-version", "open"], &rows);

    assert_eq!(vec!["from-another-version", "cut"], hidden);
}

#[test]
fn places_are_worded() {
    for (places, expected) in [
        (BuiltInMenuPlaces::FileList, "file list"),
        (BuiltInMenuPlaces::FolderTree, "folder tree"),
        (BuiltInMenuPlaces::Both, "file list and tree"),
    ] {
        assert_eq!(
            expected,
            BuiltInMenuItems::places_text(places),
            "{places:?}"
        );
    }
}

// --- MenuSeparatorRulesTests ---

fn item() -> MenuSlot {
    MenuSlot::item(true)
}

fn hidden() -> MenuSlot {
    MenuSlot::item(false)
}

fn sep() -> MenuSlot {
    MenuSlot::separator()
}

#[test]
fn separators_between_visible_groups_stay() {
    let result = MenuSeparatorRules::apply(&[item(), sep(), item(), sep(), item()]);

    assert_eq!(vec![true, true, true, true, true], result);
}

#[test]
fn a_hidden_group_does_not_leave_two_separators_touching() {
    let result = MenuSeparatorRules::apply(&[item(), sep(), hidden(), hidden(), sep(), item()]);

    assert_eq!(vec![true, true, false, false, false, true], result);
}

#[test]
fn no_separator_at_either_edge() {
    let result =
        MenuSeparatorRules::apply(&[sep(), hidden(), sep(), item(), sep(), hidden(), sep()]);

    assert_eq!(vec![false, false, false, true, false, false, false], result);
}

#[test]
fn everything_hidden_shows_nothing() {
    let result = MenuSeparatorRules::apply(&[hidden(), sep(), hidden()]);

    assert!(result.iter().all(|v| !v));
}

#[test]
fn adjacent_separators_already_in_the_markup_collapse_to_one() {
    // The custom-command and shell-extension anchors sit right beside a plain separator.
    let result = MenuSeparatorRules::apply(&[item(), sep(), sep(), sep(), item()]);

    assert_eq!(vec![true, true, false, false, true], result);
}
