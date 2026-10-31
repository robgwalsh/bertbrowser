// Port of tests/BertBrowser.Core.Tests/Theming/SystemThemeRulesTests.cs.
use bb_core::theming::{
    SystemAppearance, SystemThemeRules, ThemeCatalog, ThemeDefinition, ThemeIssueSeverity,
    ids_equal,
};

/// A theme of the user's own, so "the lookup can see it but the catalogue cannot" is exercised;
/// that is the difference that matters for a theme the user deleted.
fn user_theme(id: &str, dark: bool) -> ThemeDefinition {
    ThemeDefinition {
        id: id.into(),
        name: id.into(),
        base_theme_id: Some(if dark {
            ThemeCatalog::dark_plus().id.clone()
        } else {
            ThemeCatalog::light_plus().id.clone()
        }),
        is_dark: dark,
        ..Default::default()
    }
}

/// The built-ins plus whatever extra themes a case needs. Anything else is absent, the way a
/// deleted user theme is.
fn lookup<'a>(extra: &'a [ThemeDefinition]) -> impl Fn(&str) -> Option<&'a ThemeDefinition> + 'a {
    move |id: &str| {
        extra
            .iter()
            .find(|t| ids_equal(&t.id, id))
            .or_else(|| ThemeCatalog::find(id))
    }
}

fn ids(choices: &[&ThemeDefinition]) -> Vec<String> {
    choices.iter().map(|t| t.id.clone()).collect()
}

// --- who follows, on which launch ---

/// A brand-new install matches Windows.
#[test]
fn a_fresh_install_follows() {
    assert!(SystemThemeRules::should_follow(None, false));
}

/// And an existing user does not, including the ones who never opened Settings and so have no
/// theme id either. Keying off the theme id instead would have flipped every one of them on
/// upgrade, which is the whole reason this takes the file rather than the id.
#[test]
fn an_existing_settings_file_does_not_start_following() {
    assert!(!SystemThemeRules::should_follow(None, true));
}

#[test]
fn an_explicit_choice_beats_both() {
    for (configured, has_file) in [(true, true), (true, false), (false, true), (false, false)] {
        assert_eq!(
            configured,
            SystemThemeRules::should_follow(Some(configured), has_file),
            "{configured} {has_file}"
        );
    }
}

// --- high contrast wins outright ---

#[test]
fn high_contrast_wins_with_no_slots_set() {
    let choice = SystemThemeRules::decide(SystemAppearance::HIGH_CONTRAST, None, None, lookup(&[]));

    assert_eq!(ThemeCatalog::high_contrast_dark().id, choice.theme_id);
    assert_eq!(None, choice.issue);
}

#[test]
fn high_contrast_wins_over_both_configured_slots() {
    let choice = SystemThemeRules::decide(
        SystemAppearance::HIGH_CONTRAST,
        Some("solarized-light"),
        Some("nord"),
        lookup(&[]),
    );

    assert_eq!(ThemeCatalog::high_contrast_dark().id, choice.theme_id);
    assert_eq!(None, choice.issue);
}

/// The row that catches anyone "simplifying" the early return away: an accessibility setting must
/// not produce a warning about a slot it never consulted.
#[test]
fn high_contrast_ignores_a_broken_slot_and_reports_nothing() {
    let choice = SystemThemeRules::decide(
        SystemAppearance::HIGH_CONTRAST,
        None,
        Some("deleted-theme"),
        lookup(&[]),
    );

    assert_eq!(ThemeCatalog::high_contrast_dark().id, choice.theme_id);
    assert_eq!(None, choice.issue);
}

// --- the defaults ---

#[test]
fn dark_with_no_slot_is_dark_plus() {
    let choice = SystemThemeRules::decide(SystemAppearance::DARK, None, None, lookup(&[]));

    assert_eq!(ThemeCatalog::dark_plus().id, choice.theme_id);
    assert_eq!(None, choice.issue);
}

#[test]
fn light_with_no_slot_is_light_plus() {
    let choice = SystemThemeRules::decide(SystemAppearance::LIGHT, None, None, lookup(&[]));

    assert_eq!(ThemeCatalog::light_plus().id, choice.theme_id);
    assert_eq!(None, choice.issue);
}

