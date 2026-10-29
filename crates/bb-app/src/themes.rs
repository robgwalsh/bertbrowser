//! Themes: which definition is showing, from Windows when following and from the user's pick when
//! not, with the user's colour tweaks over it. Deliberately thin: inheritance, fallbacks and
//! validation are `ThemeResolver`'s in Core; this decides which definition to resolve, hands the
//! palette to the host (`ModelChanged(Theme)` says "re-push it") and remembers the choice.
//! Spec: App/Theming/ThemeService.cs, Core/Theming/SystemThemeRules.
//!
//! Consequences that are easy to undo: matching Windows is the default only because there is no
//! settings file (`follows_system_theme`, pinned on first run); a system-driven switch is
//! `apply_for_current_state` and never `theme_select`, because selecting writes `themeId` and the OS
//! writing it would convert a following user into a pinned one at sunset; `themeId` is left alone
//! while following so unticking restores it; high contrast wins over both slots.

use std::collections::{BTreeMap, HashMap};

use bb_core::settings::AppPaths;
use bb_core::theming::{
    ColorMap, ResolvedTheme, SystemAppearance, SystemThemeRules, ThemeCatalog, ThemeColor,
    ThemeDefinition, ThemeId, ThemeJson, ThemeResolver, ThemeToken, ids_equal,
};
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::events::Model;
use crate::ids::{AppError, AppResult};

pub(crate) struct ThemeState {
    pub themes_dir: std::path::PathBuf,
    pub available: Vec<ThemeDefinition>,
    pub definition_id: String,
    pub following: bool,
    pub appearance: SystemAppearance,
    /// Colour tweaks in force, saved or not.
    pub overrides: BTreeMap<String, String>,
    pub persisted: BTreeMap<String, String>,
    /// Unsaved edits parked by theme id for the session when the system switched under them.
    pub parked: HashMap<String, BTreeMap<String, String>>,
    pub store_issues: Vec<String>,
    pub follow_issues: Vec<String>,
}

impl ThemeState {
    pub fn new(paths: &AppPaths) -> ThemeState {
        ThemeState {
            themes_dir: paths.themes_dir(),
            available: ThemeCatalog::built_ins().to_vec(),
            definition_id: ThemeCatalog::default_theme().id.clone(),
            following: false,
            appearance: SystemAppearance::DARK,
            overrides: BTreeMap::new(),
            persisted: BTreeMap::new(),
            parked: HashMap::new(),
            store_issues: Vec::new(),
            follow_issues: Vec::new(),
        }
    }

    fn find(&self, id: &str) -> Option<&ThemeDefinition> {
        self.available.iter().find(|t| ids_equal(&t.id, id))
    }

    fn definition(&self) -> &ThemeDefinition {
        self.find(&self.definition_id)
            .unwrap_or_else(|| ThemeCatalog::default_theme())
    }

    pub fn resolved(&self) -> ResolvedTheme {
        let map = color_map(&self.overrides);
        ThemeResolver::resolve_with(self.definition(), |id| self.find(id), Some(&map))
    }

    fn resolved_without_overrides(&self) -> ResolvedTheme {
        ThemeResolver::resolve(self.definition(), |id| self.find(id))
    }

    fn reload_available(&mut self) {
        let (user, issues) = load_user_themes(&self.themes_dir);
        let mut all = ThemeCatalog::built_ins().to_vec();
        all.extend(user);
        self.available = all;
        self.store_issues = issues;
    }
}

fn color_map(overrides: &BTreeMap<String, String>) -> ColorMap {
    let mut map = ColorMap::new();
    for (k, v) in overrides {
        map.insert(k, v);
    }
    map
}

/// The user's own themes: one `<id>.json` each. The id is the file stem, which must be a safe name.
fn load_user_themes(dir: &std::path::Path) -> (Vec<ThemeDefinition>, Vec<String>) {
    let mut themes = Vec::new();
    let mut issues = Vec::new();
    let Ok(read) = std::fs::read_dir(dir) else {
        return (themes, issues);
    };
    let mut files: Vec<_> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        })
        .collect();
    files.sort();
    for file in files {
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = file
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) => {
                issues.push(format!("{name}: {e}"));
                continue;
            }
        };
        match ThemeJson::try_deserialize(&text) {
            Ok(mut def) => {
                if !ThemeId::is_safe(&stem) || ThemeCatalog::find(&stem).is_some() {
                    issues.push(format!("{name}: not a usable theme id"));
                    continue;
                }
                def.id = stem;
                def.is_built_in = false;
                themes.push(def);
            }
            Err(e) => issues.push(format!("{name}: {e}")),
        }
    }
    (themes, issues)
}

