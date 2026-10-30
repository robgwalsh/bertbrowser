//! Two panes under comparison: the engine that fills in what `misc::CompareState` switches on.
//! Spec: ShellViewModel (CompareSession), CompareSessionViewModel (SettleByContentAsync, Summary,
//! Caveat, the rules for when it ends), SyncPreviewViewModel.
//!
//! The comparison is a `CompareResult` keyed by relative path, so walking into `left\src` and
//! `right\src` needs no rescan: a row finds its verdict by its own path under the compared root.
//! Each row carries its state as a small atomic stamped when a listing is made (`make_listing`) or
//! the result changes, and the Status column reads it as words, since colour alone is not something
//! everyone can read.
//!
//! **A comparison's "same" is what authorises a delete**, so every doubt resolves away from it: the
//! only way a verdict moves toward "same" is `ContentSettlement`, which needs both files read and
//! identical byte for byte, and a row whose file is no longer the one that was compared reads as
//! unknown rather than keeping a verdict about something that stopped being true.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bb_core::cancel::CancelToken;
use bb_core::compare::{
    CompareAvailability, CompareRefusal, CompareResult, CompareRowState, CompareRules, CompareSide,
    CompareVerdict, ContentComparer, ContentSettlement, ContentVerdict, FileContentComparer,
    FileSystemCompareProbe, FolderCompareOutcome, FolderCompareService, SyncActionKind,
    SyncOutcome, SyncPlanner, SyncPlans, SyncPreview, SyncRunner,
};
use bb_core::data::FsIndexRepository;
use bb_core::delete::{DeleteMode, DeletePlanner};
use bb_core::format::byte_size;
use bb_core::transfer::{TransferExecutor, TransferPlanner};
use bb_core::undo::SyncRecord;
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::config::Fakes;
use crate::events::{AppEvent, Model};
use crate::ids::{AppError, AppResult, OpId, PaneId, TabId};
use crate::rows::Row;

/// What one tab of a comparison knows: which side it is, where that side starts, and the verdicts.
pub(crate) struct TabCompare {
    pub side: CompareSide,
    /// `PathKey::canonicalize` of the compared folder.
    pub root_key: String,
    pub result: Arc<CompareResult>,
    /// Hide the rows the comparison has nothing to say about, on both sides at once.
    pub differences_only: bool,
    /// Set when a row's file is found to have moved on since the scan.
    pub stale: Arc<AtomicBool>,
}

pub(crate) fn state_number(state: CompareRowState) -> u8 {
    match state {
        CompareRowState::None => 0,
        CompareRowState::Unknown => 1,
        CompareRowState::Same => 2,
        CompareRowState::OnlyHere => 3,
        CompareRowState::Newer => 4,
        CompareRowState::Older => 5,
        CompareRowState::Differs => 6,
    }
}

/// What the Status column says. Empty for a row the comparison has nothing to say about.
pub(crate) fn state_words(number: u8) -> &'static str {
    match number {
        1 => "Not compared",
        2 => "Same",
        3 => "Only here",
        4 => "Newer",
        5 => "Older",
        6 => "Differs",
        _ => "",
    }
}

