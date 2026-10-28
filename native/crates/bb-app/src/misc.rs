//! The small shell services the commands and the palette lean on: bookmarks, the user's own
//! commands, the drive list, the two-pane comparison switch and what the host tells us about the
//! window (a dialog is up, the clipboard has files). Spec: ShellViewModel (bookmarks, compare),
//! MainWindow (IsContentBlocked), CustomCommandDefinition.

use std::collections::HashMap;

use bb_core::custom_commands::CustomCommandDefinition;
use bb_core::data::{Bookmark, BookmarkRepository, BookmarkService};
use serde_json::{Value, json};
use windows_sys::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
};

use crate::app::{Inner, lock};
use crate::commands::Ctx;
use crate::events::Model;
use crate::ids::{AppError, AppResult, PaneId};
use crate::keys::CUSTOM_PREFIX;
use crate::tabs::tab_title;

/// Two panes under comparison. The engine that fills it in is not part of this layer; the switch,
/// its gates (no swapping panes, no moving tabs) and its state are.
#[derive(Clone, Debug)]
pub(crate) struct CompareState {
    pub left: PaneId,
    pub right: PaneId,
    pub differences_only: bool,
}

/// What the host has told us about the window, and what the Settings page is showing.
#[derive(Default)]
pub(crate) struct UiState {
    /// The Settings page is up, on this page.
    pub settings_page: Option<String>,
    /// Modal dialogs up: the title bar stays live, everything about the folders does not.
    pub dialogs: u32,
    pub compare: Option<CompareState>,
    pub banner_dismissed: bool,
    flags: HashMap<String, bool>,
}

impl UiState {
    pub fn flag(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }
}

impl Inner {
    // ---- bookmarks ----

    fn bookmark_service(&self) -> BookmarkService {
        BookmarkService::new(BookmarkRepository::new(self.db.clone()))
    }

    pub(crate) fn bookmark_list(&self) -> Vec<Bookmark> {
        self.bookmark_service().get_all().unwrap_or_default()
    }

    pub(crate) fn is_bookmarked(&self, path: &str) -> Result<bool, AppError> {
        self.bookmark_service()
            .exists(path)
            .map_err(|e| AppError::Invalid(e.to_string()))
    }

    /// Bookmarks the path, or removes the bookmark if there is one.
    pub(crate) fn toggle_bookmark(&self, path: &str, is_directory: bool) -> AppResult<()> {
        let service = self.bookmark_service();
        let err = |e: bb_core::data::Error| AppError::Invalid(e.to_string());
        if service.exists(path).map_err(err)? {
            service.remove(path).map_err(err)?;
        } else if !service.add(path, is_directory).map_err(err)? {
            return Err(AppError::Invalid("That cannot be bookmarked.".into()));
        }
        self.hub.model(Model::Saved);
        Ok(())
    }

    // ---- the user's own commands ----

    pub(crate) fn custom_command(&self, id: &str) -> Option<CustomCommandDefinition> {
        let wanted = id.strip_prefix(CUSTOM_PREFIX)?;
        lock(&self.settings)
            .custom_commands
            .iter()
            .find(|c| c.id == wanted)
            .cloned()
    }

    pub(crate) fn custom_command_ids(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        lock(&self.settings)
            .custom_commands
            .iter()
            .filter(|c| seen.insert(c.id.clone()))
            .map(|c| format!("{CUSTOM_PREFIX}{}", c.id))
            .collect()
    }

    /// Why one of the user's commands cannot run here, or None. It acts on the selection or, with
    /// nothing selected, on the folder being shown.
    pub(crate) fn custom_command_reason(&self, ctx: &Ctx, id: &str) -> Option<String> {
        let command = self.custom_command(id)?;
        if ctx.in_archive_now() {
            return Some("Not available inside an archive.".to_string());
        }
        let (files, dirs) = ctx.target_counts();
        if (files > 0 && command.applies_to_files) || (dirs > 0 && command.applies_to_directories) {
            return None;
        }
        Some(
            match (command.applies_to_files, command.applies_to_directories) {
                (true, false) => "It applies to files \u{2014} select one.",
                (false, true) => {
                    "It applies to folders \u{2014} select one, or clear the selection for this folder."
                }
                _ => "There is nothing here for it to act on.",
            }
            .to_string(),
        )
    }

    // ---- drives and tabs ----