fn argb(c: ThemeColor) -> u32 {
    u32::from(c.a) << 24 | u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}

impl Inner {
    /// First run of the service: find the themes, decide whether to follow Windows, apply.
    pub(crate) fn theme_initialize(&self) {
        let (following, appearance) = {
            let settings = lock(&self.settings);
            (
                settings.follows_system_theme(),
                self.seams.appearance.current(),
            )
        };
        {
            let mut st = lock(&self.extra.theme);
            st.reload_available();
            st.following = following;
            st.appearance = appearance;
            self.apply_for_current_state(&mut st);
        }
    }

    /// Picks up a change the host made to the system appearance without telling us (a seam read).
    fn theme_sync_appearance(&self) {
        let now = self.seams.appearance.current();
        let changed = lock(&self.extra.theme).appearance != now;
        if changed {
            self.theme_system_changed(now);
        }
    }

    /// Resolves what should be showing right now and applies it. Persists nothing.
    fn apply_for_current_state(&self, st: &mut ThemeState) {
        let (id, issues) = {
            let settings = lock(&self.settings);
            if st.following {
                let choice = SystemThemeRules::decide(
                    st.appearance,
                    settings.light_theme_id.as_deref(),
                    settings.dark_theme_id.as_deref(),
                    |id| st.find(id),
                );
                (
                    choice.theme_id,
                    choice
                        .issue
                        .map(|i| i.message.clone())
                        .into_iter()
                        .collect(),
                )
            } else {
                (
                    settings.theme_id.clone().unwrap_or_else(|| {
                        // Our control templates are fully custom, so they do not pick up Windows'
                        // high-contrast colours: honouring the setting with a matching theme is the
                        // closest equivalent.
                        if st.appearance.is_high_contrast {
                            ThemeCatalog::high_contrast_dark().id.clone()
                        } else {
                            ThemeCatalog::default_theme().id.clone()
                        }
                    }),
                    Vec::new(),
                )
            }
        };
        st.follow_issues = issues;
        self.load_overrides(st, &id);
        self.apply(st, &id);
    }

    fn load_overrides(&self, st: &mut ThemeState, id: &str) {
        let saved = lock(&self.settings)
            .theme_overrides
            .get(id)
            .cloned()
            .unwrap_or_default();
        st.persisted = saved.clone();
        st.overrides = saved;
    }

    fn apply(&self, st: &mut ThemeState, id: &str) {
        st.definition_id = st
            .find(id)
            .map(|d| d.id.clone())
            .unwrap_or_else(|| ThemeCatalog::default_theme().id.clone());
        self.hub.model(Model::Theme);
    }

    /// The settings file's theme keys changed under us (a hand edit, a page write).
    pub(crate) fn theme_settings_changed(&self) {
        let mut st = lock(&self.extra.theme);
        st.following = lock(&self.settings).follows_system_theme();
        self.apply_for_current_state(&mut st);
    }

    pub(crate) fn theme_is_following(&self) -> bool {
        lock(&self.extra.theme).following
    }

    pub(crate) fn theme_current_id(&self) -> String {
        lock(&self.extra.theme).definition_id.clone()
    }

    pub(crate) fn theme_available(&self) -> Vec<ThemeDefinition> {
        lock(&self.extra.theme).available.clone()
    }

    /// One ARGB value per `ThemeToken`, in token order, for what is showing now.
    pub(crate) fn theme_palette(&self) -> Vec<u32> {
        self.theme_sync_appearance();
        let st = lock(&self.extra.theme);
        let resolved = st.resolved();
        ThemeToken::ALL
            .iter()
            .map(|t| argb(resolved.color(*t)))
            .collect()
    }

    /// Choosing one theme by hand is the end of following: the two pickers that stay visible in
    /// follow mode go through `theme_slot_set`.
    pub(crate) fn theme_select(&self, id: &str) {
        {
            let mut st = lock(&self.extra.theme);
            let Some(found) = st.find(id).map(|d| d.id.clone()) else {
                return;
            };
            self.load_overrides(&mut st, &found);
            self.apply(&mut st, &found);
            st.following = false;
            st.follow_issues.clear();
            let mut settings = lock(&self.settings);
            settings.follow_system_theme = Some(false);
            settings.theme_id = Some(found);
        }
        let _ = self.save_settings_now();
        self.hub.model(Model::Theme);
    }