impl TabCompare {
    /// The row's path relative to the compared root, in the key's own form.
    fn relative_key<'a>(&self, row_key: &'a str) -> Option<&'a str> {
        let rest = row_key.strip_prefix(self.root_key.as_str())?;
        let rest = if self.root_key.ends_with('\\') {
            rest
        } else {
            rest.strip_prefix('\\')?
        };
        (!rest.is_empty()).then_some(rest)
    }

    pub fn state_of(&self, row: &Row) -> CompareRowState {
        let Some(key) = self.relative_key(&row.key) else {
            return CompareRowState::None;
        };
        let entries = match self.side {
            CompareSide::Left => &self.result.left,
            CompareSide::Right => &self.result.right,
        };
        let Some(entry) = entries.get(key) else {
            return CompareRowState::Unknown;
        };
        // The row is describing a file the comparison did not see. Keeping the old verdict would be
        // stating something that stopped being true, so it becomes an unknown and the banner offers
        // a rescan. Only asked of files: a folder's size comes from a cache rather than from the
        // listing, and its own timestamp moves for reasons a comparison never weighed.
        if !row.is_directory
            && let Some(size) = row.size
            && (size != entry.size_bytes || row.modified != entry.modified_utc)
        {
            self.stale.store(true, Ordering::Relaxed);
            return CompareRowState::Unknown;
        }
        match self.result.by_relative_key.get(key) {
            Some(verdict) => CompareRules::row_state(*verdict, self.side),
            None => CompareRowState::None,
        }
    }

    pub fn stamp(&self, rows: &[Arc<Row>]) {
        for row in rows {
            row.compare
                .store(state_number(self.state_of(row)), Ordering::Relaxed);
        }
    }

    /// Whether the "differences only" filter keeps this (already stamped) row: not the same, and not
    /// one the comparison has nothing to say about.
    pub fn shows_when_filtered(&self, row: &Row) -> bool {
        !matches!(row.compare.load(Ordering::Relaxed), 0 | 2)
    }
}

fn caveat(outcome: &FolderCompareOutcome) -> Option<String> {
    match outcome.availability {
        CompareAvailability::Refused | CompareAvailability::Unreadable => outcome.problem.clone(),
        _ if outcome.truncated => Some(
            "These folders hold more entries than one comparison can take, so this is only part of them \u{2014} syncing is off."
                .into(),
        ),
        CompareAvailability::Building => {
            Some("A drive is still being indexed, so something may yet be missing.".into())
        }
        _ => None,
    }
}

/// Every count a comparison found, written out in words. Counted the way the sync plans, so the
/// banner and the preview cannot disagree: a folder the other side lacks is one thing however much
/// is inside it, and a folder both sides have is not a difference of its own.
fn summary(outcome: &FolderCompareOutcome) -> String {
    if let Some(problem) = outcome.problem.as_ref().filter(|p| !p.is_empty()) {
        return problem.clone();
    }
    let result = &outcome.result;
    if !result.any_difference() {
        return "These folders match.".into();
    }
    let mut parts = Vec::new();
    let mut add = |n: usize, text: &str| {
        if n > 0 {
            parts.push(format!("{n} {text}"));
        }
    };
    add(result.count(CompareVerdict::LeftOnly), "only on the left");
    add(result.count(CompareVerdict::RightOnly), "only on the right");
    add(result.count(CompareVerdict::LeftNewer), "newer on the left");
    add(
        result.count(CompareVerdict::RightNewer),
        "newer on the right",
    );
    add(result.count(CompareVerdict::Differs), "differ");
    // Said last and always, when there are any: an entry nothing is known about is the one the sync
    // will not touch, and it must not be discovered afterwards.
    add(
        result.count(CompareVerdict::Unknown),
        "could not be compared",
    );
    parts.join(" \u{b7} ")
}

fn join(root: &str, relative: &str) -> String {
    format!("{}\\{}", root.trim_end_matches('\\'), relative)
}

impl Inner {
    /// Says why a comparison was refused, out loud, and gives the error the caller returns. The
    /// reason goes through the notice seam rather than into a footer where it would not be read.
    pub(crate) fn compare_refused(&self, why: String) -> AppError {
        self.seams.notice.say(&why, "Compare");
        AppError::Invalid(why)
    }

    /// Starts comparing the active tabs of two panes.
    pub(crate) fn compare_start(self: &Arc<Self>, left: PaneId, right: PaneId) -> AppResult<()> {
        let tabs = (self.pane_active_tab(left), self.pane_active_tab(right));
        let (Some(left_tab), Some(right_tab)) = tabs else {
            return Err(AppError::Invalid(
                "Compare needs a folder open on each side.".into(),
            ));
        };
        let (l, r) = (self.tab(left_tab)?, self.tab(right_tab)?);
        let (left_root, right_root) = (lock(&l).path.clone(), lock(&r).path.clone());
        if let Some(why) = CompareRefusal::check(&left_root, &right_root, &FileSystemCompareProbe) {
            return Err(self.compare_refused(why));
        }
        lock(&self.extra.ui).compare = Some(crate::misc::CompareState {
            left,
            right,
            differences_only: false,
            result: None,
            left_root: left_root.clone(),
            right_root: right_root.clone(),
            left_tab: Some(left_tab),
            right_tab: Some(right_tab),
            outcome: None,
            message: None,
            comparing: true,
            stale: Arc::new(AtomicBool::new(false)),
        });
        self.hub.model(Model::Compare);
        self.compare_run();
        Ok(())
    }

