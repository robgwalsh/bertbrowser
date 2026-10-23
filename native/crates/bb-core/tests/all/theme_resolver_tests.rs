// Port of tests/BertBrowser.Core.Tests/Theming/ThemeResolverTests.cs.
use bb_core::theming::{
    ColorMap, ThemeCatalog, ThemeColor, ThemeDefinition, ThemeIssueSeverity, ThemeResolver,
    ThemeToken, ids_equal, no_lookup,
};

fn theme(id: &str, base_id: Option<&str>, colors: &[(&str, &str)]) -> ThemeDefinition {
    ThemeDefinition {
        id: id.into(),
        name: id.into(),
        base_theme_id: base_id.map(str::to_string),
        colors: colors.iter().copied().collect(),
        ..Default::default()
    }
}

fn lookup<'a>(themes: &'a [ThemeDefinition]) -> impl Fn(&str) -> Option<&'a ThemeDefinition> + 'a {
    move |id: &str| {
        themes
            .iter()
            .find(|t| ids_equal(&t.id, id))
            .or_else(|| ThemeCatalog::find(id))
    }
}

#[test]
fn a_sparse_theme_inherits_everything_it_does_not_set() {
    let mine = theme(
        "mine",
        Some("dark-plus"),
        &[(ThemeToken::AccentBackground.name(), "#C586C0")],
    );

    let resolved = ThemeResolver::resolve(&mine, ThemeCatalog::find);

    assert_eq!(
        ThemeColor::parse("#C586C0"),
        resolved[ThemeToken::AccentBackground]
    );
    assert_eq!(
        ThemeColor::parse("#1E1E1E"),
        resolved[ThemeToken::WindowBackground]
    );
    assert!(resolved.issues.is_empty(), "{:?}", resolved.issues);
}

#[test]
fn resolution_is_always_complete() {
    let resolved = ThemeResolver::resolve(&theme("empty", None, &[]), ThemeCatalog::find);

    for token in ThemeToken::ALL {
        assert!(
            resolved.get(token.name()).is_some(),
            "missing {}",
            token.name()
        );
    }
    assert_eq!(ThemeToken::COUNT, resolved.colors().count());
}

#[test]
fn a_theme_beats_its_base_and_a_base_beats_its_own_base() {
    let grandparent = theme(
        "grandparent",
        None,
        &[
            (ThemeToken::TextPrimary.name(), "#111111"),
            (ThemeToken::TextMuted.name(), "#999999"),
        ],
    );
    let parent = theme(
        "parent",
        Some("grandparent"),
        &[(ThemeToken::TextPrimary.name(), "#222222")],
    );
    let child = theme(
        "child",
        Some("parent"),
        &[(ThemeToken::TextPrimary.name(), "#333333")],
    );
    let all = [grandparent, parent, child.clone()];

    let resolved = ThemeResolver::resolve(&child, lookup(&all));

    assert_eq!(
        ThemeColor::parse("#333333"),
        resolved[ThemeToken::TextPrimary]
    );
    assert_eq!(
        ThemeColor::parse("#999999"),
        resolved[ThemeToken::TextMuted]
    );
}

#[test]
fn user_overrides_beat_the_theme() {
    let mine = theme(
        "mine",
        Some("dark-plus"),
        &[(ThemeToken::AccentBackground.name(), "#C586C0")],
    );
    let overrides = ColorMap::from([(ThemeToken::AccentBackground.name(), "#4EC9B0")]);

    let resolved = ThemeResolver::resolve_with(&mine, ThemeCatalog::find, Some(&overrides));

    assert_eq!(
        ThemeColor::parse("#4EC9B0"),
        resolved[ThemeToken::AccentBackground]
    );
}

#[test]
fn a_missing_base_falls_back_to_the_default_and_says_so() {
    let orphan = theme(
        "orphan",
        Some("no-such-theme"),
        &[(ThemeToken::TextPrimary.name(), "#123456")],
    );

    let resolved = ThemeResolver::resolve(&orphan, ThemeCatalog::find);

    assert_eq!(
        ThemeColor::parse("#123456"),
        resolved[ThemeToken::TextPrimary]
    );
    assert_eq!(
        ThemeColor::parse("#1E1E1E"),
        resolved[ThemeToken::WindowBackground]
    );
    assert!(
        resolved
            .issues
            .iter()
            .any(|i| i.message.contains("no-such-theme")),
        "{:?}",
        resolved.issues
    );
}