    pub(crate) fn theme_follow_set(&self, follow: bool) {
        {
            let mut st = lock(&self.extra.theme);
            if st.following == follow {
                return;
            }
            {
                let mut settings = lock(&self.settings);
                // Unticking keeps what is on screen: the checkbox says "stop tracking Windows", not
                // "go back to whatever was selected last year".
                if !follow {
                    settings.theme_id = Some(st.definition_id.clone());
                }
                settings.follow_system_theme = Some(follow);
            }
            st.following = follow;
            st.appearance = self.seams.appearance.current();
            self.apply_for_current_state(&mut st);
        }
        let _ = self.save_settings_now();
        self.hub.model(Model::Theme);
    }

    pub(crate) fn theme_slot_set(&self, dark: bool, id: &str) -> AppResult<()> {
        let mut st = lock(&self.extra.theme);
        let Some(found) = st.find(id).map(|d| d.id.clone()) else {
            return Err(AppError::Invalid(format!("no theme '{id}'")));
        };
        {
            let mut settings = lock(&self.settings);
            if dark {
                settings.dark_theme_id = Some(found);
            } else {
                settings.light_theme_id = Some(found);
            }
        }
        let _ = self.save_settings_now();
        // Applied only when it is the slot Windows is asking for now: setting the light theme while
        // Windows is dark configures, it does not preview.
        if st.following {
            self.apply_for_current_state(&mut st);
        } else {
            self.hub.model(Model::Theme);
        }
        Ok(())
    }

    /// The host tells us what Windows looks like now (light/dark, high contrast). Nothing about the
    /// OS moving belongs in the file: writing it would convert someone following into someone pinned.
    pub(crate) fn theme_system_changed(&self, appearance: SystemAppearance) {
        self.seams.appearance.set(appearance);
        let mut st = lock(&self.extra.theme);
        st.appearance = appearance;
        // Read even when not following, so the default is right if following is turned on later in
        // the session, but nothing is applied: an unfollowed theme is the user's.
        if !st.following {
            return;
        }
        let choice = {
            let settings = lock(&self.settings);
            SystemThemeRules::decide(
                appearance,
                settings.light_theme_id.as_deref(),
                settings.dark_theme_id.as_deref(),
                |id| st.find(id),
            )
        };
        st.follow_issues = choice
            .issue
            .map(|i| i.message.clone())
            .into_iter()
            .collect();
        // Dark+ in both slots, or high contrast toggled while already on the accessibility theme:
        // repainting would be a no-op that still tells the host to re-push.
        if ids_equal(&choice.theme_id, &st.definition_id) {
            return;
        }
        // A system-driven switch is not a decision anyone made, so unsaved edits are parked and put
        // back if it switches back.
        if st.overrides != st.persisted {
            let (id, overrides) = (st.definition_id.clone(), st.overrides.clone());
            st.parked.insert(id, overrides);
        }
        self.load_overrides(&mut st, &choice.theme_id);
        if let Some(parked) = st.parked.remove(&choice.theme_id) {
            st.overrides = parked;
        }
        self.apply(&mut st, &choice.theme_id);
    }

    // ---- the colour editor ----

    pub(crate) fn theme_override_set(&self, token: &str, color: Option<&str>) -> AppResult<()> {
        if !ThemeToken::is_known(token) {
            return Err(AppError::Invalid(format!("no colour token '{token}'")));
        }
        let mut st = lock(&self.extra.theme);
        match color {
            Some(literal) => {
                let parsed = ThemeColor::try_parse(literal)
                    .ok_or_else(|| AppError::Invalid(format!("'{literal}' is not a colour")))?;
                st.overrides.insert(token.to_string(), parsed.to_hex());
            }
            None => {
                st.overrides.remove(token);
            }
        }
        self.hub.model(Model::Theme);
        Ok(())
    }

    pub(crate) fn theme_overrides_persist(&self) -> AppResult<()> {
        {
            let mut st = lock(&self.extra.theme);
            let pruned = ThemeResolver::prune_overrides(
                &color_map(&st.overrides),
                &st.resolved_without_overrides(),
            );
            let pruned: BTreeMap<String, String> = pruned
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            st.overrides = pruned.clone();
            st.persisted = pruned.clone();
            let mut settings = lock(&self.settings);
            if pruned.is_empty() {
                settings.theme_overrides.remove(&st.definition_id);
            } else {
                settings
                    .theme_overrides
                    .insert(st.definition_id.clone(), pruned);
            }
        }
        self.save_settings_now()?;
        self.hub.model(Model::Theme);
        Ok(())
    }