    /// Reads both folders and stamps the verdicts onto the two tabs. Runs on a worker: the status
    /// line says so, and the settle predicate waits for it.
    pub(crate) fn compare_run(self: &Arc<Self>) {
        let (left_root, right_root, left_tab) = {
            let mut ui = lock(&self.extra.ui);
            let Some(state) = ui.compare.as_mut() else {
                return;
            };
            state.comparing = true;
            state.message = None;
            (
                state.left_root.clone(),
                state.right_root.clone(),
                state.left_tab,
            )
        };
        let hidden = lock(&self.settings).show_hidden_items;
        if let Some(tab) = left_tab {
            self.set_status(tab, "Comparing\u{2026}");
        }
        self.hub.model(Model::Compare);
        self.schedule(move |inner| {
            let service = FolderCompareService::new(
                Arc::new(FsIndexRepository::new(inner.db.clone())),
                inner.seams.index.clone(),
            );
            let outcome =
                service.compare(&left_root, &right_root, hidden, &CancelToken::new(), None);
            inner.compare_apply(outcome);
        });
    }

    fn compare_apply(self: &Arc<Self>, outcome: FolderCompareOutcome) {
        // Ended while it ran: nothing to show.
        if lock(&self.extra.ui).compare.is_none() {
            return;
        }
        // A pair the service refused never becomes a session: an End button is a poor way to
        // deliver an error message.
        if outcome.availability == CompareAvailability::Refused {
            let why = outcome
                .problem
                .clone()
                .unwrap_or_else(|| "These folders cannot be compared.".into());
            self.compare_end();
            self.seams.notice.say(&why, "Compare");
            return;
        }
        let result = Arc::new(outcome.result.clone());
        let summary_text;
        let (left_tab, right_tab, left_root, right_root);
        {
            let mut ui = lock(&self.extra.ui);
            let Some(state) = ui.compare.as_mut() else {
                return;
            };
            state.message = caveat(&outcome);
            summary_text = summary(&outcome);
            state.outcome = Some(Arc::new(outcome));
            state.result = Some(result.clone());
            state.comparing = false;
            state.stale.store(false, Ordering::Relaxed);
            (left_tab, right_tab) = (state.left_tab, state.right_tab);
            (left_root, right_root) = (state.left_root.clone(), state.right_root.clone());
        }
        if let (Some(lt), Some(rt)) = (left_tab, right_tab) {
            self.compare_stamp(&result, lt, &left_root, rt, &right_root);
            self.set_status(lt, summary_text);
        }
        self.hub.model(Model::Compare);
    }

    pub(crate) fn compare_stamp(
        &self,
        result: &Arc<CompareResult>,
        left_tab: TabId,
        left_path: &str,
        right_tab: TabId,
        right_path: &str,
    ) {
        let (differences_only, stale) = {
            let ui = lock(&self.extra.ui);
            ui.compare
                .as_ref()
                .map(|c| (c.differences_only, c.stale.clone()))
                .unwrap_or_else(|| (false, Arc::new(AtomicBool::new(false))))
        };
        for (tab, side, path) in [
            (left_tab, CompareSide::Left, left_path),
            (right_tab, CompareSide::Right, right_path),
        ] {
            let Ok(cell) = self.tab(tab) else { continue };
            let Ok(root_key) = bb_paths::PathKey::canonicalize(path) else {
                continue;
            };
            let version = {
                let mut t = lock(&cell);
                t.compare = Some(Arc::new(TabCompare {
                    side,
                    root_key,
                    result: result.clone(),
                    differences_only,
                    stale: stale.clone(),
                }));
                // A new snapshot, not a repaint of this one: a listing caches the text of a cell it
                // has handed out, so rows stamped in place would go on reading as they did.
                t.version += 1;
                let rows: Vec<Arc<Row>> = t.listing.source_rows().to_vec();
                t.listing = t.make_listing(rows, t.is_flattened, t.is_flat_browse);
                let present: HashSet<u64> = t.listing.rows().iter().map(|r| r.id).collect();
                t.selection.retain(|id| present.contains(id));
                t.version
            };
            self.hub.emit(AppEvent::ListingChanged { tab, version });
            self.hub.emit(AppEvent::TabChanged { tab });
        }
        self.hub.model(Model::Columns);
    }

