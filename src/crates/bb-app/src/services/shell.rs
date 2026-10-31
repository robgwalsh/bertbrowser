//! The small shell services the commands and the palette lean on: bookmarks, the user's own
//! commands, the drive list, the two-pane comparison switch and what the host tells us about the
//! window (a dialog is up, the clipboard has files). Spec: ShellViewModel (bookmarks, compare),
//! MainWindow (IsContentBlocked), CustomCommandDefinition.

use std::collections::HashMap;
use std::sync::Arc;

use bb_core::custom_commands::CustomCommandDefinition;
use bb_core::data::{Bookmark, BookmarkRepository, BookmarkService};
use bb_core::layout::SplitOrientation;
use serde_json::{Value, json};
use windows_sys::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
};

use crate::app::{Inner, lock};
use crate::browse::tabs::tab_title;
use crate::commands::Ctx;
use crate::commands::keys::CUSTOM_PREFIX;
use crate::events::Model;
use crate::ids::{AppError, AppResult, PaneId, TabId};

/// Two panes under comparison. The engine that fills it in is not part of this layer; the switch,
/// its gates (no swapping panes, no moving tabs) and its state are.
#[derive(Clone, Debug)]
pub(crate) struct CompareState {
    pub left: PaneId,
    pub right: PaneId,
    pub differences_only: bool,
    /// The verdicts, once the folders have been read; settling by content replaces them.
    pub result: Option<std::sync::Arc<bb_core::compare::CompareResult>>,
    pub left_root: String,
    pub right_root: String,
    pub left_tab: Option<crate::ids::TabId>,
    pub right_tab: Option<crate::ids::TabId>,
    /// What the folder service answered, before any settling: what a sync is planned from.
    pub outcome: Option<std::sync::Arc<bb_core::compare::FolderCompareOutcome>>,
    /// A caveat to show with the summary (a partial scan, a drive still building).
    pub message: Option<String>,
    /// A scan is running.
    pub comparing: bool,
    /// Something compared has changed on disk since the scan; the verdicts on screen are older.
    pub stale: std::sync::Arc<std::sync::atomic::AtomicBool>,
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

    /// The sidebar's Bookmarks section: `[{path, name, isDirectory, hidden}]`, folders first. A
    /// hidden target is left out unless hidden items are shown, and dimmed when it is.
    pub(crate) fn bookmark_rows(&self) -> Vec<Value> {
        let show_hidden = lock(&self.settings).show_hidden_items;
        self.bookmark_list()
            .into_iter()
            .filter_map(|b| {
                let hidden = bookmark_is_hidden(&b.display_path);
                if hidden && !show_hidden {
                    return None;
                }
                let name = std::path::Path::new(&b.display_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| b.display_path.clone());
                Some(json!({
                    "path": b.display_path,
                    "name": name,
                    "isDirectory": b.is_directory,
                    "hidden": hidden,
                }))
            })
            .collect()
    }

    /// Opens a bookmark: `here` (the active tab), `new-tab`, `pane-right` or `pane-below`. A folder
    /// opens; a file opens the folder it is in with the file selected. A folder that is gone says so
    /// rather than leaving the tab where it was without a word.
    pub(crate) fn open_bookmark(self: &Arc<Self>, how: &str, path: &str) -> AppResult<()> {
        let is_dir = std::path::Path::new(path).is_dir();
        let folder = if is_dir {
            path.to_string()
        } else {
            crate::browse::tabs::parent_path(path).ok_or(AppError::Stale)?
        };
        let pane = lock(&self.shell).active_pane;
        let tab = match how {
            "here" => lock(&self.shell).active_tab_id().ok_or(AppError::Stale)?,
            // Behind the one in view. Spec: MainWindow.OpenBookmarkInNewTab (AddTab, activate: false).
            "new-tab" => self.open_tab(pane, &folder, false)?,
            "pane-right" | "pane-below" => {
                let orientation = if how == "pane-right" {
                    SplitOrientation::Vertical
                } else {
                    SplitOrientation::Horizontal
                };
                return self.split_pane(pane, orientation, &folder).map(|_| ());
            }
            other => {
                return Err(AppError::Invalid(format!(
                    "unknown bookmark verb '{other}'"
                )));
            }
        };
        if is_dir {
            return if how == "here" {
                self.navigate(tab, path)
            } else {
                Ok(())
            };
        }
        if !std::path::Path::new(path).exists() {
            self.set_status(tab, format!("Not found: {path}"));
            return Ok(());
        }
        self.reveal_file(tab, path)
    }

    pub(crate) fn is_bookmarked(&self, path: &str) -> Result<bool, AppError> {
        self.bookmark_service()
            .exists(path)
            .map_err(|e| AppError::Invalid(e.to_string()))
    }

    /// The selection's Bookmark command: when any is not bookmarked yet, bookmarks them all;
    /// otherwise removes them all. Spec: ShellViewModel.ToggleBookmarksAsync.
    pub(crate) fn toggle_bookmarks(&self, tab: TabId, paths: &[String]) -> AppResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let service = self.bookmark_service();
        let err = |e: bb_core::data::Error| AppError::Invalid(e.to_string());
        let mut any_missing = false;
        for p in paths {
            any_missing |= !service.exists(p).map_err(err)?;
        }
        for p in paths {
            if any_missing {
                service
                    .add(p, std::path::Path::new(p).is_dir())
                    .map_err(err)?;
            } else {
                service.remove(p).map_err(err)?;
            }
        }
        self.hub.model(Model::Saved);
        self.set_status(
            tab,
            if any_missing {
                format!("Bookmarked {} item(s)", paths.len())
            } else {
                format!("Removed {} bookmark(s)", paths.len())
            },
        );
        Ok(())
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

    pub(crate) fn compare_toggle(self: &std::sync::Arc<Self>) -> AppResult<()> {
        let mut ui = lock(&self.extra.ui);
        if ui.compare.take().is_some() {
            drop(ui);
            self.compare_clear();
            self.hub.model(Model::Compare);
            return Ok(());
        }
        drop(ui);
        // Always runnable, so pressing it can explain itself: a disabled button with a tooltip
        // nobody hovers is how "why is this greyed out?" happens.
        let panes = lock(&self.shell).layout.leaves().len();
        if panes != 2 {
            let why = if panes < 2 {
                "Comparing needs two panes side by side. Split this one with the split button above the file list, and open the other folder in it.".to_string()
            } else {
                format!(
                    "Comparing works between exactly two panes, and there are {panes} open. Close the ones you are not comparing."
                )
            };
            return Err(self.compare_refused(why));
        }
        // The two panes in screen order, whichever has the focus: the banner, the tints and Sync all
        // speak of "the left" and "the right", and Sync deletes from the right.
        let (left, right) = {
            let shell = lock(&self.shell);
            let leaves = shell.layout.leaves();
            (
                *shell.layout.value(leaves[0]),
                *shell.layout.value(leaves[1]),
            )
        };
        self.compare_start(left, right)
    }

    pub(crate) fn compare_end(&self) {
        if lock(&self.extra.ui).compare.take().is_some() {
            self.compare_clear();
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
            self.compare_clear();
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

/// The target's hidden attribute; false for a path that is gone or cannot be read.
fn bookmark_is_hidden(path: &str) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::metadata(path).is_ok_and(|m| m.file_attributes() & bb_core::attributes::HIDDEN != 0)
}