    pub(crate) fn theme_overrides_revert(&self) {
        {
            let mut st = lock(&self.extra.theme);
            st.overrides = st.persisted.clone();
        }
        self.hub.model(Model::Theme);
    }

    pub(crate) fn theme_overrides_reset_all(&self) -> AppResult<()> {
        lock(&self.extra.theme).overrides.clear();
        self.theme_overrides_persist()
    }

    /// Bakes what is showing into a theme of its own.
    pub(crate) fn theme_save_as(&self, name: &str) -> AppResult<String> {
        if name.trim().is_empty() {
            return Err(AppError::Invalid("Give the theme a name.".into()));
        }
        let id = {
            let mut st = lock(&self.extra.theme);
            let resolved = st.resolved();
            let id = ThemeId::unique(name, st.available.iter().map(|t| t.id.as_str()));
            let mut colors = ColorMap::new();
            for token in ThemeToken::ALL {
                colors.insert(token.name(), &resolved.color(token).to_hex());
            }
            // Baked flat rather than left as a diff: a saved theme should not change because a
            // future release retunes Dark+.
            let definition = ThemeDefinition {
                id: id.clone(),
                name: name.trim().to_string(),
                base_theme_id: None,
                is_dark: resolved.is_dark,
                colors,
                is_built_in: false,
            };
            std::fs::create_dir_all(&st.themes_dir)
                .map_err(|e| AppError::Invalid(e.to_string()))?;
            std::fs::write(
                st.themes_dir.join(format!("{id}.json")),
                ThemeJson::serialize(&definition),
            )
            .map_err(|e| AppError::Invalid(e.to_string()))?;
            st.reload_available();
            st.overrides.clear();
            st.persisted.clear();
            lock(&self.settings).theme_overrides.remove(&id);
            self.apply(&mut st, &id);
            let mut settings = lock(&self.settings);
            if st.following {
                // Baked from whatever is on screen, so it belongs to the slot that produced it.
                if definition.is_dark {
                    settings.dark_theme_id = Some(id.clone());
                } else {
                    settings.light_theme_id = Some(id.clone());
                }
            } else {
                settings.theme_id = Some(id.clone());
            }
            id
        };
        self.save_settings_now()?;
        Ok(id)
    }

    /// Writes the theme on screen, overrides and all, as a theme file someone else can import.
    /// Spec: ThemeService.TryExport.
    pub(crate) fn theme_export(&self, path: &str) -> AppResult<()> {
        let text =
            bb_core::theming::ThemeJson::serialize_resolved(&lock(&self.extra.theme).resolved());
        std::fs::write(path, text).map_err(|e| AppError::Invalid(e.to_string()))
    }

    pub(crate) fn theme_delete(&self, id: &str) -> AppResult<()> {
        if ThemeCatalog::find(id).is_some() {
            return Err(AppError::Invalid(
                "Built-in themes can't be deleted.".into(),
            ));
        }
        {
            let mut st = lock(&self.extra.theme);
            if !ThemeId::is_safe(id) {
                return Err(AppError::Invalid("That is not a theme of yours.".into()));
            }
            let file = st.themes_dir.join(format!("{id}.json"));
            std::fs::remove_file(&file).map_err(|e| AppError::Invalid(e.to_string()))?;
            {
                let mut settings = lock(&self.settings);
                settings.theme_overrides.remove(id);
                // We know this one is gone, because we removed it; a slot that merely cannot find
                // its theme (a file still syncing) is left alone by the rules instead.
                if settings
                    .light_theme_id
                    .as_deref()
                    .is_some_and(|t| ids_equal(t, id))
                {
                    settings.light_theme_id = None;
                }
                if settings
                    .dark_theme_id
                    .as_deref()
                    .is_some_and(|t| ids_equal(t, id))
                {
                    settings.dark_theme_id = None;
                }
            }
            st.parked.remove(id);
            st.reload_available();
            let was_showing = ids_equal(&st.definition_id, id);
            if st.following || was_showing {
                if was_showing && !st.following {
                    lock(&self.settings).theme_id = None;
                }
                self.apply_for_current_state(&mut st);
            }
        }
        self.save_settings_now()?;
        self.hub.model(Model::Theme);
        Ok(())
    }