    /// Takes the verdicts off every tab and puts the columns back.
    pub(crate) fn compare_clear(&self) {
        for (id, cell) in self.all_tabs() {
            let had = {
                let mut t = lock(&cell);
                let had = t.compare.take().is_some();
                if had {
                    for row in t.listing.source_rows() {
                        row.compare.store(0, Ordering::Relaxed);
                    }
                    t.version += 1;
                    let rows: Vec<Arc<Row>> = t.listing.source_rows().to_vec();
                    t.listing = t.make_listing(rows, t.is_flattened, t.is_flat_browse);
                }
                had.then_some(t.version)
            };
            if let Some(version) = had {
                self.hub.emit(AppEvent::ListingChanged { tab: id, version });
                self.hub.emit(AppEvent::TabChanged { tab: id });
            }
        }
        self.hub.model(Model::Columns);
    }

    /// Ends the comparison and says why on the left tab's status line.
    pub(crate) fn compare_end_because(&self, reason: &str) {
        let left = lock(&self.extra.ui)
            .compare
            .as_ref()
            .and_then(|c| c.left_tab);
        self.compare_end();
        if let Some(tab) = left {
            self.set_status(tab, reason);
        }
    }

    /// A tab that stayed inside the folder it compared keeps the session; one that left ends it.
    /// Staying is what makes walking into `left\src` and `right\src` work with no rescan. Leaving
    /// has to end it, because silently re-rooting the comparison onto wherever the tab went would
    /// leave a Sync button pointing at a pairing nobody asked for.
    pub(crate) fn compare_tab_moved(&self, tab: TabId, path: &str) {
        let (root, ended) = {
            let ui = lock(&self.extra.ui);
            let Some(state) = ui.compare.as_ref() else {
                return;
            };
            let root = if state.left_tab == Some(tab) {
                state.left_root.clone()
            } else if state.right_tab == Some(tab) {
                state.right_root.clone()
            } else {
                return;
            };
            let inside = match (
                bb_paths::PathKey::canonicalize(path),
                bb_paths::PathKey::canonicalize(&root),
            ) {
                (Ok(p), Ok(r)) => p == r || bb_paths::PathKey::is_under_key(&p, &r),
                _ => false,
            };
            (root, !inside)
        };
        if ended {
            self.compare_end_because(&format!("Compare ended \u{2014} a pane left {root}."));
        }
    }

    /// A flattened list is rows from all over the tree, which is not a folder listing and cannot be
    /// paired against one. Only the wording branches: telling someone their comparison ended
    /// because "a pane started a search" when they pressed the flat-view key is a small lie.
    pub(crate) fn compare_tab_flattened(&self, tab: TabId, flat_browse: bool) {
        let involved = lock(&self.extra.ui)
            .compare
            .as_ref()
            .is_some_and(|c| c.left_tab == Some(tab) || c.right_tab == Some(tab));
        if involved {
            self.compare_end_because(if flat_browse {
                "Compare ended \u{2014} a pane switched to flat view."
            } else {
                "Compare ended \u{2014} a pane started a search."
            });
        }
    }

    pub(crate) fn compare_tab_closed(&self, tab: TabId) {
        let involved = lock(&self.extra.ui)
            .compare
            .as_ref()
            .is_some_and(|c| c.left_tab == Some(tab) || c.right_tab == Some(tab));
        if involved {
            self.compare_end();
        }
    }

