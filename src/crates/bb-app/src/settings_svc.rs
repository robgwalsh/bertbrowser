//! Settings as the Settings page drives them: applied in memory at once, written on a 400 ms
//! debounce driven by an allowlist of persisted properties (so selecting a row or a page writes
//! nothing), flushed on Back and on window close. The services that own their own keys (keymap,
//! command bars, theme, palette pins, session) write at once instead, through `save_settings_now`.
//! A list with a missing field keeps the whole list unwritten and says so in a strip; only leaving
//! (`settings_leave`) takes the user to it, and the host then asks whether to leave anyway. Spec: SettingsViewModel (Apply, Flush, TryLeave).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde_json::Value;

use crate::app::{Inner, lock};
use crate::events::AppEvent;
use crate::ids::{AppError, AppResult};

/// The pages of the Settings page, in order: (id, title, icon).
pub const SETTINGS_PAGES: [(&str, &str, &str); 12] = [
    ("general", "General", "Icon.Settings"),
    ("appearance", "Appearance", "Icon.Appearance"),
    ("preview", "Preview", "Icon.PreviewPane"),
    ("search-index", "Search index", "Icon.Indexing"),
    ("history", "History", "Icon.Changes"),
    ("new-items", "New items", "Icon.Add"),
    ("columns", "Columns", "Icon.Columns"),
    ("saved-searches", "Saved searches", "Icon.Search"),
    ("workspaces", "Workspaces", "Icon.SplitRight"),
    ("context-menu", "Context menu", "Icon.CustomCommand"),
    ("command-bars", "Menus and toolbars", "Icon.Reorder"),
    ("keyboard", "Keyboard", "Icon.Keyboard"),
];

/// The settings keys the page writes. Everything else is written at once by the service that owns
/// it, or is not a preference at all.
pub const PAGE_KEYS: &[&str] = &[
    "showHiddenItems",
    "enterArchivesOnDoubleClick",
    "drivesOpenTarget",
    "restoreLastSession",
    "startupDefaultPath",
    "scrollSpeedMultiplier",
    "showPreviewPane",
    "showMetadataPane",
    "previewPaneWidth",
    "previewTextMaxBytes",
    "previewAutoAdvance",
    "previewLoop",
    "previewAutoPlay",
    "searchContentMaxBytes",
    "duplicateMinSizeBytes",
    "duplicateSkipSystemFolders",
    "checksumAlgorithms",
    "recordFileChanges",
    "startIndexerAtLaunch",
    "fileChangeRetentionHours",
    "undoMaxSteps",
    "undoMaxHeldGigabytes",
    "tileAspectRatio",
    "customCommands",
    "showShellExtensions",
    "hiddenShellExtensions",
    "hiddenBuiltInMenuItems",
    "contextMenuLayout",
    "newFileTypes",
    "fileListColumns",
    "workspacesPlacement",
    "savedSearchesPlacement",
    "advancedRenameExpanded",
    "advancedRename",
];

/// Keys whose change must reach the theme service.
const THEME_KEYS: &[&str] = &[
    "themeId",
    "followSystemTheme",
    "lightThemeId",
    "darkThemeId",
    "themeOverrides",
];

/// The two lists that are held unwritten while an entry in them is incomplete.
#[derive(Clone, Copy)]
enum HeldList {
    NewFileTypes,
    CustomCommands,
}

impl HeldList {
    fn of(key: &str) -> Option<HeldList> {
        match key {
            "newFileTypes" => Some(HeldList::NewFileTypes),
            "customCommands" => Some(HeldList::CustomCommands),
            _ => None,
        }
    }

    /// The page that shows the list; leaving a refused page goes there.
    fn page(self) -> &'static str {
        match self {
            HeldList::NewFileTypes => "new-items",
            HeldList::CustomCommands => "context-menu",
        }
    }
}

#[derive(Default)]
pub(crate) struct SaveState {
    generation: AtomicU64,
    /// Why each list the page holds cannot be written yet: (new file types, custom commands).
    problem: Mutex<(Option<String>, Option<String>)>,
    /// Something changed in memory since the file was last written.
    dirty: std::sync::atomic::AtomicBool,
}

