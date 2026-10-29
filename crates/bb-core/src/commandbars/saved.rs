//! The decisions behind saved menu-and-toolbar arrangements: whether one may be saved under a
//! name, and what saving, renaming and removing do to the list. They behave as saved workspaces
//! do — a name is one arrangement whatever its case, and saving under a name already there
//! replaces it — but live in the settings file, so the list operations are here rather than in SQL.
//! Spec: Core/Services/CommandBars/SavedCommandBarRules.cs.

use bb_paths::fold_upper;

use super::{CommandBarRules, CommandBarSet, SavedCommandBarSet};
use crate::FileTime;
use crate::rules::SavedWorkspaceRules;

pub struct SavedCommandBarRules;

impl SavedCommandBarRules {
    pub const MAX_NAME_LENGTH: usize = SavedWorkspaceRules::MAX_NAME_LENGTH;

    /// The first reason the arrangement cannot be saved under this name, in words for the user, or
    /// None when it can. `name_taken` is asked about the trimmed name; the caller excludes the one
    /// being renamed.
    pub fn validate(name: &str, name_taken: impl Fn(&str) -> bool) -> Option<String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Some("Give the arrangement a name.".into());
        }
        if trimmed.chars().count() > Self::MAX_NAME_LENGTH {
            return Some(format!(
                "Keep the name under {} characters.",
                Self::MAX_NAME_LENGTH
            ));
        }
        if name_taken(trimmed) {
            return Some(format!(
                "There is already an arrangement called \"{trimmed}\"."
            ));
        }
        None
    }

    /// "Arrangement 2026-03-05 14:30" (local wall-clock ticks).
    pub fn default_name(now_local: FileTime) -> String {
        let c = now_local.civil();
        format!(
            "Arrangement {:04}-{:02}-{:02} {:02}:{:02}",
            c.year, c.month, c.day, c.hour, c.minute
        )
    }

    pub fn find<'a>(saved: &'a [SavedCommandBarSet], name: &str) -> Option<&'a SavedCommandBarSet> {
        saved.iter().find(|s| is(s, name))
    }

    /// The list with this arrangement saved: in place of one of the same name, taking the casing
    /// just typed, or added. Always in name order.
    pub fn upsert(
        saved: &[SavedCommandBarSet],
        arrangement: SavedCommandBarSet,
    ) -> Vec<SavedCommandBarSet> {
        let named = SavedCommandBarSet {
            name: arrangement.name.trim().to_string(),
            set: arrangement.set,
        };
        let mut list: Vec<SavedCommandBarSet> = saved
            .iter()
            .filter(|s| !is(s, &named.name))
            .cloned()
            .collect();
        list.push(named);
        sorted(list)
    }

    /// The list with one arrangement renamed, or None when the new name belongs to another or
    /// there is nothing by the old one. A change of case alone is a rename.
    pub fn rename(
        saved: &[SavedCommandBarSet],
        old_name: &str,
        new_name: &str,
    ) -> Option<Vec<SavedCommandBarSet>> {
        let trimmed = new_name.trim();
        let target = saved.iter().position(|s| is(s, old_name))?;
        if saved
            .iter()
            .enumerate()
            .any(|(i, s)| i != target && is(s, trimmed))
        {
            return None;
        }

        let mut list = saved.to_vec();
        list[target].name = trimmed.to_string();
        Some(sorted(list))
    }

    pub fn remove(saved: &[SavedCommandBarSet], name: &str) -> Vec<SavedCommandBarSet> {
        saved.iter().filter(|s| !is(s, name)).cloned().collect()
    }

    /// The name of the saved arrangement this one is, or None when it is none of them — which is
    /// how the page knows to say "(modified)" and whether a recall would lose work.
    pub fn name_matching<'a>(
        saved: &'a [SavedCommandBarSet],
        current: &CommandBarSet,
    ) -> Option<&'a str> {
        saved
            .iter()
            .find(|s| CommandBarRules::same(&s.set, current))
            .map(|s| s.name.as_str())
    }

    /// Whether replacing the current arrangement would throw away something that exists nowhere
    /// else: it is neither the default nor any saved one.
    pub fn needs_confirm_before_replace(
        current: &CommandBarSet,
        saved: &[SavedCommandBarSet],
    ) -> bool {
        !CommandBarRules::is_default(current) && Self::name_matching(saved, current).is_none()
    }

    /// "6 menus, 2 toolbars" — the one-line shape, for the Settings list.
    pub fn shape_text(set: &CommandBarSet) -> String {
        let menus = set.menus.len();
        let toolbars = 1 + set.rows.len();
        format!(
            "{menus} menu{}, {toolbars} toolbar{}",
            if menus == 1 { "" } else { "s" },
            if toolbars == 1 { "" } else { "s" }
        )
    }
}

fn is(saved: &SavedCommandBarSet, name: &str) -> bool {
    fold_upper(&saved.name) == fold_upper(name.trim())
}

fn sorted(mut list: Vec<SavedCommandBarSet>) -> Vec<SavedCommandBarSet> {
    list.sort_by_cached_key(|s| fold_upper(&s.name));
    list
}
