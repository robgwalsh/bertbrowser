//! The public surface of the side panes: the animated GIF a preview plays, the metadata pane of a
//! tab, and the host's window onto the Windows property system. Everything here is a thin front
//! for `preview`, `gif_player` and `metadata_pane`.

use std::sync::Arc;

use bb_core::rename::RenameRule;
use serde_json::Value;

use crate::app::{App, lock};
use crate::gif_player::{GifOpenError, GifPlayer};
use crate::ids::{AppError, AppResult, OpId, TabId};
use crate::metadata_pane::{MetadataAction, ShellPropertyHost};

impl App {
    // ---- animated GIFs ----

    /// Starts playing a GIF: returns its handle once frame 0 is ready, the rest decode behind it.
    /// Blocking (it reads the file), so call it from a worker.
    pub fn gif_open(&self, path: &str) -> Result<(u64, Arc<GifPlayer>), GifOpenError> {
        let player = GifPlayer::open(path)?;
        let handle = self.inner.new_id();
        lock(&self.inner.extra.gifs).insert(handle, player.clone());
        Ok((handle, player))
    }

    pub fn gif(&self, handle: u64) -> AppResult<Arc<GifPlayer>> {
        lock(&self.inner.extra.gifs)
            .get(&handle)
            .cloned()
            .ok_or(AppError::Stale)
    }

    /// Stops the decoding and lets the frames go.
    pub fn gif_close(&self, handle: u64) {
        if let Some(player) = lock(&self.inner.extra.gifs).remove(&handle) {
            player.close();
        }
    }

    // ---- the metadata pane ----

    /// Hands over the Windows property system (or takes it away). The metadata pane reads a file
    /// no codec handles, and the dimensions under a picture, through it.
    pub fn set_shell_property_host(&self, host: Option<Arc<dyn ShellPropertyHost>>) {
        self.inner.extra.metadata.set_host(host);
    }

    /// Points the pane at the tab's selection and reads it; a no-op when neither the paths nor
    /// their modified times moved, unless `force`.
    pub fn metadata_load(&self, tab: TabId, force: bool) -> AppResult<()> {
        self.inner.metadata_load(tab, force)
    }

    pub fn metadata_model(&self, tab: TabId) -> AppResult<Value> {
        self.inner.metadata_model(tab)
    }

    pub fn metadata_picture(&self, tab: TabId) -> Option<Vec<u8>> {
        self.inner.metadata_picture(tab)
    }

    /// Types into a box. Only a change of text counts as touching it.
    pub fn metadata_set_text(&self, tab: TabId, id: &str, text: &str) -> AppResult<()> {
        self.inner.metadata_set_text(tab, id, text)
    }

    pub fn metadata_revert(&self, tab: TabId, id: &str) -> AppResult<()> {
        self.inner.metadata_revert(tab, id)
    }

    /// A day chosen in a date box's calendar, keeping the time already there.
    pub fn metadata_pick_date(
        &self,
        tab: TabId,
        id: &str,
        year: i32,
        month: u32,
        day: u32,
    ) -> AppResult<()> {
        self.inner.metadata_pick_date(tab, id, year, month, day)
    }

    /// Presses one of the pane's buttons: through the queue and into the undo history. `Err` says
    /// why the button would not have worked.
    pub fn metadata_act(&self, tab: TabId, action: MetadataAction) -> AppResult<OpId> {
        self.inner.metadata_act(tab, action)
    }

    // ---- comparing two panes ----

    /// Settles the verdict of each path (that both sides hold, as a file) by reading both and comparing
    /// the bytes; identical ones become "same", nothing else moves. Returns how many were identical.
    /// Blocking.
    /// As `compare_settle`, on a worker: the files are read off the window's thread, and the rows
    /// re-paint through `Model::Compare` when it is done. Spec: CompareSessionViewModel.SettleByContentAsync.
    pub fn compare_settle_start(&self, paths: Vec<String>) {
        self.inner.schedule(move |inner| {
            let _ = inner.compare_settle(&paths);
        });
    }

    pub fn compare_settle(&self, paths: &[String]) -> AppResult<usize> {
        self.inner.compare_settle(paths)
    }

    // ---- renaming by a rule ----

    pub fn op_rename_rule(&self, paths: Vec<String>, rule: RenameRule) -> AppResult<OpId> {
        self.inner.op_rename_rule(paths, rule)
    }
}

impl App {
    // ---- the media playlist ----

    /// The row after the selected one that `MediaPlaylist` says plays next (a video, wrapping
    /// never), or None.
    pub fn preview_next_media(&self, tab: TabId) -> AppResult<Option<u32>> {
        let cell = self.inner.tab(tab)?;
        let t = lock(&cell);
        let rows = t.listing.rows();
        let Some(current) = rows.iter().position(|r| t.selection.contains(&r.id)) else {
            return Ok(None);
        };
        let names: Vec<Option<&str>> = rows
            .iter()
            .map(|r| (!r.is_directory).then_some(r.name.as_str()))
            .collect();
        Ok(bb_core::preview::MediaPlaylist::next(&names, current).map(|i| i as u32))
    }
}

impl App {
    /// What the file holds now, one field by id (or `location` / `picture` as "yes" or ""), read from
    /// disk through the codec that writes it.
    pub fn metadata_peek(&self, path: &str, what: &str) -> Result<String, String> {
        self.inner.metadata_peek(path, what)
    }
}