#[test]
fn a_base_loop_terminates_and_is_reported() {
    let a = theme(
        "a",
        Some("b"),
        &[(ThemeToken::TextPrimary.name(), "#AAAAAA")],
    );
    let b = theme("b", Some("a"), &[(ThemeToken::TextMuted.name(), "#BBBBBB")]);
    let all = [a.clone(), b];

    let resolved = ThemeResolver::resolve(&a, lookup(&all));

    assert_eq!(
        ThemeColor::parse("#AAAAAA"),
        resolved[ThemeToken::TextPrimary]
    );
    assert_eq!(
        ThemeColor::parse("#BBBBBB"),
        resolved[ThemeToken::TextMuted]
    );
    assert!(
        resolved
            .issues
            .iter()
            .any(|i| i.severity == ThemeIssueSeverity::Error),
        "{:?}",
        resolved.issues
    );
}

#[test]
fn a_theme_that_bases_on_itself_terminates() {
    let this = theme(
        "self",
        Some("self"),
        &[(ThemeToken::TextPrimary.name(), "#ABCDEF")],
    );
    let all = [this.clone()];

    let resolved = ThemeResolver::resolve(&this, lookup(&all));

    assert_eq!(
        ThemeColor::parse("#ABCDEF"),
        resolved[ThemeToken::TextPrimary]
    );
    assert!(
        resolved
            .issues
            .iter()
            .any(|i| i.severity == ThemeIssueSeverity::Error),
        "{:?}",
        resolved.issues
    );
}

#[test]
fn an_unknown_token_is_reported_and_ignored() {
    let mine = theme(
        "mine",
        Some("dark-plus"),
        &[("Theme.Not.AThing", "#FF0000")],
    );

    let resolved = ThemeResolver::resolve(&mine, ThemeCatalog::find);

    assert_eq!(1, resolved.issues.len(), "{:?}", resolved.issues);
    let issue = &resolved.issues[0];
    assert_eq!(Some("Theme.Not.AThing"), issue.token.as_deref());
    assert_eq!(ThemeIssueSeverity::Warning, issue.severity);
}

#[test]
fn an_unparseable_colour_keeps_the_inherited_value_and_is_reported() {
    let mine = theme(
        "mine",
        Some("dark-plus"),
        &[(ThemeToken::WindowBackground.name(), "octarine")],
    );

    let resolved = ThemeResolver::resolve(&mine, ThemeCatalog::find);

    assert_eq!(
        ThemeColor::parse("#1E1E1E"),
        resolved[ThemeToken::WindowBackground]
    );
    assert!(
        resolved
            .issues
            .iter()
            .any(|i| i.token.as_deref() == Some(ThemeToken::WindowBackground.name())),
        "{:?}",
        resolved.issues
    );
}

#[test]
fn identity_comes_from_the_theme_not_its_base() {
    let mine = ThemeDefinition {
        id: "mine".into(),
        name: "Mine".into(),
        base_theme_id: Some("light-plus".into()),
        is_dark: false,
        ..Default::default()
    };

    let resolved = ThemeResolver::resolve(&mine, ThemeCatalog::find);

    assert_eq!("mine", resolved.id);
    assert_eq!("Mine", resolved.name);
    assert!(!resolved.is_dark);
    assert_eq!(
        ThemeColor::parse("#FFFFFF"),
        resolved[ThemeToken::WindowBackground]
    );
}

#[test]
fn no_lookup_at_all_still_resolves_against_the_default() {
    let mine = theme(
        "mine",
        Some("dark-plus"),
        &[(ThemeToken::TextPrimary.name(), "#010203")],
    );

    let resolved = ThemeResolver::resolve(&mine, no_lookup);

    assert_eq!(
        ThemeColor::parse("#010203"),
        resolved[ThemeToken::TextPrimary]
    );
    assert_eq!(
        ThemeColor::parse("#1E1E1E"),
        resolved[ThemeToken::WindowBackground]
    );
}

#[test]
fn pruning_drops_unknown_keys_bad_colours_and_no_op_overrides() {
    let dark = ThemeResolver::resolve(ThemeCatalog::dark_plus(), ThemeCatalog::find);
    let overrides = ColorMap::from([
        ("Theme.Not.AThing", "#FF0000"),
        (ThemeToken::TextPrimary.name(), "not a colour"),
        // Same as Dark+, just cased differently.
        (ThemeToken::WindowBackground.name(), "#1e1e1e"),
        (ThemeToken::AccentBackground.name(), "#C586C0"),
    ]);

    let pruned = ThemeResolver::prune_overrides(&overrides, &dark);

    assert_eq!(
        vec![ThemeToken::AccentBackground.name()],
        pruned.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        Some("#C586C0"),
        pruned.get(ThemeToken::AccentBackground.name())
    );
}
