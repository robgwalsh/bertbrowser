//! The public surface of the app: what the FFI, the harness and the tests call. Every method is a
//! thin front for the `Inner` verbs; none of them blocks on long work (a load, a search or an
//! operation runs on a worker and reports through events).

use std::sync::Arc;

use bb_core::columns::ResolvedColumn;
use bb_core::commands::{FocusState, KeyChord};
use bb_core::delete::DeleteMode;
use bb_core::layout::{NodeId, SplitOrientation};
use bb_core::transfer::TransferVerb;
use serde_json::{Value, json};

use crate::app::{App, Inner, lock};
use crate::commands::CommandOutcome;
use crate::ids::{AppError, AppResult, OpId, PaneId, TabId};
use crate::rows::Listing;
use crate::tabs::{Crumb, SelectOp, breadcrumb, parent_path, tab_title};

/// A tab as a host paints its chrome from it.
#[derive(Clone, Debug)]
pub struct TabInfo {
    pub id: TabId,
    pub pane: PaneId,
    pub path: String,
    pub title: String,
    pub can_back: bool,
    pub can_forward: bool,
    pub can_up: bool,
    pub version: u64,
    pub count: usize,
    pub selected: usize,
    pub is_flattened: bool,
    pub is_flat_browse: bool,
    pub is_flat_view: bool,
    pub is_global_search: bool,
    pub search_text: String,
    pub status: String,
    pub error: Option<String>,
    pub empty_message: Option<String>,
    pub notice: Option<String>,
    pub sort_by: String,
    pub sort_desc: bool,
    pub thumbnail_scale: f64,
    pub side_pane: u8,
    /// The preview pane's mode (0 auto, 1 raw, 2 hex) and its two per-tab switches.
    pub preview_mode: u8,
    pub preview_wrap: bool,
    pub preview_fit: bool,
    pub shows_content_matches: bool,
    pub is_search_running: bool,
    pub is_loading: bool,
    /// The listing failed because the archive needs a password (the banner offers Unlock).
    pub is_archive_locked: bool,
    /// Bumped when the core selected something the view should scroll to (`reveal_row`).
    pub reveal_seq: u64,
    pub reveal_row: Option<usize>,
}

impl App {
    // ---- panes and tabs ----

    pub fn active_pane(&self) -> PaneId {
        lock(&self.inner.shell).active_pane
    }

    pub fn active_tab(&self) -> Option<TabId> {
        lock(&self.inner.shell).active_tab_id()
    }

    pub fn panes(&self) -> Vec<PaneId> {
        let shell = lock(&self.inner.shell);
        shell
            .layout
            .leaves()
            .into_iter()
            .map(|n| *shell.layout.value(n))
            .collect()
    }

    pub fn pane_tabs(&self, pane: PaneId) -> AppResult<Vec<TabId>> {
        let shell = lock(&self.inner.shell);
        Ok(shell.panes.get(&pane).ok_or(AppError::Stale)?.tabs.clone())
    }

    pub fn active_tab_of(&self, pane: PaneId) -> AppResult<Option<TabId>> {
        let shell = lock(&self.inner.shell);
        Ok(shell.panes.get(&pane).ok_or(AppError::Stale)?.active)
    }

    /// Opens a tab in `pane` (the active pane when None).
    pub fn open_tab(&self, pane: Option<PaneId>, path: &str, activate: bool) -> AppResult<TabId> {
        let pane = pane.unwrap_or_else(|| self.active_pane());
        self.inner.open_tab(pane, path, activate)
    }

    pub fn close_tab(&self, tab: TabId) -> AppResult<()> {
        self.inner.close_tab(tab)
    }

    pub fn activate_tab(&self, tab: TabId) -> AppResult<()> {
        self.inner.activate_tab(tab)
    }

    pub fn split_pane(
        &self,
        pane: PaneId,
        orientation: SplitOrientation,
        path: &str,
    ) -> AppResult<PaneId> {
        self.inner.split_pane(pane, orientation, path)
    }