#[test]
fn a_blank_slot_is_the_same_as_no_slot() {
    for slot in ["", "   "] {
        let choice =
            SystemThemeRules::decide(SystemAppearance::DARK, None, Some(slot), lookup(&[]));

        assert_eq!(ThemeCatalog::dark_plus().id, choice.theme_id, "{slot:?}");
        assert_eq!(None, choice.issue, "{slot:?}");
    }
}

// --- the slots ---

#[test]
fn dark_uses_the_dark_slot() {
    let choice = SystemThemeRules::decide(SystemAppearance::DARK, None, Some("nord"), lookup(&[]));

    assert_eq!("nord", choice.theme_id);
    assert_eq!(None, choice.issue);
}

#[test]
fn light_uses_the_light_slot() {
    let choice = SystemThemeRules::decide(
        SystemAppearance::LIGHT,
        Some("solarized-light"),
        None,
        lookup(&[]),
    );

    assert_eq!("solarized-light", choice.theme_id);
    assert_eq!(None, choice.issue);
}

#[test]
fn a_user_theme_the_catalogue_does_not_know_is_still_usable() {
    let mine = [user_theme("midnight", true)];

    let choice = SystemThemeRules::decide(
        SystemAppearance::DARK,
        None,
        Some("midnight"),
        lookup(&mine),
    );

    assert_eq!("midnight", choice.theme_id);
    assert_eq!(None, choice.issue);
}

#[test]
fn slot_ids_are_matched_case_insensitively() {
    let choice = SystemThemeRules::decide(SystemAppearance::DARK, None, Some("NoRd"), lookup(&[]));

    assert_eq!("nord", choice.theme_id);
    assert_eq!(None, choice.issue);
}

/// The inactive slot is never consulted, so a broken one stays silent until you switch, which is
/// the only moment it matters.
#[test]
fn the_inactive_slot_is_not_consulted() {
    let choice = SystemThemeRules::decide(
        SystemAppearance::DARK,
        Some("no-such-theme"),
        None,
        lookup(&[]),
    );

    assert_eq!(ThemeCatalog::dark_plus().id, choice.theme_id);
    assert_eq!(None, choice.issue);
}

// --- a slot that has gone missing ---

#[test]
fn a_missing_dark_slot_falls_back_and_says_so() {
    let choice = SystemThemeRules::decide(
        SystemAppearance::DARK,
        None,
        Some("deleted-theme"),
        lookup(&[]),
    );

    assert_eq!(ThemeCatalog::dark_plus().id, choice.theme_id);
    let issue = choice.issue.expect("an issue");
    assert_eq!(ThemeIssueSeverity::Error, issue.severity);
    assert!(issue.message.contains("deleted-theme"), "{}", issue.message);
}

#[test]
fn a_missing_light_slot_falls_back_and_says_so() {
    let choice = SystemThemeRules::decide(
        SystemAppearance::LIGHT,
        Some("deleted-theme"),
        None,
        lookup(&[]),
    );

    assert_eq!(ThemeCatalog::light_plus().id, choice.theme_id);
    assert_eq!(
        Some(ThemeIssueSeverity::Error),
        choice.issue.map(|i| i.severity)
    );
}

// --- a slot holding the wrong kind of theme ---

/// Honoured rather than corrected: `is_dark` is metadata and the choice is the user's. Only a
/// hand-edited settings file reaches here, and repairing it would be the surprise.
#[test]
fn a_light_theme_in_the_dark_slot_is_honoured_with_a_warning() {
    let choice = SystemThemeRules::decide(
        SystemAppearance::DARK,
        None,
        Some("solarized-light"),
        lookup(&[]),
    );

    assert_eq!("solarized-light", choice.theme_id);
    assert_eq!(
        Some(ThemeIssueSeverity::Warning),
        choice.issue.map(|i| i.severity)
    );
}

#[test]
fn a_dark_theme_in_the_light_slot_is_honoured_with_a_warning() {
    let choice = SystemThemeRules::decide(SystemAppearance::LIGHT, Some("nord"), None, lookup(&[]));

    assert_eq!("nord", choice.theme_id);
    assert_eq!(
        Some(ThemeIssueSeverity::Warning),
        choice.issue.map(|i| i.severity)
    );
}

// --- totality ---

