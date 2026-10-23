// Port of tests/BertBrowser.Core.Tests/MenuLayoutRulesTests.cs.
use std::collections::HashSet;

use bb_core::shellmenu::{BuiltInMenuItems, MenuLayoutRules, MenuPlacement, MenuPlacementKind};

fn all_built_ins() -> HashSet<&'static str> {
    BuiltInMenuItems::all().iter().map(|i| i.id).collect()
}

fn all_ids() -> impl Iterator<Item = &'static str> {
    BuiltInMenuItems::all().iter().map(|i| i.id)
}

fn show(placements: &[MenuPlacement]) -> String {
    placements
        .iter()
        .map(|p| match p.kind {
            MenuPlacementKind::Separator => "|".to_string(),
            MenuPlacementKind::Command => format!("cmd:{}", p.id),
            _ => p.id.clone(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

const NONE: &[&str] = &[];

#[test]
fn the_default_places_every_built_in_once() {
    let placed: Vec<&str> = MenuLayoutRules::default_layout()
        .iter()
        .filter_map(|t| MenuLayoutRules::is_built_in(t))
        .collect();

    assert_eq!(all_ids().collect::<Vec<_>>(), placed);
    assert_eq!(
        1,
        MenuLayoutRules::default_layout()
            .iter()
            .filter(|t| **t == MenuLayoutRules::MORE)
            .count()
    );
}

#[test]
fn no_layout_means_the_default() {
    assert_eq!(
        MenuLayoutRules::default_layout(),
        MenuLayoutRules::normalize(None::<&[&str]>, NONE)
    );
}

#[test]
fn a_built_in_nobody_placed_goes_back_beside_its_default_neighbour() {
    // As a later build adding "copy-name" would look to a layout saved before it existed.
    let layout = MenuLayoutRules::normalize(
        Some(&[
            "app:properties",
            "-",
            "app:copy-path",
            "app:cut",
            MenuLayoutRules::MORE,
        ]),
        all_ids().filter(|id| !matches!(*id, "copy-name" | "copy-path" | "cut" | "properties")),
    );

    assert_eq!(
        vec![
            "app:properties",
            "-",
            "app:copy-path",
            "app:copy-name",
            "app:cut",
            MenuLayoutRules::MORE
        ],
        layout
    );
}

#[test]
fn a_hidden_built_in_is_not_put_back() {
    let hidden: Vec<&str> = all_ids().filter(|id| *id != "open").collect();

    let layout = MenuLayoutRules::normalize(Some(&[MenuLayoutRules::MORE]), hidden);

    assert_eq!(vec!["app:open", MenuLayoutRules::MORE], layout);
}

#[test]
fn a_missing_catch_all_is_restored_and_a_repeat_dropped() {
    let hidden = all_ids().filter(|id| !matches!(*id, "bookmark" | "properties"));

    let layout = MenuLayoutRules::normalize(
        Some(&["app:bookmark", "-", "app:BOOKMARK", "-", "app:properties"]),
        hidden,
    );

    assert_eq!(
        vec![
            "app:bookmark",
            MenuLayoutRules::MORE,
            "-",
            "-",
            "app:properties"
        ],
        layout
    );
}

#[test]
fn a_token_this_build_does_not_know_is_kept() {
    let layout = MenuLayoutRules::normalize(
        Some(&[
            "app:from-a-later-build",
            "clsid:{abc}",
            MenuLayoutRules::MORE,
        ]),
        all_ids(),
    );

    assert_eq!(
        vec![
            "app:from-a-later-build",
            "clsid:{abc}",
            MenuLayoutRules::MORE
        ],
        layout
    );
}

#[test]
fn the_default_arrangement_is_the_menu_as_it_was() {
    let arranged = MenuLayoutRules::arrange(
        MenuLayoutRules::default_layout(),
        all_built_ins(),
        &["c1"],
        &["verb:git_shell", "clsid:{7z}"],
    );

    let shown = show(&arranged);
    assert!(
        shown.ends_with(
            "rename delete delete-permanently bookmark | cmd:c1 | verb:git_shell clsid:{7z} | properties"
        ),
        "{shown}"
    );
}

#[test]
fn the_tree_skips_what_it_does_not_have() {
    let tree = ["new", "open-terminal", "copy-path", "properties"];

    let arranged = MenuLayoutRules::arrange(MenuLayoutRules::default_layout(), tree, NONE, NONE);

    assert_eq!(
        "new | open-terminal | copy-path | | | properties",
        show(&arranged)
    );
}

#[test]
fn a_placed_extension_appears_at_its_spot_and_not_again_at_the_catch_all() {
    let layout = MenuLayoutRules::normalize(
        Some(&[
            "app:compress",
            "clsid:{7z}",
            "-",
            "app:cut",
            MenuLayoutRules::MORE,
            "cmd:c2",
        ]),
        all_ids().filter(|id| !matches!(*id, "compress" | "cut")),
    );

    let arranged = MenuLayoutRules::arrange(
        &layout,
        all_built_ins(),
        &["c1", "c2"],
        &["verb:git_shell", "clsid:{7z}"],
    );

    assert_eq!(
        "compress clsid:{7z} | cut | cmd:c1 | verb:git_shell cmd:c2",
        show(&arranged)
    );
}

#[test]
fn what_is_not_offered_here_is_never_emitted() {
    // A command not applicable to this selection, an extension with nothing for it, and a
    // built-in this menu does not declare.
    let arranged = MenuLayoutRules::arrange(
        &["app:cut", "cmd:c1", "clsid:{7z}", MenuLayoutRules::MORE],
        NONE,
        NONE,
        NONE,
    );

    assert!(arranged.is_empty(), "{}", show(&arranged));
}