    /// The comparison's folders changed on disk through this app. A sync reads them again
    /// (`rescan`); anything else marks the comparison stale, so the banner offers Rescan rather
    /// than the app re-reading two trees after every rename. Spec: CompareSessionViewModel (IsStale).
    pub(crate) fn compare_directories_changed(
        self: &Arc<Self>,
        keys: &HashSet<String>,
        rescan: bool,
    ) {
        let (left, right) = {
            let ui = lock(&self.extra.ui);
            let Some(state) = ui.compare.as_ref() else {
                return;
            };
            (state.left_root.clone(), state.right_root.clone())
        };
        let inside = |root: &str| {
            let Ok(root_key) = bb_paths::PathKey::canonicalize(root) else {
                return false;
            };
            keys.iter()
                .any(|k| *k == root_key || bb_paths::PathKey::is_under_key(k, &root_key))
        };
        if !(inside(&left) || inside(&right)) {
            return;
        }
        if rescan {
            self.compare_scan_now();
            return;
        }
        if let Some(state) = lock(&self.extra.ui).compare.as_ref() {
            state.stale.store(true, Ordering::Relaxed);
        }
        self.hub.model(Model::Compare);
    }

    /// Runs a scan on this thread and applies it. For a caller already off the UI thread.
    fn compare_scan_now(self: &Arc<Self>) {
        let (left_root, right_root) = {
            let ui = lock(&self.extra.ui);
            let Some(state) = ui.compare.as_ref() else {
                return;
            };
            (state.left_root.clone(), state.right_root.clone())
        };
        let hidden = lock(&self.settings).show_hidden_items;
        let service = FolderCompareService::new(
            Arc::new(FsIndexRepository::new(self.db.clone())),
            self.seams.index.clone(),
        );
        let outcome = service.compare(&left_root, &right_root, hidden, &CancelToken::new(), None);
        self.compare_apply(outcome);
    }

    /// "Rescan" on the banner.
    pub(crate) fn compare_rescan(self: &Arc<Self>) -> AppResult<()> {
        if lock(&self.extra.ui).compare.is_none() {
            return Err(AppError::Invalid("Nothing is being compared.".into()));
        }
        self.compare_run();
        Ok(())
    }

    pub(crate) fn compare_set_differences_only(&self, on: bool) -> AppResult<()> {
        let (result, left_tab, right_tab, left_root, right_root) = {
            let mut ui = lock(&self.extra.ui);
            let state = ui
                .compare
                .as_mut()
                .ok_or_else(|| AppError::Invalid("Nothing is being compared.".into()))?;
            state.differences_only = on;
            (
                state.result.clone(),
                state.left_tab,
                state.right_tab,
                state.left_root.clone(),
                state.right_root.clone(),
            )
        };
        if let (Some(result), Some(lt), Some(rt)) = (result, left_tab, right_tab) {
            self.compare_stamp(&result, lt, &left_root, rt, &right_root);
        }
        self.hub.model(Model::Compare);
        Ok(())
    }

    /// `{active, comparing, hasResult, summary, message, canSync, differencesOnly, stale, left,
    /// right}`: everything the banner shows.
    pub(crate) fn compare_model(&self) -> Value {
        let ui = lock(&self.extra.ui);
        let Some(state) = ui.compare.as_ref() else {
            return json!({"active": false});
        };
        let outcome = state.outcome.as_ref();
        let has_result = outcome.is_some_and(|o| o.availability != CompareAvailability::Refused);
        json!({
            "active": true,
            "comparing": state.comparing,
            "hasResult": has_result,
            "summary": outcome.map(|o| summary(o)).unwrap_or_default(),
            "message": state.message,
            "canSync": !state.comparing && outcome.is_some_and(|o| o.can_sync()),
            "differencesOnly": state.differences_only,
            "stale": state.stale.load(Ordering::Relaxed),
            "left": state.left_root,
            "right": state.right_root,
        })
    }