    /// Every drive root with a label that never waits on a drive that is not there: asking an empty
    /// card reader for its label is what makes a list of drives take seconds to appear.
    pub(crate) fn drive_list(&self) -> Vec<(String, String)> {
        let mut buffer = vec![0u16; 512];
        let n = unsafe { GetLogicalDriveStringsW(buffer.len() as u32, buffer.as_mut_ptr()) };
        if n == 0 || n as usize > buffer.len() {
            return Vec::new();
        }
        buffer[..n as usize]
            .split(|c| *c == 0)
            .filter(|s| !s.is_empty())
            .map(|root| {
                let text = String::from_utf16_lossy(root);
                let wide: Vec<u16> = root.iter().copied().chain([0]).collect();
                let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
                let label = match kind {
                    4 => "Network".to_string(),
                    5 => "CDRom".to_string(),
                    2 => "Removable".to_string(),
                    3 => {
                        let mut name = [0u16; 261];
                        let ok = unsafe {
                            GetVolumeInformationW(
                                wide.as_ptr(),
                                name.as_mut_ptr(),
                                name.len() as u32,
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                                0,
                            )
                        };
                        if ok != 0 {
                            let end = name.iter().position(|c| *c == 0).unwrap_or(0);
                            String::from_utf16_lossy(&name[..end])
                        } else {
                            String::new()
                        }
                    }
                    _ => String::new(),
                };
                (text, label)
            })
            .collect()
    }

    /// Every open tab, panes in on-screen order: its path and its title.
    pub(crate) fn all_tab_titles(&self) -> Vec<(String, String)> {
        let shell = lock(&self.shell);
        let mut out = Vec::new();
        for leaf in shell.layout.leaves() {
            let pane = *shell.layout.value(leaf);
            let Some(p) = shell.panes.get(&pane) else {
                continue;
            };
            for t in &p.tabs {
                if let Some(cell) = shell.tabs.get(t) {
                    let path = lock(cell).path.clone();
                    out.push((path.clone(), tab_title(&path)));
                }
            }
        }
        out
    }

    // ---- comparing two panes ----

    pub(crate) fn compare_toggle(&self) -> AppResult<()> {
        let mut ui = lock(&self.extra.ui);
        if ui.compare.take().is_some() {
            drop(ui);
            self.hub.model(Model::Compare);
            return Ok(());
        }
        drop(ui);
        let left = lock(&self.shell).active_pane;
        let right = self.other_pane(left).ok_or_else(|| {
            AppError::Invalid("There is no other pane \u{2014} split this one first.".into())
        })?;
        lock(&self.extra.ui).compare = Some(CompareState {
            left,
            right,
            differences_only: false,
        });
        self.hub.model(Model::Compare);
        Ok(())
    }

    pub(crate) fn compare_end(&self) {
        if lock(&self.extra.ui).compare.take().is_some() {
            self.hub.model(Model::Compare);
        }
    }

    pub(crate) fn compare_pane_closed(&self, pane: PaneId) {
        let mut ui = lock(&self.extra.ui);
        if ui
            .compare
            .as_ref()
            .is_some_and(|c| c.left == pane || c.right == pane)
        {
            ui.compare = None;
            drop(ui);
            self.hub.model(Model::Compare);
        }
    }

    // ---- the window, as the host tells us ----

    pub(crate) fn ui_set_flag(&self, name: &str, value: bool) {
        lock(&self.extra.ui).flags.insert(name.to_string(), value);
        if name == "propertyProvider" {
            for (id, _) in self.all_tabs() {
                self.columns_changed(id);
            }
        }
        self.hub.model(Model::Commands);
    }

    /// A modal dialog opened (`true`) or closed.
    pub(crate) fn ui_set_dialog_open(&self, open: bool) {
        {
            let mut ui = lock(&self.extra.ui);
            ui.dialogs = if open {
                ui.dialogs + 1
            } else {
                ui.dialogs.saturating_sub(1)
            };
        }
        self.hub.model(Model::Commands);
    }

    pub(crate) fn settings_value(&self, key: &str) -> Option<Value> {
        let value = serde_json::to_value(&*lock(&self.settings)).ok()?;
        value.get(key).cloned()
    }

    pub(crate) fn ui_json(&self) -> Value {
        let ui = lock(&self.extra.ui);
        json!({
            "settingsPage": ui.settings_page,
            "dialogs": ui.dialogs,
            "compare": ui.compare.as_ref().map(|c| json!({
                "left": c.left.0, "right": c.right.0, "differencesOnly": c.differences_only,
            })),
        })
    }
}
