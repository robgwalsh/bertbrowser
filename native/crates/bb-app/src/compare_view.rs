//! Two panes under comparison: the engine that fills in what `misc::CompareState` switches on.
//! Spec: ShellViewModel (CompareSession), CompareSessionViewModel (SettleByContentAsync).
//!
//! The comparison is a `CompareResult` keyed by relative path, so walking into `left\src` and
//! `right\src` needs no rescan: a row finds its verdict by its own path under the compared root.
//! Each row carries its state as a small atomic stamped when a listing is made (`make_listing`) or
//! the result changes, and the Status column reads it as words, since colour alone is not something
//! everyone can read.
//!
//! **A comparison's "same" is what authorises a delete**, so every doubt resolves away from it. The
//! only way a verdict moves toward "same" is `ContentSettlement`, which needs both files read and
//! identical byte for byte.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use bb_core::cancel::CancelToken;
use bb_core::compare::{
    CompareResult, CompareRowState, CompareRules, CompareSide, ContentComparer, ContentSettlement,
    ContentVerdict, FileContentComparer, FolderCompareService,
};
use bb_core::data::FsIndexRepository;

use crate::app::{Inner, lock};
use crate::events::{AppEvent, Model};
use crate::ids::{AppError, AppResult, PaneId, TabId};
use crate::rows::Row;

/// What one tab of a comparison knows: which side it is, where that side starts, and the verdicts.
pub(crate) struct TabCompare {
    pub side: CompareSide,
    /// `PathKey::canonicalize` of the compared folder.
    pub root_key: String,
    pub result: Arc<CompareResult>,
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
        match self.relative_key(&row.key) {
            Some(key) => match self.result.by_relative_key.get(key) {
                Some(verdict) => CompareRules::row_state(*verdict, self.side),
                None => CompareRowState::None,
            },
            None => CompareRowState::None,
        }
    }

    pub fn stamp(&self, rows: &[Arc<Row>]) {
        for row in rows {
            row.compare
                .store(state_number(self.state_of(row)), Ordering::Relaxed);
        }
    }
}

impl Inner {
    /// Reads both folders and stamps the verdicts onto the two tabs. Runs on a worker: the status
    /// line says so, and the settle predicate waits for it.
    pub(crate) fn compare_begin(self: &Arc<Self>, left: PaneId, right: PaneId) {
        let tabs = (self.pane_active_tab(left), self.pane_active_tab(right));
        let (Some(left_tab), Some(right_tab)) = tabs else {
            return;
        };
        let (Ok(l), Ok(r)) = (self.tab(left_tab), self.tab(right_tab)) else {
            return;
        };
        let (left_path, right_path) = (lock(&l).path.clone(), lock(&r).path.clone());
        let hidden = lock(&self.settings).show_hidden_items;
        self.set_status(left_tab, "Comparing\u{2026}");
        self.schedule(move |inner| {
            let service = FolderCompareService::new(
                Arc::new(FsIndexRepository::new(inner.db.clone())),
                inner.seams.index.clone(),
            );
            let outcome =
                service.compare(&left_path, &right_path, hidden, &CancelToken::new(), None);
            // Ended while it ran: nothing to show.
            if lock(&inner.extra.ui).compare.is_none() {
                return;
            }
            if let Some(problem) = &outcome.problem {
                inner.set_status(left_tab, problem.clone());
                return;
            }
            let result = Arc::new(outcome.result);
            {
                let mut ui = lock(&inner.extra.ui);
                if let Some(state) = ui.compare.as_mut() {
                    state.result = Some(result.clone());
                    state.left_root = left_path.clone();
                    state.right_root = right_path.clone();
                    state.left_tab = Some(left_tab);
                    state.right_tab = Some(right_tab);
                }
            }
            inner.compare_stamp(&result, left_tab, &left_path, right_tab, &right_path);
            let (same, different, unknown) = (
                result.same_count,
                result.difference_count,
                result.unknown_count,
            );
            inner.set_status(
                left_tab,
                format!("Compared: {same} the same, {different} different, {unknown} not compared"),
            );
        });
    }

    fn compare_stamp(
        &self,
        result: &Arc<CompareResult>,
        left_tab: TabId,
        left_path: &str,
        right_tab: TabId,
        right_path: &str,
    ) {
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
                let compare = Arc::new(TabCompare {
                    side,
                    root_key,
                    result: result.clone(),
                });
                t.compare = Some(compare);
                // A new snapshot, not a repaint of this one: a listing caches the text of a cell it has
                // handed out, so rows stamped in place would go on reading as they did.
                t.version += 1;
                let rows: Vec<Arc<Row>> = t.listing.rows().to_vec();
                t.listing = t.make_listing(rows, t.is_flattened, t.is_flat_browse);
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
                    for row in t.listing.rows() {
                        row.compare.store(0, Ordering::Relaxed);
                    }
                    t.version += 1;
                    let rows: Vec<Arc<Row>> = t.listing.rows().to_vec();
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
        Ok(identical)
    }

    fn pane_active_tab(&self, pane: PaneId) -> Option<TabId> {
        lock(&self.shell).panes.get(&pane).and_then(|p| p.active)
    }
}

fn join(root: &str, relative: &str) -> String {
    format!("{}\\{}", root.trim_end_matches('\\'), relative)
}