    /// Reads both sides of each selected pair and, where the bytes are identical, settles the
    /// verdict to "same". One direction only, earned, and never for a folder. Returns how many pairs
    /// were identical. Blocking: call it from a worker.
    pub(crate) fn compare_settle(&self, paths: &[String]) -> AppResult<usize> {
        let (result, left_root, right_root, left_tab, right_tab) = {
            let ui = lock(&self.extra.ui);
            let state = ui.compare.as_ref().ok_or_else(|| {
                AppError::Invalid("Nothing is being compared. Run 'compare' first.".into())
            })?;
            let result = state
                .result
                .clone()
                .ok_or_else(|| AppError::Invalid("The comparison has no result yet.".into()))?;
            (
                result,
                state.left_root.clone(),
                state.right_root.clone(),
                state.left_tab,
                state.right_tab,
            )
        };
        let (Some(left_tab), Some(right_tab)) = (left_tab, right_tab) else {
            return Ok(0);
        };
        let left_key = bb_paths::PathKey::canonicalize(&left_root).unwrap_or_default();
        let right_key = bb_paths::PathKey::canonicalize(&right_root).unwrap_or_default();

        // A row is only comparable when both sides hold it: anything else has nothing to read, and the
        // comparison already says so more precisely than bytes could.
        let mut pairs: Vec<(String, String, String)> = Vec::new();
        for path in paths {
            let Ok(key) = bb_paths::PathKey::canonicalize(path) else {
                continue;
            };
            let relative = [&left_key, &right_key].into_iter().find_map(|root| {
                let prefix = if root.ends_with('\\') {
                    root.to_string()
                } else {
                    format!("{root}\\")
                };
                key.strip_prefix(prefix.as_str()).map(str::to_string)
            });
            let Some(relative) = relative else { continue };
            let (Some(l), Some(r)) = (result.left.get(&relative), result.right.get(&relative))
            else {
                continue;
            };
            if l.is_directory || r.is_directory {
                continue;
            }
            pairs.push((
                relative.clone(),
                join(
                    &left_root,
                    &result.display_path(&relative, CompareSide::Left),
                ),
                join(
                    &right_root,
                    &result.display_path(&relative, CompareSide::Right),
                ),
            ));
        }
        if pairs.is_empty() {
            return Ok(0);
        }

        let comparer = FileContentComparer;
        let cancel = CancelToken::new();
        let mut verdicts: HashMap<String, ContentVerdict> = HashMap::new();
        for (key, left, right) in &pairs {
            if let Ok(found) = comparer.compare(left, right, None, &cancel) {
                verdicts.insert(key.clone(), found.verdict);
            }
        }
        let identical = verdicts
            .values()
            .filter(|v| **v == ContentVerdict::Identical)
            .count();

        let settled = Arc::new(ContentSettlement::apply(&result, &verdicts).into_owned());
        if let Some(state) = lock(&self.extra.ui).compare.as_mut() {
            state.result = Some(settled.clone());
        }
        // Re-assigning the stamps is what re-paints every row; the rows themselves remember nothing.
        self.compare_stamp(&settled, left_tab, &left_root, right_tab, &right_root);
        self.hub.model(Model::Compare);
        Ok(identical)
    }

    fn pane_active_tab(&self, pane: PaneId) -> Option<TabId> {
        lock(&self.shell).panes.get(&pane).and_then(|p| p.active)
    }

    // ---- syncing ----

    /// The folder service's answer with whatever settling has since made of its verdicts: what a sync
    /// is planned from, because `SyncPlanner` reads verdicts straight off the result.
    fn compare_outcome(&self) -> AppResult<FolderCompareOutcome> {
        let ui = lock(&self.extra.ui);
        let state = ui.compare.as_ref().ok_or_else(|| {
            AppError::Invalid("Nothing is being compared. Run 'compare' first.".into())
        })?;
        let (Some(outcome), Some(result)) = (&state.outcome, &state.result) else {
            return Err(AppError::Invalid(
                "The comparison has no result to sync.".into(),
            ));
        };
        Ok(FolderCompareOutcome {
            result: (**result).clone(),
            ..(**outcome).clone()
        })
    }

    fn sync_preview(
        &self,
        with_deletes: bool,
        ticked: Option<&HashSet<String>>,
    ) -> AppResult<SyncPreview> {
        let outcome = self.compare_outcome()?;
        let preview = SyncPlanner::preview(&outcome, with_deletes);
        Ok(match ticked {
            Some(keys) => SyncPlanner::with_ticks(&preview, keys),
            None => preview,
        })
    }