    pub fn close_pane(&self, pane: PaneId) -> AppResult<()> {
        self.inner.close_pane(pane)
    }

    pub fn activate_pane(&self, pane: PaneId) -> AppResult<()> {
        self.inner.activate_pane(pane)
    }

    // ---- navigation ----

    pub fn navigate(&self, tab: TabId, path: &str) -> AppResult<()> {
        self.inner.navigate(tab, path)
    }

    pub fn back(&self, tab: TabId) -> AppResult<()> {
        self.inner.back(tab)
    }

    pub fn forward(&self, tab: TabId) -> AppResult<()> {
        self.inner.forward(tab)
    }

    pub fn up(&self, tab: TabId) -> AppResult<()> {
        self.inner.up(tab)
    }

    pub fn refresh(&self, tab: TabId) -> AppResult<()> {
        self.inner.refresh(tab)
    }

    pub fn set_search(&self, tab: TabId, text: &str, global: bool) -> AppResult<()> {
        self.inner.set_search(tab, text, global)
    }

    pub fn clear_search(&self, tab: TabId) -> AppResult<()> {
        self.inner.clear_search(tab)
    }

    pub fn set_flat_view(&self, tab: TabId, on: bool) -> AppResult<()> {
        self.inner.set_flat_view(tab, on)
    }

