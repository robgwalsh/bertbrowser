//! Which theme "match Windows" means right now. Pure, so every row of the decision is a test
//! rather than something to notice in a screenshot; the window-coupled theme service has no tests,
//! so no branch of this may live there.
//! Spec: src/BertBrowser.Core/Theming/SystemThemeRules.cs, SystemAppearance.cs.

use super::{ThemeCatalog, ThemeDefinition, ThemeIssue, ThemeIssueSeverity, ids_equal};

/// What Windows is currently set to, as far as theming cares. Two independent switches rather
/// than three states, deliberately: high contrast decides which theme is used, but the light/dark
/// bit is still read and kept underneath it. That is what lets turning high contrast *off* land
/// back on the right slot with nothing to re-read and no restart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SystemAppearance {
    pub is_dark: bool,
    pub is_high_contrast: bool,
}

impl SystemAppearance {
    pub const LIGHT: SystemAppearance = SystemAppearance {
        is_dark: false,
        is_high_contrast: false,
    };

    pub const DARK: SystemAppearance = SystemAppearance {
        is_dark: true,
        is_high_contrast: false,
    };

    /// Windows ships a light and a dark high-contrast scheme and we answer both with the one
    /// accessibility theme, so this carries a dark bit only for what happens when it goes off.
    pub const HIGH_CONTRAST: SystemAppearance = SystemAppearance {
        is_dark: true,
        is_high_contrast: true,
    };
}

/// `theme_id` is the theme to apply: always usable, never empty. `issue` is what was wrong with
/// the user's configuration, for the notice line in Settings, or `None` when nothing was.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SystemThemeChoice {
    pub theme_id: String,
    pub issue: Option<ThemeIssue>,
}

impl SystemThemeChoice {
    fn clean(theme_id: &str) -> SystemThemeChoice {
        SystemThemeChoice {
            theme_id: theme_id.to_string(),
            issue: None,
        }
    }
}

pub struct SystemThemeRules;

impl SystemThemeRules {
    pub fn default_light_theme_id() -> &'static str {
        &ThemeCatalog::light_plus().id
    }

    pub fn default_dark_theme_id() -> &'static str {
        &ThemeCatalog::dark_plus().id
    }

    /// What high contrast always resolves to. Our control templates are fully custom, so they
    /// never pick up Windows' own high-contrast colours; this is the closest equivalent.
    pub fn high_contrast_theme_id() -> &'static str {
        &ThemeCatalog::high_contrast_dark().id
    }

    /// Whether a launch should match Windows, given what the settings file said. `configured` is
    /// the stored preference, `None` when it has never been set; `has_settings_file` whether
    /// settings came from a file at all.
    ///
    /// A brand-new install matches Windows; anyone who already had a settings file keeps what they
    /// had. It has to key off the file rather than off a null theme id, because someone who has
    /// simply never opened Settings also has no theme id, and flipping all of them on upgrade would
    /// be exactly the surprise this is optional to avoid. The caller pins the answer immediately,
    /// or launch two would find a file and read the `None` as "no".
    pub fn should_follow(configured: Option<bool>, has_settings_file: bool) -> bool {
        configured.unwrap_or(!has_settings_file)
    }

    /// `lookup` resolves an id to a theme, user themes included: a slot pointing at a theme the
    /// user deleted is the case that matters, and `ThemeCatalog::find` alone would not see it.
    pub fn decide<'a>(
        appearance: SystemAppearance,
        light_theme_id: Option<&str>,
        dark_theme_id: Option<&str>,
        lookup: impl Fn(&str) -> Option<&'a ThemeDefinition>,
    ) -> SystemThemeChoice {
        // High contrast wins outright, and deliberately does not consult the slots at all: an
        // accessibility setting must not be able to land on a low-contrast theme because of
        // something configured months ago, and a broken slot must not produce a warning here.
        if appearance.is_high_contrast {
            return SystemThemeChoice::clean(SystemThemeRules::high_contrast_theme_id());
        }

        let (wanted, fallback, slot) = if appearance.is_dark {
            (
                dark_theme_id,
                SystemThemeRules::default_dark_theme_id(),
                "dark",
            )
        } else {
            (
                light_theme_id,
                SystemThemeRules::default_light_theme_id(),
                "light",
            )
        };

        let wanted = match wanted {
            Some(w) if !w.trim().is_empty() => w,
            _ => return SystemThemeChoice::clean(fallback),
        };

        let Some(definition) = lookup(wanted) else {
            // A theme can legitimately be absent for one launch (a file still syncing, a themes
            // folder on a drive that is not mounted), so fall back for now and say so. The caller
            // must not rewrite the setting.
            return SystemThemeChoice {
                theme_id: fallback.to_string(),
                issue: Some(ThemeIssue::new(
                    ThemeIssueSeverity::Error,
                    None,
                    format!(
                        "Your {slot} theme '{wanted}' was not found; using {fallback} for now."
                    ),
                )),
            };
        };

        if definition.is_dark != appearance.is_dark {
            // Honoured rather than corrected. `is_dark` is metadata and the choice is the user's;
            // the pickers only offer matching themes, so this can only come from a hand-edited
            // settings file or a user theme whose own isDark changed later.
            return SystemThemeChoice {
                theme_id: definition.id.clone(),
                issue: Some(ThemeIssue::new(
                    ThemeIssueSeverity::Warning,
                    None,
                    format!(
                        "'{}' is set as your {slot} theme but is not a {slot} theme.",
                        definition.name
                    ),
                )),
            };
        }

        SystemThemeChoice::clean(&definition.id)
    }

    /// What one slot's picker offers. High contrast is excluded: the dark list answers "which dark
    /// theme do I like", and the accessibility theme arrives on its own switch rather than by
    /// being chosen here. `assigned_id` is kept in the list even when it fails the filter, or a
    /// slot holding a mismatched theme would render as an empty box and the only way to see what
    /// was set would be the settings file.
    pub fn slot_choices<'a>(
        available: &'a [ThemeDefinition],
        dark: bool,
        assigned_id: Option<&str>,
    ) -> Vec<&'a ThemeDefinition> {
        let high_contrast = SystemThemeRules::high_contrast_theme_id();
        let mut chosen: Vec<&ThemeDefinition> = available
            .iter()
            .filter(|t| t.is_dark == dark && !ids_equal(&t.id, high_contrast))
            .collect();

        if let Some(assigned) = assigned_id
            && !assigned.is_empty()
            && !chosen.iter().any(|t| ids_equal(&t.id, assigned))
            && let Some(odd) = available.iter().find(|t| ids_equal(&t.id, assigned))
        {
            chosen.insert(0, odd);
        }

        chosen
    }
}