    /// The sync dialog's rows: `{left, right, withDeletes, unknownCount, caveat, summary, canRun,
    /// actions:[{key, kind, name, size, why, ticked, isDirectory}]}`. `ticked` is the keys the person
    /// has left ticked; None means every action as the planner ticked it.
    pub(crate) fn sync_preview_json(
        &self,
        with_deletes: bool,
        ticked: Option<&HashSet<String>>,
    ) -> AppResult<Value> {
        let preview = self.sync_preview(with_deletes, ticked)?;
        let mut actions: Vec<_> = preview.actions.iter().collect();
        let order = |k: SyncActionKind| match k {
            SyncActionKind::Copy => 0,
            SyncActionKind::Overwrite => 1,
            SyncActionKind::Delete => 2,
        };
        actions.sort_by(|a, b| {
            order(a.kind).cmp(&order(b.kind)).then_with(|| {
                a.relative_display
                    .to_lowercase()
                    .cmp(&b.relative_display.to_lowercase())
            })
        });
        let mut parts = Vec::new();
        for (n, word) in [
            (preview.copy_count(), "copy"),
            (preview.overwrite_count(), "replace"),
            (preview.delete_count(), "delete"),
        ] {
            if n > 0 {
                parts.push(format!("{word} {}", bb_core::format::thousands(n as i64)));
            }
        }
        let mut summary = if parts.is_empty() {
            "Nothing selected".to_string()
        } else {
            parts.join(" \u{b7} ")
        };
        // Withheld rather than approximated the moment one contributing size is unknown.
        if let Some(total) = preview.total_bytes().filter(|t| *t > 0) {
            summary.push_str(&format!(" \u{2014} {}", byte_size(total)));
        }
        Ok(json!({
            "left": preview.left_path,
            "right": preview.right_path,
            "withDeletes": with_deletes,
            "unknownCount": preview.unknown_count,
            "caveat": (preview.unknown_count > 0).then(|| format!(
                "{} item(s) could not be compared and will be left alone.",
                bb_core::format::thousands(preview.unknown_count as i64))),
            "summary": summary,
            "canRun": preview.has_work(),
            "actions": actions.iter().map(|a| json!({
                "key": a.relative_key,
                "kind": match a.kind {
                    SyncActionKind::Copy => "Copy",
                    SyncActionKind::Overwrite => "Replace",
                    SyncActionKind::Delete => "Delete",
                },
                "name": a.relative_display,
                "size": a.bytes.map(byte_size).unwrap_or_default(),
                "why": match a.verdict {
                    CompareVerdict::LeftOnly => if a.is_directory { "New folder" } else { "New file" },
                    CompareVerdict::LeftNewer => "The left side is newer",
                    CompareVerdict::RightNewer => "The right side is newer",
                    CompareVerdict::RightOnly => "Only on the right",
                    _ => "The two differ",
                },
                "ticked": a.ticked,
                "isDirectory": a.is_directory,
            })).collect::<Vec<_>>(),
        }))
    }

    /// Makes the right side match the left, through the same planner, runner and undo history the
    /// dialog's Sync button goes through. `with_deletes` ticks the destructive half, which the dialog
    /// leaves off; `ticked` narrows to the keys the person left ticked.
    pub(crate) fn compare_sync(
        self: &Arc<Self>,
        with_deletes: bool,
        ticked: Option<HashSet<String>>,
    ) -> AppResult<OpId> {
        {
            let ui = lock(&self.extra.ui);
            if ui.compare.as_ref().is_some_and(|c| c.comparing) {
                return Err(AppError::Invalid("The comparison is still running.".into()));
            }
        }
        let preview = self.sync_preview(with_deletes, ticked.as_ref())?;
        if !preview.has_work() {
            return Err(AppError::Invalid(
                "There is nothing for a sync to do.".into(),
            ));
        }
        self.submit(
            "Sync",
            false,
            Box::new(move |inner, _op, cancel| inner.run_sync(&preview, cancel)),
        )
    }