/// Like the resolver, this never fails and never produces an unusable answer.
#[test]
fn every_combination_yields_a_theme_the_lookup_can_resolve() {
    let slots = [
        None,
        Some(""),
        Some("nord"),
        Some("solarized-light"),
        Some("deleted-theme"),
        Some("midnight"),
    ];
    let mine = [user_theme("midnight", true)];
    let lookup = lookup(&mine);

    for appearance in [
        SystemAppearance::LIGHT,
        SystemAppearance::DARK,
        SystemAppearance::HIGH_CONTRAST,
    ] {
        for light in slots {
            for dark in slots {
                let choice = SystemThemeRules::decide(appearance, light, dark, &lookup);

                assert!(
                    !choice.theme_id.trim().is_empty(),
                    "{appearance:?} {light:?} {dark:?}"
                );
                assert!(
                    lookup(&choice.theme_id).is_some(),
                    "{appearance:?} {light:?} {dark:?}"
                );
            }
        }
    }
}

#[test]
fn the_three_answers_it_can_invent_are_real_built_ins() {
    assert!(ThemeCatalog::find(SystemThemeRules::default_light_theme_id()).is_some());
    assert!(ThemeCatalog::find(SystemThemeRules::default_dark_theme_id()).is_some());
    assert!(ThemeCatalog::find(SystemThemeRules::high_contrast_theme_id()).is_some());

    assert!(
        !ThemeCatalog::find(SystemThemeRules::default_light_theme_id())
            .unwrap()
            .is_dark
    );
    assert!(
        ThemeCatalog::find(SystemThemeRules::default_dark_theme_id())
            .unwrap()
            .is_dark
    );
}

// --- what the two pickers offer ---

#[test]
fn the_dark_list_is_every_dark_built_in_except_the_accessibility_one() {
    let choices = SystemThemeRules::slot_choices(ThemeCatalog::built_ins(), true, None);

    assert!(choices.iter().all(|t| t.is_dark));
    assert!(
        !choices
            .iter()
            .any(|t| t.id == ThemeCatalog::high_contrast_dark().id)
    );
    assert!(choices.iter().any(|t| t.id == ThemeCatalog::dark_plus().id));
    assert_eq!(
        ThemeCatalog::built_ins()
            .iter()
            .filter(|t| t.is_dark)
            .count()
            - 1,
        choices.len()
    );
}

#[test]
fn the_light_list_is_every_light_built_in() {
    let choices = SystemThemeRules::slot_choices(ThemeCatalog::built_ins(), false, None);

    assert!(choices.iter().all(|t| !t.is_dark));
    assert!(
        choices
            .iter()
            .any(|t| t.id == ThemeCatalog::light_plus().id)
    );
    assert_eq!(
        ThemeCatalog::built_ins()
            .iter()
            .filter(|t| !t.is_dark)
            .count(),
        choices.len()
    );
}

#[test]
fn a_user_theme_appears_in_the_list_for_its_own_side() {
    let mut available = ThemeCatalog::built_ins().to_vec();
    available.push(user_theme("midnight", true));

    assert!(
        SystemThemeRules::slot_choices(&available, true, None)
            .iter()
            .any(|t| t.id == "midnight")
    );
    assert!(
        !SystemThemeRules::slot_choices(&available, false, None)
            .iter()
            .any(|t| t.id == "midnight")
    );
}

/// Or the picker would render empty and the only way to see what was set would be the settings
/// file.
#[test]
fn an_assigned_theme_that_fails_the_filter_is_kept_in_the_list_once() {
    let choices =
        SystemThemeRules::slot_choices(ThemeCatalog::built_ins(), true, Some("solarized-light"));

    assert_eq!(
        1,
        choices.iter().filter(|t| t.id == "solarized-light").count()
    );
    assert_eq!("solarized-light", choices[0].id);
}

#[test]
fn an_assigned_theme_already_in_the_list_is_not_duplicated() {
    let choices = SystemThemeRules::slot_choices(ThemeCatalog::built_ins(), true, Some("nord"));

    assert_eq!(1, choices.iter().filter(|t| t.id == "nord").count());
}

#[test]
fn an_assigned_theme_that_does_not_exist_changes_nothing() {
    let plain = SystemThemeRules::slot_choices(ThemeCatalog::built_ins(), true, None);
    let with_ghost =
        SystemThemeRules::slot_choices(ThemeCatalog::built_ins(), true, Some("deleted-theme"));

    assert_eq!(ids(&plain), ids(&with_ghost));
}
