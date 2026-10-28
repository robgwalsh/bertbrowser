//! Commands: the one gate (`why_not`) and the one runner. A key, a menu row and the palette all go
//! through these, so the menu greying an item is a courtesy and this is the guard. Only commands the
//! native app can carry out are handled; every other catalogue id answers "not yet" rather than
//! doing nothing quietly. Spec: Core/Services/Commands, MainWindow.Commands.

use std::sync::Arc;

use bb_core::commands::{
    CommandCatalog, FileVerb, FileVerbRules, FileVerbSnapshot, FocusState, KeyChord, Keymap,
    KeymapRules,
};
use bb_core::delete::DeleteMode;

use crate::app::{Inner, TabData, lock};
use crate::ids::{AppError, AppResult};
use crate::tabs::{SelectOp, parent_path};

const NOT_YET: &str = "Not available in the native app yet.";

/// Ids with a handler here. Anything else in the catalogue is known but not runnable yet.
const HANDLED: &[&str] = &[
    "nav.back",
    "nav.forward",
    "nav.up",
    "nav.refresh",
    "tab.new",
    "tab.duplicate",
    "tab.close",
    "tab.reopen",
    "tab.next",
    "tab.previous",
    "pane.close",
    "pane.next",
    "pane.previous",
    "select.all",
    "select.none",
    "select.invert",
    "edit.undo",
    "edit.redo",
    "file.delete",
    "view.flat",
    "view.hidden",
    "search.clear",
];

pub fn is_handled(id: &str) -> bool {
    HANDLED.contains(&id)
}

pub(crate) fn build_keymap(overrides: Option<&bb_core::commands::Overrides>) -> Keymap {
    KeymapRules::resolve(&KeymapRules::catalog_bindables(), overrides)
}

fn selection_snapshot_counts(t: &TabData) -> (usize, usize) {
    let mut count = 0;
    let mut files = 0;
    for row in t.listing.rows() {
        if t.selection.contains(&row.id) {
            count += 1;
            if !row.is_directory {
                files += 1;
            }
        }
    }
    (count, files)
}

impl Inner {
    pub(crate) fn dispatch_key(&self, chord: &KeyChord, state: &FocusState) -> Option<String> {
        let keymap: Arc<Keymap> = lock(&self.keymap).clone();
        KeymapRules::dispatch(&keymap, chord, state)
    }

    pub(crate) fn rebuild_keymap(&self) {
        let overrides = lock(&self.settings).key_bindings.clone();
        *lock(&self.keymap) = Arc::new(build_keymap(overrides.as_ref()));
    }

    /// Why the command cannot run now, or None when it can. `Err` for an id nobody knows.
    pub(crate) fn why_not(&self, id: &str) -> AppResult<Option<String>> {
        if !CommandCatalog::is_known(id) {
            return Err(AppError::Invalid(format!("unknown command '{id}'")));
        }
        if !is_handled(id) {
            return Ok(Some(NOT_YET.to_string()));
        }
        let (transferring, paused) = (lock(&self.queue.state).draining, self.gate.is_paused());
        let active = lock(&self.shell).active_tab_id();
        let cell = active.and_then(|t| self.tab(t).ok());
        let Some(cell) = cell else {
            return Ok(Some("There is no tab.".to_string()));
        };
        let pane_count = lock(&self.shell).panes.len();
        let t = lock(&cell);
        let summary = lock(&self.undo_summary).clone();
        let reason = match id {
            "nav.back" => t.back.is_empty().then_some("Nothing to go back to."),
            "nav.forward" => t.forward.is_empty().then_some("Nothing to go forward to."),
            "nav.up" => parent_path(&t.path)
                .is_none()
                .then_some("Already at the top."),
            "nav.refresh" | "view.flat" | "search.clear" if t.path.is_empty() => {
                Some("This tab has no folder.")
            }
            "search.clear" => (t.search_text.is_empty() && t.global_search_text.is_empty())
                .then_some("There is no search to clear."),
            "edit.undo" => {
                if transferring && !paused {
                    Some("An operation is running.")
                } else if summary.undo.is_none() {
                    Some("Nothing to undo.")
                } else {
                    None
                }
            }
            "edit.redo" => {
                if transferring && !paused {
                    Some("An operation is running.")
                } else if summary.redo.is_none() {
                    Some("Nothing to redo.")
                } else {
                    None
                }
            }
            "select.all" | "select.invert" => t.listing.is_empty().then_some("Nothing is listed."),
            "file.delete" => {
                let (count, files) = selection_snapshot_counts(&t);
                let snapshot = FileVerbSnapshot::new(
                    count,
                    files,
                    false,
                    t.is_flattened && !t.is_flat_browse,
                    !t.path.is_empty(),
                );
                let state = FileVerbRules::for_verb(FileVerb::Delete, &snapshot);
                if !state.enabled {
                    return Ok(state.reason.or(Some("Not available.".to_string())));
                }
                if transferring {
                    Some("An operation is running.")
                } else {
                    None
                }
            }
            "pane.close" => (pane_count < 2).then_some("This is the only pane."),
            _ => None,
        };
        Ok(reason.map(str::to_string))
    }

    pub(crate) fn run_command(self: &Arc<Self>, id: &str) -> AppResult<()> {
        if let Some(reason) = self.why_not(id)? {
            return Err(AppError::Invalid(reason));
        }
        let (tab, pane) = {
            let shell = lock(&self.shell);
            let tab = shell.active_tab_id().ok_or(AppError::Stale)?;
            (tab, shell.active_pane)
        };
        match id {
            "nav.back" => self.back(tab),
            "nav.forward" => self.forward(tab),
            "nav.up" => self.up(tab),
            "nav.refresh" => self.refresh(tab),
            "tab.new" | "tab.duplicate" => {
                let cell = self.tab(tab)?;
                let path = lock(&cell).path.clone();
                self.open_tab(pane, &path, true).map(|_| ())
            }
            "tab.close" => self.close_tab(tab),
            "tab.reopen" => {
                let path = {
                    let mut shell = lock(&self.shell);
                    shell
                        .panes
                        .get_mut(&pane)
                        .and_then(|p| p.closed_paths.pop())
                };
                match path {
                    Some(path) => self.open_tab(pane, &path, true).map(|_| ()),
                    None => Err(AppError::Invalid("no closed tab to reopen".into())),
                }
            }
            "tab.next" => self.step_tab(pane, 1),
            "tab.previous" => self.step_tab(pane, -1),
            "pane.close" => self.close_pane(pane),
            "pane.next" => {
                self.step_pane(1);
                Ok(())
            }
            "pane.previous" => {
                self.step_pane(-1);
                Ok(())
            }
            "select.all" => self.select(tab, SelectOp::All, &[]),
            "select.none" => self.select(tab, SelectOp::Clear, &[]),
            "select.invert" => self.select(tab, SelectOp::Invert, &[]),
            "edit.undo" => self.op_undo_step(1).map(|_| ()),
            "edit.redo" => self.op_undo_step(-1).map(|_| ()),
            "file.delete" => {
                let paths = self.selected_paths(tab)?;
                self.op_delete(paths, DeleteMode::Recycle).map(|_| ())
            }
            "view.flat" => {
                let cell = self.tab(tab)?;
                let on = !lock(&cell).is_flat_view;
                self.set_flat_view(tab, on)
            }
            "view.hidden" => {
                let now = !lock(&self.settings).show_hidden_items;
                self.settings_set("showHiddenItems", if now { "true" } else { "false" })
            }
            "search.clear" => self.clear_search(tab),
            _ => Err(AppError::Invalid(NOT_YET.to_string())),
        }
    }
}