    fn run_sync(self: &Arc<Self>, preview: &SyncPreview, cancel: &CancelToken) -> (bool, String) {
        let transfer_planner = TransferPlanner::default();
        let delete_planner =
            DeletePlanner::new(&crate::ops::DELETE_PROBE).recycle_probe(&*crate::ops::RECYCLE_BIN);
        let plans = SyncPlanner::to_plans(
            preview,
            &transfer_planner,
            &delete_planner,
            // A run under the fakes (tests, the harness) holds what it removes in the staging folder:
            // the real Recycle Bin is the user's, and a test must never put anything in it.
            if self.config.fakes == Fakes::Harness {
                DeleteMode::Staged
            } else {
                DeleteMode::Recycle
            },
        );
        if !plans.has_work() {
            let why = plans.refused.first().cloned().unwrap_or_default();
            let text = format!("Nothing to sync. {why}").trim().to_string();
            self.set_active_status(text.clone());
            return (false, text);
        }
        self.set_active_status(format!(
            "Syncing {} item(s)\u{2026}",
            bb_core::format::thousands(plans.item_count() as i64)
        ));
        let me = self.clone();
        let mut on_progress = move |p: bb_core::transfer::TransferProgress| {
            *lock(&me.transfer_progress) = Some(p);
        };
        let transfers = TransferExecutor::default();
        let deletes = self.delete_executor();
        let outcome =
            SyncRunner::new(&transfers, &deletes).run(&plans, cancel, Some(&mut on_progress));

        self.record(SyncRecord::for_outcome(outcome.clone()).map(|r| Box::new(r) as _));

        let mut directories: Vec<String> = plans
            .copies
            .iter()
            .map(|c| c.destination_directory.clone())
            .chain(plans.removals.deletions.iter().map(|d| d.parent_path()))
            .collect();
        directories.sort_by_key(|d| d.to_uppercase());
        directories.dedup_by_key(|d| d.to_uppercase());
        self.refresh_tabs_showing(&directories);
        let keys: HashSet<String> = directories
            .iter()
            .filter_map(|d| bb_paths::PathKey::canonicalize(d).ok())
            .collect();
        self.compare_directories_changed(&keys, true);

        let failed = outcome.failed_copies().len() + outcome.removals.failed.len();
        let text = self.describe_sync(&outcome, &plans);
        // The folders it wrote into have been told to reload; the sentence waits for them.
        self.set_final_status(text.clone());
        (failed == 0 && !outcome.cancelled, text)
    }

    /// What the status bar says afterwards. Each half is named separately: "synced 20 items" hides
    /// the only number anyone checks twice, which is how many were taken away.
    /// Spec: ShellViewModel.DescribeSync.
    fn describe_sync(&self, outcome: &SyncOutcome, plans: &SyncPlans) -> String {
        let mut parts = Vec::new();
        for (word, n) in [
            ("copied", outcome.copied_count()),
            ("replaced", outcome.replaced_count()),
            ("removed", outcome.removed_count()),
        ] {
            if n > 0 {
                parts.push(format!("{word} {}", bb_core::format::thousands(n as i64)));
            }
        }
        if parts.is_empty() {
            parts.push("nothing to do".to_string());
        }
        let mut text = format!(
            "{} \u{2014} {}",
            if outcome.cancelled {
                "Sync cancelled"
            } else {
                "Synced"
            },
            parts.join(", ")
        );
        let failed_copies = outcome.failed_copies();
        let failed = failed_copies.len() + outcome.removals.failed.len();
        if failed > 0 {
            let first = failed_copies
                .first()
                .map(|f| f.message.clone())
                .or_else(|| outcome.removals.failed.first().map(|f| f.message.clone()))
                .unwrap_or_default();
            text.push_str(&format!(
                ", {} failed \u{2014} {first}",
                bb_core::format::thousands(failed as i64)
            ));
        } else if let Some(first) = plans.refused.first() {
            text.push_str(&format!(
                ", {} refused \u{2014} {first}",
                bb_core::format::thousands(plans.refused.len() as i64)
            ));
        } else if outcome.can_undo() {
            text.push_str(&self.undo_hint());
        }
        text
    }
}
