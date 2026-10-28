//! The keymap's one writer and the Keyboard page's model. A shortcut is written the moment it is
//! assigned, never on the settings page's debounce: it can be rebound from the palette while the
//! page is open, and a page that wrote its own copy 400 ms later would put the old one back.
//! Spec: KeymapService, SettingsViewModel.Keyboard.

use bb_core::commands::{
    Bindable, CommandCatalog, CommandContext, CommandMatcher, KeyChord, Keymap, KeymapRules,
    Overrides,
};
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::events::Model;
use crate::ids::{AppError, AppResult};

pub const CUSTOM_PREFIX: &str = "custom:";

/// A command as a list of them is drawn: id, name, category, icon, aliases.
pub(crate) type CommandRow = (String, String, String, Option<String>, Vec<String>);

#[derive(Default)]
pub(crate) struct KeyPage {
    pub filter: String,
    pub selected: Option<String>,
    pub recording: bool,
    pub recording_adds: bool,
    pub finding: bool,
    pub pending: Option<KeyChord>,
    pub can_reassign: bool,
    pub text: String,
}

impl KeyPage {
    pub fn is_capturing(&self) -> bool {
        self.recording || self.finding
    }

    pub fn cancel_capture(&mut self) {
        self.recording = false;
        self.finding = false;
        self.can_reassign = false;
        self.pending = None;
        self.text.clear();
    }
}

fn context_name(c: CommandContext) -> &'static str {
    match c {
        CommandContext::App => "app",
        CommandContext::Browser => "browser",
        CommandContext::FileList => "file-list",
    }
}

impl Inner {
    /// Everything a shortcut can be given to: the catalogue, then the user's own commands, which
    /// ship with none and act on the active tab like any other browser command.
    pub(crate) fn bindables(&self) -> Vec<Bindable> {
        let mut all = KeymapRules::catalog_bindables();
        let mut seen = std::collections::HashSet::new();
        for c in lock(&self.settings).custom_commands.iter() {
            if seen.insert(c.id.clone()) {
                all.push(Bindable::new(
                    &format!("{CUSTOM_PREFIX}{}", c.id),
                    CommandContext::Browser,
                    &[],
                ));
            }
        }
        all
    }

    fn overrides(&self) -> Option<Overrides> {
        lock(&self.settings).key_bindings.clone()
    }

    pub(crate) fn resolve_keymap(&self) -> Keymap {
        KeymapRules::resolve(&self.bindables(), self.overrides().as_ref())
    }

    fn store_overrides(&self, overrides: Overrides) -> AppResult<()> {
        lock(&self.settings).key_bindings = if overrides.is_empty() {
            None
        } else {
            Some(overrides)
        };
        self.save_settings_now()?;
        self.rebuild_keymap();
        self.hub.model(Model::KeyPage);
        Ok(())
    }

    /// Gives a command exactly these shortcuts, taking each from whoever had it.
    pub(crate) fn keymap_set_chords(&self, id: &str, chords: &[KeyChord]) -> AppResult<()> {
        let next =
            KeymapRules::with_chords(&self.bindables(), self.overrides().as_ref(), id, chords);
        self.store_overrides(next)
    }

    pub(crate) fn keymap_reset(&self, id: &str) -> AppResult<()> {
        let next = KeymapRules::with_defaults(&self.bindables(), self.overrides().as_ref(), id);
        self.store_overrides(next)
    }

    pub(crate) fn keymap_reset_all(&self) -> AppResult<()> {
        self.store_overrides(Overrides::new())
    }

    /// Re-reads the settings: the custom commands may have changed under the keymap.
    pub(crate) fn keymap_refresh(&self) {
        self.rebuild_keymap();
    }

    pub(crate) fn keymap_gesture(&self, id: &str) -> String {
        lock(&self.keymap).gesture_text(id)
    }

