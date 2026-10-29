// Port of tests/BertBrowser.Core.Tests/CommandCatalogTests.cs.
use std::collections::{HashMap, HashSet};

use bb_core::commands::{CommandCatalog, CommandCategories, KeyChord, KeymapRules};

#[test]
fn ids_are_unique_dotted_and_lowercase() {
    let ids: Vec<&str> = CommandCatalog::all().iter().map(|c| c.id).collect();
    let distinct: HashSet<String> = ids.iter().map(|i| i.to_lowercase()).collect();
    assert_eq!(ids.len(), distinct.len());
    for id in ids {
        // ^[a-z]+\.[a-z0-9-]+$
        let (head, tail) = id.split_once('.').unwrap_or_else(|| panic!("{id}"));
        assert!(
            !head.is_empty() && head.bytes().all(|b| b.is_ascii_lowercase()),
            "{id}"
        );
        assert!(
            !tail.is_empty()
                && tail
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "{id}"
        );
    }
}

#[test]
fn every_command_has_a_name_and_a_known_category() {
    for c in CommandCatalog::all() {
        assert!(!c.name.trim().is_empty(), "{}", c.id);
        assert!(CommandCategories::ALL.contains(&c.category), "{}", c.id);
    }
}

#[test]
fn names_are_unique_within_a_category() {
    // Two rows reading the same in the palette cannot be told apart by the person choosing.
    let mut seen: HashMap<(&str, &str), usize> = HashMap::new();
    for c in CommandCatalog::all() {
        *seen.entry((c.category, c.name)).or_insert(0) += 1;
    }
    let repeats: Vec<_> = seen
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(k, _)| k.1)
        .collect();
    assert!(repeats.is_empty(), "{repeats:?}");
}

#[test]
fn icons_are_named_resources() {
    for c in CommandCatalog::all() {
        if let Some(icon) = c.icon {
            assert!(icon.starts_with("Icon."), "{}", c.id);
        }
    }
}

#[test]
fn no_two_commands_ship_with_the_same_shortcut() {
    let mut owners: HashMap<KeyChord, Vec<&str>> = HashMap::new();
    for c in CommandCatalog::all() {
        for chord in &c.default_chords {
            owners.entry(*chord).or_default().push(c.id);
        }
    }
    let shared: Vec<String> = owners
        .iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|(chord, ids)| format!("{chord}: {}", ids.join(", ")))
        .collect();
    assert!(shared.is_empty(), "{shared:?}");
}

#[test]
fn no_shipped_shortcut_is_one_the_rules_would_refuse() {
    let refused: Vec<String> = CommandCatalog::all()
        .iter()
        .flat_map(|c| {
            c.default_chords.iter().filter_map(move |chord| {
                KeymapRules::refuse(chord, c.context).map(|why| format!("{} {chord}: {why}", c.id))
            })
        })
        .collect();
    assert!(refused.is_empty(), "{refused:?}");
}

#[test]
fn every_shipped_shortcut_is_in_force_with_nothing_overridden() {
    let keymap = KeymapRules::resolve(&KeymapRules::catalog_bindables(), None);
    for c in CommandCatalog::all() {
        assert_eq!(
            c.default_chords.as_slice(),
            keymap.chords_for(c.id),
            "{}",
            c.id
        );
    }
}

#[test]
fn found_by_id_exactly() {
    assert_eq!(
        Some("New tab"),
        CommandCatalog::find("tab.new").map(|c| c.name)
    );
    assert!(CommandCatalog::find("TAB.NEW").is_none());
    assert!(!CommandCatalog::is_known("no.such"));
}