    /// Lowers (or restores) how many rows a flat listing shows before it stops and says so; takes
    /// effect on the next flat listing. For the harness: a dozen files cannot reach the real ceiling.
    pub fn set_flat_cap(&self, cap: usize) -> AppResult<()> {
        if cap == 0 {
            return Err(AppError::Invalid("the flat cap must be at least 1".into()));
        }
        self.inner
            .extra
            .flat_cap
            .store(cap, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Sorts by a column id; the same column again flips the direction.
    pub fn set_sort(&self, tab: TabId, column: &str) -> AppResult<()> {
        self.inner.set_sort(tab, column)
    }

    pub fn set_thumbnail_scale(&self, tab: TabId, scale: f64) -> AppResult<()> {
        self.inner.set_thumbnail_scale(tab, scale)
    }

    /// 0 none, 1 preview, 2 metadata.
    pub fn set_side_pane(&self, tab: TabId, pane: u8) -> AppResult<()> {
        self.inner.set_side_pane(tab, pane)
    }

    pub fn open_item(&self, tab: TabId, index: u32) -> AppResult<()> {
        self.inner.open_item(tab, index)
    }

    // ---- reading a tab ----

    pub fn tab_info(&self, tab: TabId) -> AppResult<TabInfo> {
        let cell = self.inner.tab(tab)?;
        let t = lock(&cell);
        Ok(TabInfo {
            id: t.id,
            pane: t.pane,
            path: t.path.clone(),
            title: tab_title(&t.path),
            can_back: !t.back.is_empty(),
            can_forward: !t.forward.is_empty(),
            can_up: !t.path.is_empty() && parent_path(&t.path).is_some(),
            version: t.version,
            count: t.listing.len(),
            selected: t.selection.len(),
            is_flattened: t.is_flattened,
            is_flat_browse: t.is_flat_browse,
            is_flat_view: t.is_flat_view,
            is_global_search: t.is_global_search(),
            search_text: t.active_search_text().to_string(),
            status: t.status.clone(),
            error: t.error.clone(),
            empty_message: t.empty_message.clone(),
            notice: t.notice.clone(),
            sort_by: t.sort_by.clone(),
            sort_desc: t.sort_desc,
            thumbnail_scale: t.thumbnail_scale,
            side_pane: t.side_pane,
            preview_mode: t.preview_mode,
            preview_wrap: t.preview_wrap,
            preview_fit: t.preview_fit,
            shows_content_matches: t.shows_content_matches,
            is_search_running: t.is_search_running,
            is_loading: t.is_loading,
            is_archive_locked: t.archive_locked,
            reveal_seq: t.reveal_seq,
            reveal_row: t.reveal_row,
        })
    }

    /// "3 items selected (1.2 MB)", or `None` with nothing selected.
    pub fn selection_summary(&self, tab: TabId) -> AppResult<Option<String>> {
        self.inner.selection_summary(tab)
    }

    pub fn breadcrumb(&self, tab: TabId) -> AppResult<Vec<Crumb>> {
        let cell = self.inner.tab(tab)?;
        Ok(breadcrumb(&lock(&cell).path))
    }

    /// The tab's current snapshot. Hold it as long as you paint from it.
    pub fn listing(&self, tab: TabId) -> AppResult<Arc<Listing>> {
        let cell = self.inner.tab(tab)?;
        Ok(lock(&cell).listing.clone())
    }

    /// The columns the tab shows, with the mode-driven ones (Folder, Match) in place.
    pub fn columns(&self, tab: TabId) -> AppResult<Vec<ResolvedColumn>> {
        let cell = self.inner.tab(tab)?;
        let t = lock(&cell);
        Ok(t.resolved_columns())
    }

    /// As `columns`, each with the cell index `bb_listing_cell` takes (-1: not readable here).
    pub fn columns_with_cells(&self, tab: TabId) -> AppResult<Vec<(ResolvedColumn, i64)>> {
        self.inner.tab_columns(tab)
    }

    // ---- selection ----

    pub fn select(&self, tab: TabId, op: SelectOp, indices: &[u32]) -> AppResult<()> {
        self.inner.select(tab, op, indices)
    }

    pub fn selected_indices(&self, tab: TabId) -> AppResult<Vec<u32>> {
        self.inner.selected_indices(tab)
    }

    pub fn selected_paths(&self, tab: TabId) -> AppResult<Vec<String>> {
        self.inner.selected_paths(tab)
    }

    // ---- operations ----

    pub fn op_move(&self, sources: Vec<String>, destination: &str) -> AppResult<OpId> {
        self.inner
            .op_transfer(sources, destination.to_string(), TransferVerb::Move)
    }

    pub fn op_copy(&self, sources: Vec<String>, destination: &str) -> AppResult<OpId> {
        self.inner
            .op_transfer(sources, destination.to_string(), TransferVerb::Copy)
    }

    pub fn op_delete(&self, paths: Vec<String>, mode: DeleteMode) -> AppResult<OpId> {
        self.inner.op_delete(paths, mode)
    }

    pub fn op_rename(&self, paths: Vec<String>, pattern: &str) -> AppResult<OpId> {
        self.inner.op_rename(paths, pattern.to_string())
    }

    pub fn op_pause(&self, paused: bool) {
        self.inner.set_paused(paused);
    }

    pub fn op_cancel(&self, op: OpId) -> AppResult<()> {
        self.inner.cancel_op(op)
    }

    /// Positive undoes that many entries, negative redoes.
    pub fn undo_step(&self, steps: i32) -> AppResult<OpId> {
        self.inner.op_undo_step(steps)
    }

    pub fn can_undo(&self) -> bool {
        lock(&self.inner.undo_summary).undo.is_some() && !self.is_transferring()
    }

    pub fn can_redo(&self) -> bool {
        lock(&self.inner.undo_summary).redo.is_some() && !self.is_transferring()
    }

    pub fn undo_description(&self) -> Option<String> {
        lock(&self.inner.undo_summary).undo.clone()
    }

    pub fn redo_description(&self) -> Option<String> {
        lock(&self.inner.undo_summary).redo.clone()
    }

    /// Commits what the undo history is holding (the window is closing).
    pub fn release_undo_history(&self) {
        self.inner.release_undo_history();
    }

    // ---- commands and keys ----

    pub fn key_dispatch(&self, chord: &KeyChord, state: &FocusState) -> Option<String> {
        self.inner.dispatch_key(chord, state)
    }

    /// Why the command cannot run now (`Some`), or None when it can: **the one gate**, the answer
    /// for a menu row, a toolbar button, a key and a palette row alike. `Err` for an unknown id.
    pub fn command_why_not(&self, id: &str) -> AppResult<Option<String>> {
        self.inner.why_not_now(id)
    }

    /// Runs the command through the gate. A command that is the host's to carry out (a dialog, the
    /// clipboard, focus) is not run: `Ok` with `NeedsUi` says so.
    pub fn command_execute(&self, id: &str) -> AppResult<CommandOutcome> {
        self.inner.run_command(id)
    }

    pub fn command_run(&self, id: &str) -> AppResult<()> {
        self.inner.run_command(id).map(|_| ())
    }

    /// Carries out one of the commands the host hands back because it needs the selection and a
    /// program to start (`is_launch_command`): Run as administrator, Open in Terminal, Open in VS
    /// Code, a user's own command. Through the launcher seam, so the harness refuses and records.
    pub fn launch_command(&self, id: &str) -> AppResult<()> {
        self.inner.run_launch_command(id)
    }

    /// As `launch_command`, on `folder` rather than the selection (the folder tree's menu).
    pub fn launch_command_at(&self, id: &str, folder: &str) -> AppResult<()> {
        self.inner.run_launch_command_at(id, folder)
    }

    /// Opens what the command line (or a second launch's hand-off) named: a tab, a pane or the
    /// active tab per the request's mode, with a file highlighted when asked.
    pub fn open_request(&self, request: &bb_core::cli::CommandLineRequest) -> AppResult<()> {
        self.inner.open_request(request)
    }

    /// Puts a sentence on the active tab's status line, for something the host did on its own.
    pub fn post_status(&self, text: &str) {
        self.inner.set_active_status(text);
    }

    /// Asks the user a yes/no question through the confirm seam (the window's, or the harness's).
    pub fn ask(&self, message: &str, title: &str, confirm_label: &str) -> bool {
        self.inner.seams.confirm.ask(message, title, confirm_label)
    }

    // ---- settings ----

    /// One setting as JSON text (`null` for an unset optional). Keys are the settings file's.
    pub fn settings_get(&self, key: &str) -> AppResult<String> {
        let settings = lock(&self.inner.settings);
        let Value::Object(map) =
            serde_json::to_value(&*settings).map_err(|e| AppError::Invalid(e.to_string()))?
        else {
            return Err(AppError::Invalid("settings are not an object".into()));
        };
        match map.get(key) {
            Some(v) => Ok(v.to_string()),
            None => Err(AppError::Invalid(format!("unknown setting '{key}'"))),
        }
    }

    pub fn settings_set(&self, key: &str, json_value: &str) -> AppResult<()> {
        self.inner.settings_set(key, json_value)
    }

    // ---- theme ----

    /// One ARGB value per `ThemeToken`, in token order, for the theme the settings (and, when
    /// following Windows, the system appearance) select.
    pub fn theme_palette(&self) -> Vec<u32> {
        self.inner.theme_palette()
    }

    // ---- state ----

    /// The Rust half of the harness's `state` verb.
    pub fn state_json(&self) -> String {
        self.inner.state_json().to_string()
    }

    /// Blocks until the settle predicate has been false for `quiet_ms` straight, or `timeout_ms`
    /// passes. For tests and the harness; the UI never calls it.
    pub fn wait_idle(&self, quiet_ms: u64, timeout_ms: u64) -> bool {
        let start = std::time::Instant::now();
        let mut quiet_since: Option<std::time::Instant> = None;
        while start.elapsed().as_millis() < u128::from(timeout_ms) {
            if self.is_busy() {
                quiet_since = None;
            } else {
                let since = *quiet_since.get_or_insert_with(std::time::Instant::now);
                if since.elapsed().as_millis() >= u128::from(quiet_ms) {
                    return true;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }
}

impl Inner {
    fn layout_json(&self, shell: &crate::app::Shell, node: NodeId) -> Value {
        if shell.layout.is_leaf(node) {
            return json!({
                "kind": "pane",
                "pane": shell.layout.value(node).0,
                "weight": shell.layout.weight(node),
            });
        }
        let orientation = match shell.layout.orientation(node) {
            Some(SplitOrientation::Vertical) => "vertical",
            _ => "horizontal",
        };
        json!({
            "kind": "split",
            "orientation": orientation,
            "weight": shell.layout.weight(node),
            "children": shell.layout.children(node).iter().map(|c| self.layout_json(shell, *c)).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn state_json(&self) -> Value {
        let (draining, paused) = (lock(&self.queue.state).draining, self.gate.is_paused());
        let summary = lock(&self.undo_summary).clone();
        let progress = lock(&self.transfer_progress).clone();
        let shell = lock(&self.shell);
        let mut tab_ids: Vec<TabId> = shell.tabs.keys().copied().collect();
        tab_ids.sort();
        let tabs: Vec<Value> = tab_ids
            .iter()
            .filter_map(|id| shell.tabs.get(id))
            .map(|cell| {
                let t = lock(cell);
                json!({
                    "id": t.id.0,
                    "pane": t.pane.0,
                    "path": t.path,
                    "title": tab_title(&t.path),
                    "canBack": !t.back.is_empty(),
                    "canForward": !t.forward.is_empty(),
                    "canUp": !t.path.is_empty() && parent_path(&t.path).is_some(),
                    "count": t.listing.len(),
                    "selected": t.selection.len(),
                    "version": t.version,
                    "flattened": t.is_flattened,
                    "flatBrowse": t.is_flat_browse,
                    "flatView": t.is_flat_view,
                    "insideArchive": crate::commands::ArchivePathCheck::in_archive(&t.path),
                    "globalSearch": t.is_global_search(),
                    "search": t.active_search_text(),
                    "status": t.status,
                    "error": t.error,
                    "empty": t.empty_message,
                    "notice": t.notice,
                    "sortBy": t.sort_by,
                    "sortDescending": t.sort_desc,
                    "thumbnailScale": t.thumbnail_scale,
                    "sidePane": t.side_pane,
                    "searchRunning": t.is_search_running,
                    "columns": t.resolved_columns().iter().map(|c| c.spec.id.clone()).collect::<Vec<_>>(),
                    "columnsCustomized": t.columns_customized,
                    "previewMode": t.preview_mode,
                })
            })
            .collect();
        let mut pane_ids: Vec<&PaneId> = shell.panes.keys().collect();
        pane_ids.sort();
        let panes: Vec<Value> = pane_ids
            .iter()
            .map(|id| {
                let p = &shell.panes[*id];
                json!({
                    "id": id.0,
                    "tabs": p.tabs.iter().map(|t| t.0).collect::<Vec<_>>(),
                    "activeTab": p.active.map(|t| t.0),
                })
            })
            .collect();
        json!({
            "busy": self.busy(),
            "transferring": draining,
            "paused": paused,
            "loading": self.loading.load(std::sync::atomic::Ordering::SeqCst),
            "hydrating": self.hydrating.load(std::sync::atomic::Ordering::SeqCst),
            "canUndo": summary.undo.is_some() && !draining,
            "canRedo": summary.redo.is_some() && !draining,
            "undo": summary.undo,
            "redo": summary.redo,
            "undoEntries": summary.entries,
            "activePane": shell.active_pane.0,
            "activeTab": shell.active_tab_id().map(|t| t.0),
            "layout": self.layout_json(&shell, shell.layout.root()),
            "panes": panes,
            "tabs": tabs,
            "transfer": progress.map(|p| json!({
                "done": p.done, "total": p.total, "current": p.current_name, "bytes": p.bytes_done,
            })),
            "launches": self.probes.as_ref().map(|p| p.launches().len()).unwrap_or(0),
            "ui": self.ui_json(),
            "index": self.index_json(),
            "settingsProblem": self.settings_problem(),
            "bars": json!({ "name": self.bars_current_name(), "isDefault": self.bars_is_default() }),
            "theme": self.theme_summary_json(),
        })
    }
}