    pub(crate) fn keymap_chords(&self, id: &str) -> Vec<String> {
        lock(&self.keymap)
            .chords_for(id)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn key_name_of(&self, id: &str) -> String {
        CommandCatalog::find(id)
            .map(|c| c.name.to_string())
            .or_else(|| self.custom_command(id).map(|c| c.name))
            .unwrap_or_else(|| id.to_string())
    }

    // ---- the page ----

    fn key_rows(&self, page: &KeyPage) -> Vec<Value> {
        let keymap = lock(&self.keymap).clone();
        let bindables: std::collections::HashMap<String, Bindable> = self
            .bindables()
            .into_iter()
            .map(|b| (b.id.clone(), b))
            .collect();
        let filter = page.filter.trim().to_string();
        let mut rows: Vec<CommandRow> = CommandCatalog::all()
            .iter()
            .map(|c| {
                (
                    c.id.to_string(),
                    c.name.to_string(),
                    c.category.to_string(),
                    c.icon.map(str::to_string),
                    c.aliases.iter().map(|a| a.to_string()).collect(),
                )
            })
            .collect();
        for id in self.custom_command_ids() {
            if let Some(c) = self.custom_command(&id) {
                rows.push((
                    id,
                    c.name,
                    "Custom commands".to_string(),
                    Some(
                        if c.run_elevated {
                            "Icon.Shield"
                        } else {
                            "Icon.CustomCommand"
                        }
                        .to_string(),
                    ),
                    Vec::new(),
                ));
            }
        }
        let mut out = Vec::new();
        let mut heading: Option<String> = None;
        for (id, name, category, icon, aliases) in rows {
            let Some(bindable) = bindables.get(&id) else {
                continue;
            };
            let chords = keymap.chords_for(&id);
            let chord_text = chords
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            if !filter.is_empty() {
                let alias_refs: Vec<&str> = aliases.iter().map(String::as_str).collect();
                let matched = CommandMatcher::matches(&filter, &name, &alias_refs, &category, &chord_text)
                        .is_match()
                        // Any of its shortcuts, not just the first: a command found by its second
                        // chord is still the command that key runs.
                        || chords.iter().any(|c| {
                            c.to_string()
                                .to_lowercase()
                                .contains(&filter.to_lowercase())
                        });
                if !matched {
                    continue;
                }
            }
            if heading.as_deref() != Some(&category) {
                out.push(json!({ "heading": true, "name": category }));
                heading = Some(category.clone());
            }
            let defaults = &bindable.defaults;
            let changed = chords != defaults.as_slice();
            out.push(json!({
                "heading": false,
                "id": id,
                "name": name,
                "category": category,
                "icon": icon,
                "context": context_name(bindable.context),
                "where": match bindable.context {
                    CommandContext::FileList => "in the file list",
                    CommandContext::App => "anywhere",
                    CommandContext::Browser => "",
                },
                "chords": chords.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "changed": changed,
                "defaultText": if defaults.is_empty() {
                    "Ships with no shortcut".to_string()
                } else {
                    format!("Ships as {}", defaults.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))
                },
            }));
        }
        out
    }

    pub(crate) fn key_page_json(&self) -> Value {
        let page = lock(&self.extra.keys);
        let rows = self.key_rows(&page);
        let has_changed = rows.iter().any(|r| r["changed"].as_bool().unwrap_or(false));
        let selected = page
            .selected
            .clone()
            .filter(|id| rows.iter().any(|r| r["id"].as_str() == Some(id)));
        json!({
            "filter": page.filter,
            "selected": selected,
            "recording": page.recording,
            "finding": page.finding,
            "capturing": page.is_capturing(),
            "canReassign": page.can_reassign,
            "text": page.text,
            "hasChanged": has_changed,
            "rows": rows,
        })
    }

    fn key_page_changed(&self) {
        self.hub.model(Model::KeyPage);
    }

    pub(crate) fn key_set_filter(&self, text: &str) {
        lock(&self.extra.keys).filter = text.to_string();
        self.key_page_changed();
    }

    /// Selects one command, clearing the filter if it would hide it. How the palette's "change
    /// shortcut" lands here.
    pub(crate) fn key_select(&self, id: &str) {
        let mut page = lock(&self.extra.keys);
        let visible = self
            .key_rows(&page)
            .iter()
            .any(|r| r["id"].as_str() == Some(id));
        if !visible {
            page.filter.clear();
        }
        page.selected = Some(id.to_string());
        drop(page);
        self.key_page_changed();
    }

    pub(crate) fn key_begin(&self, adds: bool) -> AppResult<()> {
        let mut page = lock(&self.extra.keys);
        let id = page
            .selected
            .clone()
            .ok_or_else(|| AppError::Invalid("no command is selected".into()))?;
        let name = self.key_name_of(&id);
        page.recording_adds = adds;
        page.pending = None;
        page.can_reassign = false;
        page.finding = false;
        page.recording = true;
        page.text = if adds {
            format!("Press another shortcut for \u{201c}{name}\u{201d}. Esc cancels.")
        } else {
            format!("Press the new shortcut for \u{201c}{name}\u{201d}. Esc cancels.")
        };
        drop(page);
        self.key_page_changed();
        Ok(())
    }

    pub(crate) fn key_find(&self) {
        let mut page = lock(&self.extra.keys);
        page.recording = false;
        page.can_reassign = false;
        page.finding = true;
        page.text = "Press a shortcut to see what it runs. Esc cancels.".to_string();
        drop(page);
        self.key_page_changed();
    }