fn custom_command_problem(value: &Value) -> Option<String> {
    let list = value.as_array()?;
    for item in list {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("");
        let command = item.get("command").and_then(Value::as_str).unwrap_or("");
        if name.trim().is_empty() || command.trim().is_empty() {
            return Some("Every command needs a name and a program.".into());
        }
        // A command offered for neither could never be in a menu. Spec: SettingsViewModel.CommandProblem.
        let files = item
            .get("appliesToFiles")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let folders = item
            .get("appliesToDirectories")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !files && !folders {
            return Some(format!("'{name}' must apply to files, folders, or both."));
        }
    }
    None
}

fn new_file_type_problem(value: &Value) -> Option<String> {
    let list = value.as_array()?;
    for item in list {
        let label = item.get("label").and_then(Value::as_str).unwrap_or("");
        let extension = item.get("extension").and_then(Value::as_str).unwrap_or("");
        if label.trim().is_empty() {
            return Some("Every file type needs a name.".into());
        }
        if !extension.starts_with('.') || extension.len() < 2 {
            return Some(format!(
                "'{label}' needs an extension starting with a dot, like \".txt\"."
            ));
        }
        if let Some(why) =
            bb_core::rename::RenamePattern::validate(&format!("x{}", extension.trim()))
        {
            return Some(format!(
                "'{label}' has an extension that can't end a file name \u{2014} {why}"
            ));
        }
    }
    None
}

impl Inner {
    /// Writes the settings file now, whatever is pending.
    pub(crate) fn save_settings_now(&self) -> AppResult<()> {
        self.extra.save.generation.fetch_add(1, Ordering::SeqCst);
        let result = lock(&self.settings).save(&self.settings_path);
        match result {
            Ok(()) => {
                self.extra.save.dirty.store(false, Ordering::SeqCst);
                Ok(())
            }
            Err(e) => Err(AppError::Invalid(e.to_string())),
        }
    }

