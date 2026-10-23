// Port of tests/BertBrowser.Core.Tests/Theming/ThemeTokenTests.cs.
//
// The C# reflected over the string constants to check each had a descriptor; here the enum and
// the descriptor list are one macro, so the checks are that `name`/`from_name` and the descriptor
// table agree with `ALL` entry for entry.
use std::collections::HashSet;

use bb_core::theming::ThemeToken;

#[test]
fn every_declared_constant_has_a_descriptor() {
    let missing: Vec<&str> = ThemeToken::ALL
        .iter()
        .map(|t| t.name())
        .filter(|key| !ThemeToken::is_known(key))
        .collect();
    assert!(missing.is_empty(), "{missing:?}");
    for (i, token) in ThemeToken::ALL.iter().enumerate() {
        assert_eq!(*token, ThemeToken::DESCRIPTORS[i].token, "{}", token.name());
        assert_eq!(
            token.name(),
            ThemeToken::DESCRIPTORS[i].key,
            "{}",
            token.name()
        );
        assert_eq!(i, token.index(), "{}", token.name());
    }
}

#[test]
fn every_descriptor_corresponds_to_a_declared_constant() {
    let orphans: Vec<&str> = ThemeToken::DESCRIPTORS
        .iter()
        .filter(|d| ThemeToken::from_name(d.key) != Some(d.token))
        .map(|d| d.key)
        .collect();
    assert!(orphans.is_empty(), "{orphans:?}");
    assert_eq!(ThemeToken::COUNT, ThemeToken::ALL.len());
}

#[test]
fn token_keys_are_unique() {
    let mut seen = HashSet::new();
    let duplicates: Vec<&str> = ThemeToken::ALL
        .iter()
        .map(|t| t.name())
        .filter(|key| !seen.insert(*key))
        .collect();
    assert!(duplicates.is_empty(), "{duplicates:?}");
}

#[test]
fn every_descriptor_is_presentable() {
    for descriptor in &ThemeToken::DESCRIPTORS {
        assert!(
            !descriptor.group.trim().is_empty(),
            "{} has no group",
            descriptor.key
        );
        assert!(
            !descriptor.display_name.trim().is_empty(),
            "{} has no display name",
            descriptor.key
        );
    }
}

/// Keys double as resource keys, so they must look like the ones the UI uses.
#[test]
fn token_keys_are_dotted_theme_paths() {
    for token in ThemeToken::ALL {
        let key = token.name();
        assert!(key.starts_with("Theme."), "{key}");
        assert_eq!(3, key.split('.').count(), "{key}");
    }
}

#[test]
fn the_editors_default_list_is_a_useful_size() {
    let core = ThemeToken::DESCRIPTORS.iter().filter(|d| d.is_core).count();
    assert!((8..=24).contains(&core), "{core}");
}

#[test]
fn groups_are_contiguous_so_the_editor_can_list_them_in_order() {
    let mut seen = HashSet::new();
    let mut previous: Option<&str> = None;

    for descriptor in &ThemeToken::DESCRIPTORS {
        if Some(descriptor.group) == previous {
            continue;
        }
        assert!(
            seen.insert(descriptor.group),
            "group '{}' is split across the list",
            descriptor.group
        );
        previous = Some(descriptor.group);
    }
}