    pub(crate) fn key_cancel(&self) {
        lock(&self.extra.keys).cancel_capture();
        self.key_page_changed();
    }

    /// A chord pressed while the recorder was listening.
    pub(crate) fn key_offer(&self, chord_text: &str) -> AppResult<()> {
        let chord = KeyChord::try_parse(chord_text)
            .ok_or_else(|| AppError::Invalid(format!("'{chord_text}' is not a key")))?;
        let keymap = lock(&self.keymap).clone();
        let mut page = lock(&self.extra.keys);

        if page.finding {
            page.finding = false;
            page.text.clear();
            // The filter does the looking up: the list narrows to what is on that key, with the
            // command it actually runs selected, or to nothing, which is the answer too.
            page.filter = chord.to_string();
            let runs = keymap.owner_of(&chord).map(str::to_string);
            page.selected = runs.or_else(|| {
                self.key_rows(&page)
                    .iter()
                    .find_map(|r| r["id"].as_str().map(str::to_string))
            });
            drop(page);
            self.key_page_changed();
            return Ok(());
        }
        if !page.recording {
            return Ok(());
        }
        let Some(id) = page.selected.clone() else {
            return Ok(());
        };
        let context = self
            .bindables()
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.context)
            .unwrap_or(CommandContext::Browser);
        if let Some(refusal) = KeymapRules::refuse(&chord, context) {
            // Still listening: a refused key is a reason to try another, not to start again.
            page.text = format!(
                "{chord} can\u{2019}t be used \u{2014} {refusal} Press another, or Esc to cancel."
            );
            page.can_reassign = false;
            page.pending = None;
            drop(page);
            self.key_page_changed();
            return Ok(());
        }
        let owner = keymap.owner_of(&chord).map(str::to_string);
        match owner {
            None => {
                let adds = page.recording_adds;
                page.cancel_capture();
                drop(page);
                self.assign(&id, chord, adds)
            }
            Some(o) if o == id => {
                let adds = page.recording_adds;
                page.cancel_capture();
                drop(page);
                self.assign(&id, chord, adds)
            }
            Some(owner) => {
                // Somebody else's: say whose, and take it only when asked to. Listening stops
                // here, so Tab, Space and Enter are keys again and can reach the Reassign button.
                let theirs = self.key_name_of(&owner);
                let mine = self.key_name_of(&id);
                page.pending = Some(chord);
                page.recording = false;
                page.can_reassign = true;
                page.text = format!(
                    "{chord} already runs \u{201c}{theirs}\u{201d}. Reassign it to \u{201c}{mine}\u{201d}?"
                );
                drop(page);
                self.key_page_changed();
                Ok(())
            }
        }
    }

    fn assign(&self, id: &str, chord: KeyChord, adds: bool) -> AppResult<()> {
        let current: Vec<KeyChord> = lock(&self.keymap).chords_for(id).to_vec();
        let chords = if adds {
            if current.contains(&chord) {
                current
            } else {
                let mut next = current;
                next.push(chord);
                next
            }
        } else {
            vec![chord]
        };
        self.keymap_set_chords(id, &chords)
    }

    pub(crate) fn key_reassign(&self) -> AppResult<()> {
        let mut page = lock(&self.extra.keys);
        let (Some(chord), Some(id)) = (page.pending, page.selected.clone()) else {
            return Ok(());
        };
        let adds = page.recording_adds;
        page.cancel_capture();
        drop(page);
        self.assign(&id, chord, adds)
    }

    pub(crate) fn key_remove(&self) -> AppResult<()> {
        let id = self.key_selected()?;
        self.key_cancel();
        self.keymap_set_chords(&id, &[])
    }

    pub(crate) fn key_reset_selected(&self) -> AppResult<()> {
        let id = self.key_selected()?;
        self.key_cancel();
        self.keymap_reset(&id)
    }

    pub(crate) fn key_reset_all(&self) -> AppResult<()> {
        self.key_cancel();
        self.keymap_reset_all()
    }

    fn key_selected(&self) -> AppResult<String> {
        lock(&self.extra.keys)
            .selected
            .clone()
            .ok_or_else(|| AppError::Invalid("no command is selected".into()))
    }

    /// The palette's "change shortcut": the Keyboard page, on that command, already listening.
    pub(crate) fn key_change_shortcut(&self, id: &str) -> AppResult<()> {
        self.settings_open_page("keyboard")?;
        self.key_select(id);
        self.key_begin(false)
    }
}