    /// Writes after `settings_debounce_ms` of quiet. Zero writes at once.
    pub(crate) fn schedule_save(self: &Arc<Self>) {
        self.extra.save.dirty.store(true, Ordering::SeqCst);
        let delay = self.config.settings_debounce_ms;
        let generation = self.extra.save.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if delay == 0 {
            let _ = self.save_settings_now();
            return;
        }
        let weak: Weak<Inner> = Arc::downgrade(self);
        let _ = std::thread::Builder::new()
            .name("bb-settings-save".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(delay));
                if let Some(inner) = weak.upgrade()
                    && inner.extra.save.generation.load(Ordering::SeqCst) == generation
                {
                    let _ = inner.save_settings_now();
                }
            });
    }

    /// Writes anything pending. Called on Back and on window close.
    pub(crate) fn flush_settings(&self) {
        if self.extra.save.dirty.load(Ordering::SeqCst) {
            let _ = self.save_settings_now();
        }
    }

    pub(crate) fn settings_set(self: &Arc<Self>, key: &str, json_value: &str) -> AppResult<()> {
        let value: Value = serde_json::from_str(json_value)
            .map_err(|e| AppError::Invalid(format!("not JSON: {e}")))?;
        if let Some(list) = HeldList::of(key) {
            let problem = match list {
                HeldList::CustomCommands => custom_command_problem(&value),
                HeldList::NewFileTypes => new_file_type_problem(&value),
            };
            let held = problem.is_some();
            {
                let mut slots = lock(&self.extra.save.problem);
                match list {
                    HeldList::NewFileTypes => slots.0 = problem,
                    HeldList::CustomCommands => slots.1 = problem,
                }
            }
            if held {
                // Held, not written: the page keeps editing and the strip says why nothing is saved.
                self.hub.emit(AppEvent::SettingsChanged {
                    key: key.to_string(),
                });
                return Ok(());
            }
        }
        {
            let mut settings = lock(&self.settings);
            let Value::Object(mut map) =
                serde_json::to_value(&*settings).map_err(|e| AppError::Invalid(e.to_string()))?
            else {
                return Err(AppError::Invalid("settings are not an object".into()));
            };
            if !map.contains_key(key) {
                return Err(AppError::Invalid(format!("unknown setting '{key}'")));
            }
            map.insert(key.to_string(), value);
            let next: bb_core::settings::AppSettings =
                serde_json::from_value(Value::Object(map))
                    .map_err(|e| AppError::Invalid(format!("'{key}': {e}")))?;
            *settings = next;
        }
        if PAGE_KEYS.contains(&key) {
            self.schedule_save();
        } else {
            self.save_settings_now()?;
        }
        match key {
            "keyBindings" => self.rebuild_keymap(),
            "customCommands" => self.keymap_refresh(),
            "showHiddenItems" => {
                self.refresh_all();
                // The Bookmarks section filters hidden targets too.
                self.hub.model(crate::events::Model::Saved);
            }
            "commandBars" => self.reload_bars(),
            "fileListColumns" => self.columns_default_changed(),
            // Read afresh by every search, through the closure `App::new` gave the search service.
            // Spec: App.xaml.cs (the content budget is read per search).
            "searchContentMaxBytes" => {
                let bytes = lock(&self.settings).search_content_max_bytes;
                let budget = &self.content_budget;
                budget.store(i64::from(bytes), Ordering::Relaxed);
            }
            // Lowering a limit releases entries, so it is applied the moment it changes.
            // Spec: MainWindow.Settings_Applied (SetUndoBudgetAsync).
            "undoMaxSteps" | "undoMaxHeldGigabytes" => {
                let budget = lock(&self.settings).effective_undo_budget();
                self.set_undo_budget(budget);
            }
            "recordFileChanges" | "fileChangeRetentionHours" => self.change_log_policy_changed(),
            k if THEME_KEYS.contains(&k) => self.theme_settings_changed(),
            _ => {}
        }
        self.hub.emit(AppEvent::SettingsChanged {
            key: key.to_string(),
        });
        Ok(())
    }

    /// The first list that cannot be written and why (file types before commands, as the original
    /// checks them), without the strip's trailing sentence.
    fn held_list(&self) -> Option<(HeldList, String)> {
        let slots = lock(&self.extra.save.problem);
        let (types, commands) = &*slots;
        if let Some(why) = types {
            return Some((HeldList::NewFileTypes, why.clone()));
        }
        commands.clone().map(|why| (HeldList::CustomCommands, why))
    }

    /// The strip the page shows when a list could not be written, if any.
    /// Spec: SettingsViewModel.Apply (PendingProblem).
    pub(crate) fn settings_problem(&self) -> Option<String> {
        self.held_list()
            .map(|(_, why)| format!("{why} That list is not saved until this is fixed."))
    }

    /// Leaving the page: writes what is pending, and refuses (with the reason) while a list is
    /// still unwritable. Like `TryLeave`, a refusal brings the offending list's page to the front;
    /// the reason is the entry's own, since the host asks whether to leave anyway.
    pub(crate) fn settings_leave(&self) -> Result<(), String> {
        self.flush_settings();
        match self.held_list() {
            Some((list, why)) => {
                let _ = self.settings_open_page(list.page());
                Err(why)
            }
            None => Ok(()),
        }
    }

    pub(crate) fn settings_open_page(&self, page: &str) -> AppResult<()> {
        let id = if page.is_empty() {
            "general"
        } else {
            SETTINGS_PAGES
                .iter()
                .map(|p| p.0)
                .find(|p| p.eq_ignore_ascii_case(page))
                .ok_or_else(|| AppError::Invalid(format!("no settings page '{page}'")))?
        };
        {
            let mut ui = lock(&self.extra.ui);
            // Changing page stops listening for a shortcut.
            if ui.settings_page.as_deref() != Some(id) {
                lock(&self.extra.keys).cancel_capture();
            }
            ui.settings_page = Some(id.to_string());
        }
        self.hub.model(crate::events::Model::SettingsPage);
        Ok(())
    }

    /// Closes the page, flushing first. Err carries the problem that kept it open.
    pub(crate) fn settings_close_page(&self) -> Result<(), String> {
        if lock(&self.extra.ui).settings_page.is_none() {
            return Ok(());
        }
        self.settings_leave()?;
        lock(&self.extra.keys).cancel_capture();
        lock(&self.extra.ui).settings_page = None;
        self.hub.model(crate::events::Model::SettingsPage);
        Ok(())
    }
}