    /// Copies a theme file into the themes folder under a safe, unused id.
    pub(crate) fn theme_import(&self, source: &str) -> AppResult<String> {
        let text = std::fs::read_to_string(source).map_err(|e| AppError::Invalid(e.to_string()))?;
        let parsed = ThemeJson::try_deserialize(&text).map_err(AppError::Invalid)?;
        let mut st = lock(&self.extra.theme);
        // Rename rather than overwrite: importing must never silently replace a theme the user
        // already has, and a built-in id would shadow something we ship. The id also becomes a
        // filename, so keeping it is conditional on it being safe.
        let keep = ThemeId::is_safe(&parsed.id)
            && ThemeCatalog::find(&parsed.id).is_none()
            && st.find(&parsed.id).is_none();
        let id = if keep {
            parsed.id.clone()
        } else {
            ThemeId::unique(&parsed.name, st.available.iter().map(|t| t.id.as_str()))
        };
        let definition = ThemeDefinition {
            id: id.clone(),
            is_built_in: false,
            ..parsed
        };
        std::fs::create_dir_all(&st.themes_dir).map_err(|e| AppError::Invalid(e.to_string()))?;
        std::fs::write(
            st.themes_dir.join(format!("{id}.json")),
            ThemeJson::serialize(&definition),
        )
        .map_err(|e| AppError::Invalid(e.to_string()))?;
        st.reload_available();
        drop(st);
        self.hub.model(Model::Theme);
        Ok(id)
    }

    pub(crate) fn theme_summary_json(&self) -> Value {
        let st = lock(&self.extra.theme);
        json!({ "id": st.definition_id, "following": st.following })
    }

    pub(crate) fn theme_tokens_json(&self) -> Value {
        self.theme_sync_appearance();
        let st = lock(&self.extra.theme);
        let resolved = st.resolved();
        Value::Array(
            ThemeToken::DESCRIPTORS
                .iter()
                .map(|d| {
                    json!({
                        "key": d.key,
                        "group": d.group,
                        "name": d.display_name,
                        "description": d.description,
                        "core": d.is_core,
                        "argb": argb(resolved.color(d.token)),
                        "overridden": st.overrides.contains_key(d.key),
                    })
                })
                .collect(),
        )
    }

    pub(crate) fn theme_json(&self) -> Value {
        self.theme_sync_appearance();
        let st = lock(&self.extra.theme);
        let resolved = st.resolved();
        let settings = lock(&self.settings);
        let slot = |id: &Option<String>, fallback: &str| {
            id.clone().unwrap_or_else(|| fallback.to_string())
        };
        let light = slot(
            &settings.light_theme_id,
            SystemThemeRules::default_light_theme_id(),
        );
        let dark = slot(
            &settings.dark_theme_id,
            SystemThemeRules::default_dark_theme_id(),
        );
        // Each slot offers the themes of its own kind, plus whatever it holds now (a hand-edited file).
        let choices = |is_dark: bool, held: &str| {
            SystemThemeRules::slot_choices(&st.available, is_dark, Some(held))
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<_>>()
        };
        json!({
            "lightChoices": choices(false, &light),
            "darkChoices": choices(true, &dark),
            "id": st.definition_id,
            "name": resolved.name,
            "isDark": resolved.is_dark,
            "following": st.following,
            "lightSlot": slot(&settings.light_theme_id, SystemThemeRules::default_light_theme_id()),
            "darkSlot": slot(&settings.dark_theme_id, SystemThemeRules::default_dark_theme_id()),
            "appearance": {
                "dark": st.appearance.is_dark,
                "highContrast": st.appearance.is_high_contrast,
            },
            "dimmedIconOpacity": if resolved.is_dark { 0.55 } else { 0.45 },
            "available": st.available.iter().map(|t| json!({
                "id": t.id, "name": t.name, "isDark": t.is_dark, "builtIn": t.is_built_in,
                "current": ids_equal(&t.id, &st.definition_id),
            })).collect::<Vec<_>>(),
            "overridden": st.overrides.keys().collect::<Vec<_>>(),
            "dirty": st.overrides != st.persisted,
            "issues": st.follow_issues.iter().chain(st.store_issues.iter())
                .cloned().chain(resolved.issues.iter().map(|i| i.message.clone()))
                .collect::<Vec<_>>(),
        })
    }
}
