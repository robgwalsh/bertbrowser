//! A tab: history, breadcrumb, the listing and the jobs that fill it, search, flat view, the watcher
//! merge and selection. Spec: DirectoryTabViewModel, FileListViewModel.
//!
//! Every job is a closure on the worker pool that re-checks the tab's `generation` before it publishes, so
//! a navigation that was superseded while it ran changes nothing.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::time::Duration;

use bb_core::archives::ArchiveBrowser;
use bb_core::attributes::HIDDEN;
use bb_core::cancel::CancelToken;
use bb_core::data::SearchHit;
use bb_core::format::{byte_size, thousands};
use bb_core::fs::{FileListDiff, FileSystemService, SearchProgress, SearchService};
use bb_core::models::{ContentScanProgress, SearchOutcome};
use bb_core::rules::FlatViewRules;
use bb_core::search::SearchQuery;
use bb_paths::PathKey;

use crate::app::{Inner, TabCell, TabData, lock};
use crate::browse::dirwatch::DirWatcher;
use crate::browse::rows::Row;
use crate::browse::sort::SortSpec;
use crate::events::AppEvent;
use crate::ids::{AppError, AppResult, TabId};

/// How a gesture changes the selection. Indices are positions in the listing the caller holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectOp {
    Replace,
    Add,
    Toggle,
    Remove,
    All,
    Clear,
    Invert,
}

/// A segment of the breadcrumb.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crumb {
    pub name: String,
    pub full_path: String,
}

/// The cap on rows a flat view lists.
pub const FLAT_ENTRY_CAP: usize = SearchService::MAX_FLAT_ENTRIES;

/// The parent of a path, or None at a root.
pub fn parent_path(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() || trimmed.len() == 2 && trimmed.ends_with(':') {
        return None;
    }
    let cut = trimmed.rfind(['\\', '/'])?;
    let parent = &trimmed[..cut];
    if parent.len() == 2 && parent.ends_with(':') {
        return Some(format!("{parent}\\"));
    }
    if parent.is_empty() || parent == "\\" {
        return None;
    }
    Some(parent.to_string())
}

/// `C:\a\b` as C:, a, b with each segment's full path. A root keeps its trailing separator.
pub fn breadcrumb(path: &str) -> Vec<Crumb> {
    let mut out = Vec::new();
    if path.is_empty() {
        return out;
    }
    let root_len = bb_paths::root_len(path);
    let root = &path[..root_len];
    if root.is_empty() {
        return out;
    }
    out.push(Crumb {
        name: root.trim_end_matches(['\\', '/']).to_string(),
        full_path: root.to_string(),
    });
    let mut acc = root.trim_end_matches(['\\', '/']).to_string();
    for part in path[root_len..]
        .split(['\\', '/'])
        .filter(|p| !p.is_empty())
    {
        acc = format!("{acc}\\{part}");
        out.push(Crumb {
            name: part.to_string(),
            full_path: acc.clone(),
        });
    }
    out
}

/// A tab's title: the folder's name, or the drive.
pub fn tab_title(path: &str) -> String {
    if path.is_empty() {
        return "New tab".to_string();
    }
    let trimmed = path.trim_end_matches(['\\', '/']);
    let name = bb_paths::file_name(trimmed);
    if name.is_empty() {
        trimmed.to_string()
    } else {
        name.to_string()
    }
}

fn access_denied_message(path: &str) -> String {
    format!(
        "Access is denied. \"{path}\" needs administrator rights, which BertBrowser does not use \u{2014} the same folders are closed to File Explorer."
    )
}

fn intern(t: &mut TabData, key: &Arc<str>) -> u64 {
    if let Some(&id) = t.interner.get(key) {
        return id;
    }
    t.next_row += 1;
    let id = t.next_row;
    t.interner.insert(key.clone(), id);
    id
}

impl TabData {
    pub(crate) fn sort_spec(&self) -> SortSpec {
        SortSpec::new(
            &self.sort_by,
            self.sort_desc,
            self.is_flattened,
            self.thumbnail_scale > 0.0,
        )
    }

    /// Replaces the listing with `rows`, giving each a stable id. Selection survives only where
    /// asked, and only for rows that are still there.
    pub(crate) fn install(&mut self, mut rows: Vec<Row>, keep_selection: bool) {
        self.is_loading = false;
        for row in rows.iter_mut() {
            row.id = intern(self, &row.key);
            // What the host already read for this file survives the reload, unless the file has been
            // written since: a title edited in place must not go on reading as it did.
            if self
                .prop_stamps
                .get(&row.key)
                .is_some_and(|stamp| *stamp != (row.modified.0, row.size.unwrap_or(-1)))
            {
                self.props.remove(&row.key);
                self.prop_stamps.remove(&row.key);
            }
            if row.props.is_none()
                && let Some(known) = self.props.get(&row.key)
            {
                row.props = Some(Arc::new(known.clone()));
            }
        }
        if keep_selection {
            let present: std::collections::HashSet<u64> = rows.iter().map(|r| r.id).collect();
            self.selection.retain(|id| present.contains(id));
        } else {
            self.selection.clear();
        }
        self.version += 1;
        self.listing = self.make_listing(
            rows.into_iter().map(Arc::new).collect(),
            self.is_flattened,
            self.is_flat_browse,
        );
        self.apply_pending_selection();
    }

    pub(crate) fn reset_view_flags(&mut self) {
        self.is_flattened = false;
        self.is_flat_browse = false;
        self.shows_content_matches = false;
        self.error = None;
        self.empty_message = None;
        self.notice = None;
    }

    pub(crate) fn clear_search_state(&mut self) {
        self.search_gen += 1;
        self.search_text.clear();
        self.global_search_text.clear();
    }
}

fn parse_has_search(text: &str) -> bool {
    let parse = SearchQuery::parse(text);
    parse.query.is_some() || parse.problem.is_some()
}

impl Inner {
    fn include_hidden(&self) -> bool {
        lock(&self.settings).show_hidden_items
    }

    // ---- navigation ----

    pub(crate) fn navigate(self: &Arc<Self>, tab: TabId, path: &str) -> AppResult<()> {
        let cell = self.tab(tab)?;
        {
            let t = lock(&cell);
            if path.eq_ignore_ascii_case(&t.path) {
                return Ok(());
            }
        }
        // A real folder, or somewhere inside a container file that is there. The container itself is
        // not opened here (it may be large, or on a share that has gone away): the load reads it off
        // this thread, and a folder it does not hold is the banner's to say. Spec:
        // DirectoryTabViewModel.NavigateToAsync.
        let container = || {
            self.fs
                .resolve(path)
                .is_some_and(|a| std::path::Path::new(&a.archive_file).is_file())
        };
        if !std::path::Path::new(path).is_dir() && !container() {
            self.set_status(tab, format!("Folder not found: {path}"));
            return Err(AppError::Invalid(format!("Folder not found: {path}")));
        }
        {
            let mut t = lock(&cell);
            if !t.path.is_empty() {
                let current = t.path.clone();
                t.back.push(current);
            }
            t.forward.clear();
        }
        self.set_path_and_load(&cell, path.to_string());
        Ok(())
    }

    pub(crate) fn back(self: &Arc<Self>, tab: TabId) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let target = {
            let mut t = lock(&cell);
            let Some(target) = t.back.pop() else {
                return Err(AppError::Invalid("nothing to go back to".into()));
            };
            let current = std::mem::take(&mut t.path);
            t.forward.push(current);
            target
        };
        self.set_path_and_load(&cell, target);
        Ok(())
    }

    pub(crate) fn forward(self: &Arc<Self>, tab: TabId) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let target = {
            let mut t = lock(&cell);
            let Some(target) = t.forward.pop() else {
                return Err(AppError::Invalid("nothing to go forward to".into()));
            };
            let current = std::mem::take(&mut t.path);
            t.back.push(current);
            target
        };
        self.set_path_and_load(&cell, target);
        Ok(())
    }

    pub(crate) fn up(self: &Arc<Self>, tab: TabId) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let path = lock(&cell).path.clone();
        match parent_path(&path) {
            Some(parent) => self.navigate(tab, &parent),
            None => Err(AppError::Invalid("already at the top".into())),
        }
    }

    pub(crate) fn refresh(self: &Arc<Self>, tab: TabId) -> AppResult<()> {
        self.tab(tab)?;
        self.refresh_view(tab);
        Ok(())
    }

    fn set_path_and_load(self: &Arc<Self>, cell: &TabCell, path: String) {
        let id;
        {
            let mut t = lock(cell);
            id = t.id;
            t.clear_search_state();
            t.path = path.clone();
            t.interner.clear();
            t.selection.clear();
            t.props.clear();
            t.props_inflight.clear();
            // Restore this folder's tile/list preference.
            let key = PathKey::canonicalize(&path).unwrap_or_default();
            t.thumbnail_scale = lock(&self.settings)
                .directory_thumbnail_scales
                .get(&key)
                .copied()
                .unwrap_or(0.0);
            t.watcher = None;
        }
        let watcher = self.start_watching(id, &path);
        lock(cell).watcher = watcher;
        self.compare_tab_moved(id, &path);
        self.hub.emit(AppEvent::TabChanged { tab: id });
        self.refresh_view(id);
    }

    fn start_watching(self: &Arc<Self>, tab: TabId, path: &str) -> Option<DirWatcher> {
        let weak: Weak<Inner> = Arc::downgrade(self);
        let on_quiet: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner.schedule(move |inner| inner.merge_directory(tab));
            }
        });
        DirWatcher::start(
            path,
            Duration::from_millis(self.config.watch_debounce_ms),
            on_quiet,
        )
    }

    // ---- refresh: the one entry point every reload goes through ----

    pub(crate) fn refresh_view(self: &Arc<Self>, tab: TabId) {
        let Ok(cell) = self.tab(tab) else { return };
        let (generation, token, kind) = {
            let mut t = lock(&cell);
            if t.path.is_empty() {
                return;
            }
            t.generation += 1;
            t.is_loading = true;
            t.cancel.cancel();
            t.cancel = CancelToken::new();
            t.search_stop.cancel();
            t.search_stop = CancelToken::new();
            let kind = if parse_has_search(t.active_search_text()) {
                Kind::Search
            } else if t.is_flat_view {
                Kind::Flat
            } else {
                Kind::Directory
            };
            (t.generation, t.cancel.clone(), kind)
        };
        match kind {
            Kind::Directory => {
                self.schedule(move |inner| inner.load_directory(tab, generation, token))
            }
            Kind::Search => self.schedule(move |inner| inner.run_search(tab, generation, token)),
            Kind::Flat => self.schedule(move |inner| inner.run_flat(tab, generation, token)),
        }
        self.hub.emit(AppEvent::TabChanged { tab });
        self.update_busy();
    }

    fn load_directory(self: &Arc<Self>, tab: TabId, generation: u64, token: CancelToken) {
        let Ok(cell) = self.tab(tab) else { return };
        let include_hidden = self.include_hidden();
        let (path, spec) = {
            let mut t = lock(&cell);
            if t.generation != generation {
                return;
            }
            t.reset_view_flags();
            (t.path.clone(), t.sort_spec())
        };

        let listed = self.fs.list_directory(&path);
        if token.is_cancelled() {
            return;
        }
        match listed {
            Ok(entries) => {
                let mut rows: Vec<Row> = entries
                    .iter()
                    .filter(|e| include_hidden || e.attributes & HIDDEN == 0)
                    .map(Row::from_entry)
                    .collect();
                spec.sort(&mut rows);
                self.hydrate_dir_sizes(&mut rows);
                if token.is_cancelled() {
                    return;
                }
                let mut t = lock(&cell);
                if t.generation != generation {
                    return;
                }
                let count = rows.len();
                t.install(rows, false);
                t.archive_locked = false;
                t.status = format!("{count} item(s)");
                let version = t.version;
                drop(t);
                bb_shell::startup_trace::global()
                    .mark(bb_core::benchmarking::StartupMarks::LISTING_LANDED);
                self.hub.emit(AppEvent::ListingChanged { tab, version });
                self.hub.emit(AppEvent::TabChanged { tab });
                self.props_after_install(tab);
            }
            Err(e) => {
                // A container that wants a password is not a folder Windows closed: say so, and the
                // host offers to ask for it.
                let locked = bb_core::archives::ArchiveLockedError::in_error(&e).is_some();
                if locked && let Some(archive) = self.archive_file_for(&path) {
                    self.extra.ops.passwords.refused(&archive);
                }
                let message =
                    if let Some(locked) = bb_core::archives::ArchiveLockedError::in_error(&e) {
                        locked.message.clone()
                    } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                        access_denied_message(&path)
                    } else {
                        e.to_string()
                    };
                let mut t = lock(&cell);
                if t.generation != generation {
                    return;
                }
                t.install(Vec::new(), false);
                t.error = Some(message);
                t.archive_locked = locked;
                let version = t.version;
                drop(t);
                self.hub.emit(AppEvent::ListingChanged { tab, version });
                self.hub.emit(AppEvent::TabChanged { tab });
            }
        }
    }

    /// A volume's MFT index just finished: an active whole-PC search runs again against it, and a
    /// plain folder picks up the sizes `dir_size_cache` now has. Spec: DirectoryTabViewModel.OnMftIndexRefreshed.
    pub(crate) fn mft_index_refreshed(self: &Arc<Self>) {
        for (id, cell) in self.all_tabs() {
            let (search, global, flattened) = {
                let t = lock(&cell);
                (
                    parse_has_search(t.active_search_text()),
                    t.is_global_search(),
                    t.is_flattened,
                )
            };
            if search {
                if global {
                    self.refresh_view(id);
                }
                continue;
            }
            if flattened {
                continue;
            }
            let mut t = lock(&cell);
            let mut rows: Vec<Row> = t.listing.rows().iter().map(|r| clone_row(r)).collect();
            for r in rows.iter_mut().filter(|r| r.is_directory) {
                r.size = None;
            }
            self.hydrate_dir_sizes(&mut rows);
            t.install(rows, true);
            let version = t.version;
            drop(t);
            self.hub.emit(AppEvent::ListingChanged { tab: id, version });
        }
    }

    /// A background (re)crawl of `root_key` finished: a search in a tab at or under it runs again.
    /// Spec: DirectoryTabViewModel.OnIndexRefreshed.
    pub(crate) fn folder_index_refreshed(self: &Arc<Self>, root_key: &str) {
        for (id, cell) in self.all_tabs() {
            let under = {
                let t = lock(&cell);
                !t.path.is_empty()
                    && t.index_rerun != t.generation
                    && parse_has_search(t.active_search_text())
                    && !t.is_global_search()
                    && PathKey::canonicalize(&t.path)
                        .is_ok_and(|k| k == root_key || PathKey::is_under_key(&k, root_key))
            };
            if under {
                self.refresh_view(id);
                let mut t = lock(&cell);
                t.index_rerun = t.generation;
            }
        }
    }

    /// Folder sizes come from `dir_size_cache` and nowhere else; a missing row stays blank.
    fn hydrate_dir_sizes(&self, rows: &mut [Row]) {
        let dirs: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.is_directory && r.size.is_none())
            .map(|(i, _)| i)
            .collect();
        if dirs.is_empty() {
            return;
        }
        let paths: Vec<&str> = dirs.iter().map(|&i| rows[i].full_path.as_str()).collect();
        let Ok(cache) = self.dir_sizes.get_many(&paths) else {
            return;
        };
        for i in dirs {
            if let Some(hit) = cache.get(&*rows[i].key) {
                rows[i].size = Some(hit.size_bytes);
                rows[i].size_incomplete = hit.incomplete;
                rows[i].size_computed = hit.computed_utc;
            }
        }
    }

    // ---- the watcher merge ----

    fn merge_directory(self: &Arc<Self>, tab: TabId) {
        let Ok(cell) = self.tab(tab) else { return };
        let include_hidden = self.include_hidden();
        let (path, generation, listing) = {
            let t = lock(&cell);
            // A search result is not a folder listing, and an errored list has nothing to merge into.
            // The listing's own flag as well as the tab's: a load clears the tab's before it has
            // replaced the rows, and diffing a search result against a folder would "add" the folder.
            if t.is_flattened || t.listing.is_flattened || t.error.is_some() || t.path.is_empty() {
                return;
            }
            (t.path.clone(), t.generation, t.listing.clone())
        };
        let Ok(listed) = self.fs.list_directory(&path) else {
            return;
        };
        let entries: Vec<_> = listed
            .into_iter()
            .filter(|e| include_hidden || e.attributes & HIDDEN == 0)
            .collect();
        let current: Vec<_> = listing.source_rows().iter().map(|r| r.to_entry()).collect();
        let Ok(changes) = FileListDiff::compute(&current, &entries) else {
            return;
        };
        if !changes.any() {
            return;
        }

        let mut added: Vec<Row> = changes.added.iter().map(Row::from_entry).collect();
        let mut updated: Vec<Row> = changes.updated.iter().map(Row::from_entry).collect();
        // Only folders that arrived need a size; an updated folder keeps the one it had.
        self.hydrate_dir_sizes(&mut added);
        let removed: std::collections::HashSet<&str> =
            changes.removed.iter().map(String::as_str).collect();

        let mut t = lock(&cell);
        // Dropped if anything replaced the rows while the diff was computed: it was a diff of
        // rows that are gone, and applying it to the new ones would duplicate entries.
        if t.generation != generation || t.is_flattened || t.listing.version != listing.version {
            return;
        }
        let spec = t.sort_spec();
        let mut rows: Vec<Arc<Row>> = Vec::with_capacity(t.listing.len() + added.len());
        let mut replacements: std::collections::HashMap<Arc<str>, Row> =
            updated.drain(..).map(|r| (r.key.clone(), r)).collect();
        let mut stale: Vec<Arc<str>> = Vec::new();
        for existing in t.listing.source_rows() {
            if removed.contains(&*existing.key) {
                continue;
            }
            if let Some(mut replacement) = replacements.remove(&existing.key) {
                if replacement.modified != existing.modified || replacement.size != existing.size {
                    stale.push(existing.key.clone());
                }
                if replacement.is_directory {
                    replacement.size = existing.size;
                    replacement.size_incomplete = existing.size_incomplete;
                    replacement.size_computed = existing.size_computed;
                }
                replacement.id = existing.id;
                rows.push(Arc::new(replacement));
            } else {
                rows.push(existing.clone());
            }
        }
        for key in stale {
            t.props.remove(&key);
            t.prop_stamps.remove(&key);
        }
        for mut row in added {
            row.id = intern(&mut t, &row.key);
            let at = rows.partition_point(|r| spec.compare(&row, r) != std::cmp::Ordering::Less);
            rows.insert(at, Arc::new(row));
        }
        let present: std::collections::HashSet<u64> = rows.iter().map(|r| r.id).collect();
        t.selection.retain(|id| present.contains(id));
        t.version += 1;
        let count = rows.len();
        t.listing = t.make_listing(rows, false, false);
        t.apply_pending_selection();
        t.empty_message = None;
        t.status = format!("{count} item(s)");
        let version = t.version;
        drop(t);
        self.hub.emit(AppEvent::ListingChanged { tab, version });
        self.hub.emit(AppEvent::TabChanged { tab });
        self.props_after_install(tab);
    }

    // ---- search ----

    pub(crate) fn set_search(
        self: &Arc<Self>,
        tab: TabId,
        text: &str,
        global: bool,
    ) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let search_gen = {
            let mut t = lock(&cell);
            if global {
                t.global_search_text = text.to_string();
                if !text.is_empty() {
                    t.search_text.clear();
                }
            } else {
                t.search_text = text.to_string();
                if !text.is_empty() {
                    t.global_search_text.clear();
                }
            }
            t.search_gen += 1;
            t.search_gen
        };
        let delay = self.config.search_debounce_ms;
        if delay == 0 {
            self.refresh_view(tab);
            return Ok(());
        }
        // The pending wait counts as loading, so a settle cannot slip between typing and searching.
        self.loading.fetch_add(1, Ordering::SeqCst);
        let inner = self.clone();
        let spawned = std::thread::Builder::new()
            .name("bb-search-debounce".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(delay));
                let current = inner
                    .tab(tab)
                    .map(|c| lock(&c).search_gen == search_gen)
                    .unwrap_or(false);
                if current {
                    inner.refresh_view(tab);
                }
                inner.loading.fetch_sub(1, Ordering::SeqCst);
                inner.update_busy();
            });
        if spawned.is_err() {
            self.loading.fetch_sub(1, Ordering::SeqCst);
            self.refresh_view(tab);
        }
        Ok(())
    }

    pub(crate) fn clear_search(self: &Arc<Self>, tab: TabId) -> AppResult<()> {
        let cell = self.tab(tab)?;
        lock(&cell).clear_search_state();
        self.refresh_view(tab);
        Ok(())
    }

    pub(crate) fn set_flat_view(self: &Arc<Self>, tab: TabId, on: bool) -> AppResult<()> {
        let cell = self.tab(tab)?;
        {
            let mut t = lock(&cell);
            if t.is_flat_view == on {
                return Ok(());
            }
            t.is_flat_view = on;
        }
        self.hub.emit(AppEvent::TabChanged { tab });
        self.refresh_view(tab);
        Ok(())
    }

    /// Starts a flattened listing: the rows are about to arrive from many folders.
    fn begin_flattened(t: &mut TabData, flat_browse: bool) {
        t.reset_view_flags();
        t.is_flattened = true;
        t.is_flat_browse = flat_browse;
        t.install(Vec::new(), false);
        // Still loading while the rows stream in; the final install lowers it. Spec:
        // FileListViewModel.BeginFlattened (IsLoading).
        t.is_loading = true;
    }

    /// A streamed run that ended without its final install (refused, failed): the activity bar stops.
    fn stop_loading(&self, tab: TabId, generation: u64) {
        let Ok(cell) = self.tab(tab) else { return };
        {
            let mut t = lock(&cell);
            if t.generation != generation {
                return;
            }
            t.is_loading = false;
            t.is_search_running = false;
        }
        self.hub.emit(AppEvent::TabChanged { tab });
    }

    /// Appends a streamed batch, unsorted: the final sort replaces it when the run ends.
    fn append_hits(&self, tab: TabId, generation: u64, hits: &[SearchHit], status: Option<String>) {
        let Ok(cell) = self.tab(tab) else { return };
        let mut t = lock(&cell);
        if t.generation != generation {
            return;
        }
        let mut rows: Vec<Row> = t.listing.rows().iter().map(|r| clone_row(r)).collect();
        rows.extend(hits.iter().map(Row::from_hit));
        t.install(rows, true);
        t.is_loading = true;
        if let Some(status) = status {
            // `{N}` is the count with thousands separators (the flat view's), `{n}` without.
            let n = t.listing.len();
            t.status = status
                .replace("{N}", &thousands(n as i64))
                .replace("{n}", &n.to_string());
        }
        let version = t.version;
        drop(t);
        self.hub.emit(AppEvent::ListingChanged { tab, version });
        self.hub.emit(AppEvent::TabChanged { tab });
    }

    fn run_search(self: &Arc<Self>, tab: TabId, generation: u64, token: CancelToken) {
        let Ok(cell) = self.tab(tab) else { return };
        let (text, global, path, stop) = {
            let t = lock(&cell);
            if t.generation != generation {
                return;
            }
            (
                t.active_search_text().to_string(),
                t.is_global_search(),
                t.path.clone(),
                t.search_stop.clone(),
            )
        };
        let parse = SearchQuery::parse(&text);
        if let Some(problem) = parse.problem {
            let mut t = lock(&cell);
            if t.generation == generation {
                // Refused before anything ran, so nothing is loading. Spec:
                // DirectoryTabViewModel.RunSearchAsync (Problem returns before BeginSearch).
                t.is_loading = false;
                t.error = Some(problem.clone());
                t.status = problem;
            }
            drop(t);
            self.hub.emit(AppEvent::TabChanged { tab });
            return;
        }
        let Some(query) = parse.query else {
            self.stop_loading(tab, generation);
            return;
        };
        let needs_content = query.needs_content();
        // Inside a container the answer comes from the index its listing was read from: instant, and
        // nowhere near fs_entry, which must never learn a virtual path.
        let archive_here = if global { None } else { self.fs.resolve(&path) };
        if needs_content && archive_here.is_some() {
            // Refused rather than answered wrongly: an entry has no file on disk, so its candidate
            // carries no content, and an unread candidate counts as a possible match by design. Run the
            // walk anyway and every entry in the container comes back as a hit.
            // Spec: DirectoryTabViewModel.RunSearchAsync (the archive branch).
            let message =
                "content: can't look inside archives \u{2014} extract it first.".to_string();
            let mut t = lock(&cell);
            if t.generation == generation {
                Self::begin_flattened(&mut t, false);
                t.is_loading = false;
                t.error = Some(message.clone());
                t.status = message;
                t.is_search_running = false;
                let version = t.version;
                drop(t);
                self.hub.emit(AppEvent::ListingChanged { tab, version });
                self.hub.emit(AppEvent::TabChanged { tab });
            }
            return;
        }
        {
            let mut t = lock(&cell);
            if t.generation != generation {
                return;
            }
            Self::begin_flattened(&mut t, false);
            t.shows_content_matches = needs_content;
            t.is_search_running = needs_content;
            t.status = if global {
                format!("Searching this PC for '{text}'\u{2026}")
            } else {
                format!("Searching for '{text}'\u{2026}")
            };
            let version = t.version;
            drop(t);
            self.compare_tab_flattened(tab, false);
            self.hub.emit(AppEvent::ListingChanged { tab, version });
        }

        let stream_status = (!needs_content && !global)
            .then(|| format!("{{n}} result(s) so far for '{text}'\u{2026}"));
        // What matched streams onto the list either way (the service keeps a content query's walk, which
        // is only candidates, to itself); the reading pass owns the status line once it reports, or the
        // two channels would fight over one row of text.
        let on_batch = |hits: &[SearchHit]| {
            if !token.is_cancelled() {
                self.append_hits(tab, generation, hits, stream_status.clone());
            }
        };
        // How much of the reading pass is left: during the tens of seconds a whole-tree grep can take,
        // the only thing worth saying. Spec: DirectoryTabViewModel.RunSearchAsync (contentProgress).
        let on_content = |p: ContentScanProgress| {
            if token.is_cancelled() {
                return;
            }
            let Ok(cell) = self.tab(tab) else { return };
            let mut t = lock(&cell);
            if t.generation != generation {
                return;
            }
            t.status = if p.files_to_read > 0 {
                format!(
                    "Reading {} of {} file(s) for '{text}' \u{2014} {} result(s) so far \u{2014} Esc to stop",
                    thousands(p.files_read as i64),
                    thousands(p.files_to_read as i64),
                    t.listing.len()
                )
            } else {
                format!("Searching for '{text}'\u{2026}")
            };
            drop(t);
            self.hub.emit(AppEvent::TabChanged { tab });
        };
        let progress = SearchProgress {
            live_batches: Some(&on_batch),
            content_progress: needs_content
                .then_some(&on_content as &(dyn Fn(ContentScanProgress) + Sync)),
        };
        let result = if global {
            self.search.search_all(&text, false, progress, &stop)
        } else if let Some(a) = archive_here {
            let index = self.fs.read_archive(&a.archive_file);
            // Spec: DirectoryTabViewModel.ArchiveSearchLimit (1,000, and "showing first" past it).
            const ARCHIVE_SEARCH_LIMIT: usize = 1_000;
            match bb_core::archives::ArchiveSearchScanner::search(
                &index,
                &a.archive_file,
                &a.entry_path,
                Some(&query),
                ARCHIVE_SEARCH_LIMIT,
                &stop,
                true,
            ) {
                Ok(hits) => {
                    let truncated = hits.len() >= ARCHIVE_SEARCH_LIMIT;
                    Ok(Some(SearchOutcome::new(
                        hits,
                        truncated,
                        bb_core::models::SearchResultSource::Index,
                        false,
                    )))
                }
                Err(_) => Ok(None),
            }
        } else {
            self.search.search(&path, &text, false, progress, &stop)
        };
        // The navigation token, not the stop token: typing more supersedes this run and its results
        // belong to a query no longer on screen. A Stop leaves that token alone, so the floor the
        // pass returned is kept and shown.
        let outcome = match result {
            Ok(Some(outcome)) => outcome,
            _ => {
                self.stop_loading(tab, generation);
                return;
            }
        };
        if token.is_cancelled() {
            return;
        }

        let mut rows: Vec<Row> = outcome.hits.iter().map(Row::from_hit).collect();
        if global {
            for row in rows.iter_mut() {
                hydrate_search_metadata(row);
            }
        }
        let spec = lock(&cell).sort_spec();
        spec.sort(&mut rows);
        self.hydrate_dir_sizes(&mut rows);

        let mut t = lock(&cell);
        if t.generation != generation {
            return;
        }
        t.is_search_running = false;
        let count = rows.len();
        t.install(rows, false);
        t.empty_message = (count == 0).then(|| format!("No results for '{text}'"));
        let scope = if global { "this PC".to_string() } else { path };
        let truncated = if outcome.truncated {
            " (showing first 1,000)"
        } else {
            ""
        };
        t.status = format!(
            "{count} result(s) for '{text}' in {scope}{truncated}{}",
            search_suffix(
                &outcome,
                global,
                self.seams.index.any_indexed() || self.seams.index.is_building()
            )
        );
        let version = t.version;
        drop(t);
        self.hub.emit(AppEvent::ListingChanged { tab, version });
        self.hub.emit(AppEvent::TabChanged { tab });
        self.props_after_install(tab);
    }

    fn run_flat(self: &Arc<Self>, tab: TabId, generation: u64, token: CancelToken) {
        let Ok(cell) = self.tab(tab) else { return };
        let (path, include_hidden) = {
            let t = lock(&cell);
            if t.generation != generation {
                return;
            }
            (t.path.clone(), self.include_hidden())
        };

        let estimate = self.dir_sizes.get(&path).ok().flatten();
        let cap = self
            .extra
            .flat_cap
            .load(std::sync::atomic::Ordering::Relaxed);
        let preflight = FlatViewRules::decide(&path, estimate.as_ref(), cap as i64);
        if preflight.confirm
            && !self
                .seams
                .confirm
                .ask(&preflight.message, "Flat view", &preflight.confirm_label)
        {
            // Declining leaves the listing, and the selection, exactly where they were. Spec:
            // DirectoryTabViewModel (the flat-view prompt declined).
            let mut t = lock(&cell);
            if t.generation == generation {
                t.is_flat_view = false;
                t.is_loading = false;
            }
            drop(t);
            self.hub.emit(AppEvent::TabChanged { tab });
            return;
        }

        let folder = match bb_paths::file_name(path.trim_end_matches(['\\', '/'])) {
            "" => path.clone(),
            name => name.to_string(),
        };
        {
            let mut t = lock(&cell);
            if t.generation != generation {
                return;
            }
            Self::begin_flattened(&mut t, true);
            t.status = format!("Listing everything under {folder}\u{2026}");
            let version = t.version;
            drop(t);
            self.compare_tab_flattened(tab, true);
            self.hub.emit(AppEvent::ListingChanged { tab, version });
        }

        let stream_status = format!("{{N}} file(s) so far under {folder}\u{2026}");
        let on_batch = |hits: &[SearchHit]| {
            if !token.is_cancelled() {
                self.append_hits(tab, generation, hits, Some(stream_status.clone()));
            }
        };
        let progress = SearchProgress {
            live_batches: Some(&on_batch),
            content_progress: None,
        };
        let outcome: SearchOutcome = if let Some(a) = self.fs.resolve(&path) {
            // A container lists itself flat from the index its listing was read from.
            let index = self.fs.read_archive(&a.archive_file);
            match bb_core::archives::ArchiveSearchScanner::search(
                &index,
                &a.archive_file,
                &a.entry_path,
                None,
                cap,
                &token,
                false,
            ) {
                Ok(hits) => {
                    let truncated = hits.len() >= cap;
                    SearchOutcome::new(
                        hits,
                        truncated,
                        bb_core::models::SearchResultSource::Index,
                        false,
                    )
                }
                Err(_) => {
                    self.stop_loading(tab, generation);
                    return;
                }
            }
        } else {
            match self
                .search
                .list_subtree(&path, cap, include_hidden, progress, &token)
            {
                Ok(outcome) => outcome,
                Err(_) => {
                    self.stop_loading(tab, generation);
                    return;
                }
            }
        };
        if token.is_cancelled() {
            return;
        }

        let mut t = lock(&cell);
        if t.generation != generation {
            return;
        }
        if let Some(problem) = outcome.problem {
            t.is_loading = false;
            t.error = Some(problem.clone());
            t.status = problem;
            drop(t);
            self.hub.emit(AppEvent::TabChanged { tab });
            return;
        }
        let mut rows: Vec<Row> = outcome.hits.iter().map(Row::from_hit).collect();
        t.sort_spec().sort(&mut rows);
        self.hydrate_dir_sizes(&mut rows);
        let count = rows.len();
        t.install(rows, false);
        t.empty_message = (count == 0).then(|| format!("Nothing under \"{folder}\"."));
        t.notice = outcome.truncated.then(|| {
            format!(
                "Showing the first {} of a larger tree. Open a folder further in, or search to narrow it.",
                thousands(cap as i64)
            )
        });
        let truncated = if outcome.truncated {
            format!(" (first {})", thousands(cap as i64))
        } else {
            String::new()
        };
        t.status = format!(
            "{} file(s) under {}{truncated}",
            thousands(count as i64),
            path
        );
        let version = t.version;
        drop(t);
        self.hub.emit(AppEvent::ListingChanged { tab, version });
        self.hub.emit(AppEvent::TabChanged { tab });
        self.props_after_install(tab);
    }

    /// Ends a running search, keeping what it has found so far.
    pub(crate) fn stop_search(&self, tab: TabId) -> AppResult<()> {
        let cell = self.tab(tab)?;
        {
            let mut t = lock(&cell);
            t.search_stop.cancel();
            t.is_search_running = false;
            t.status = format!("{} \u{2014} stopped", t.status.trim_end_matches('\u{2026}'));
        }
        self.hub.emit(AppEvent::TabChanged { tab });
        self.update_busy();
        Ok(())
    }

    /// Marks `created` to be selected in every tab showing `directory` once its listing arrives.
    pub(crate) fn select_after_refresh(&self, directory: &str, created: &str) {
        let Ok(key) = PathKey::canonicalize(directory) else {
            return;
        };
        for (_, cell) in self.all_tabs() {
            let mut t = lock(&cell);
            if PathKey::canonicalize(&t.path).is_ok_and(|k| k == key) {
                t.pending_selection = vec![created.to_string()];
            }
        }
    }

    /// Marks several paths to be selected in every tab showing `directory` once its listing arrives.
    pub(crate) fn select_many_after_refresh(&self, directory: &str, paths: Vec<String>) {
        let Ok(key) = PathKey::canonicalize(directory) else {
            return;
        };
        for (_, cell) in self.all_tabs() {
            let mut t = lock(&cell);
            if PathKey::canonicalize(&t.path).is_ok_and(|k| k == key) {
                t.pending_selection = paths.clone();
            }
        }
    }

    /// Moves any tab sitting in (or below) a renamed folder over to its new path: otherwise it points at
    /// a name that no longer exists, which a refresh can only turn into an error.
    /// Spec: ShellViewModel.FollowRenamedFoldersAsync.
    pub(crate) fn follow_renamed_folders(
        self: &Arc<Self>,
        completed: &[bb_core::rename::CompletedRename],
    ) {
        for rename in completed.iter().filter(|r| r.is_directory) {
            let Ok(from) = PathKey::canonicalize(&rename.source_path) else {
                continue;
            };
            for (tab, cell) in self.all_tabs() {
                let path = lock(&cell).path.clone();
                let Ok(key) = PathKey::canonicalize(&path) else {
                    continue;
                };
                // Canonicalizing only changes case, so the old path's length indexes the tab's own.
                let next = if key == from {
                    Some(rename.final_path.clone())
                } else if key.starts_with(&from) && key[from.len()..].starts_with('\\') {
                    Some(format!(
                        "{}{}",
                        rename.final_path.trim_end_matches('\\'),
                        &path[from.len()..]
                    ))
                } else {
                    None
                };
                if let Some(next) = next {
                    let _ = self.navigate(tab, &next);
                }
            }
        }
    }

    /// Goes to the folder a file is in and selects it: "show me where this is".
    pub(crate) fn reveal_file(self: &Arc<Self>, tab: TabId, file: &str) -> AppResult<()> {
        let Some(parent) = parent_path(file) else {
            return Ok(());
        };
        {
            let cell = self.tab(tab)?;
            let mut t = lock(&cell);
            t.clear_search_state();
            t.is_flat_view = false;
            t.pending_selection = vec![file.to_string()];
        }
        let tab_cell = self.tab(tab)?;
        let current = lock(&tab_cell).path.clone();
        if current.eq_ignore_ascii_case(&parent) {
            self.refresh_view(tab);
            Ok(())
        } else {
            self.navigate(tab, &parent)
        }
    }

    // ---- sort, view options ----

    pub(crate) fn set_sort(&self, tab: TabId, column: &str) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let mut t = lock(&cell);
        let id = bb_core::columns::ColumnCatalog::sort_spec(Some(column)).id;
        if t.sort_by.eq_ignore_ascii_case(&id) {
            t.sort_desc = !t.sort_desc;
        } else {
            t.sort_by = id;
            t.sort_desc = false;
        }
        resort(&mut t);
        let version = t.version;
        drop(t);
        self.hub.emit(AppEvent::ListingChanged { tab, version });
        self.hub.emit(AppEvent::TabChanged { tab });
        Ok(())
    }

    pub(crate) fn set_thumbnail_scale(&self, tab: TabId, scale: f64) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let mut t = lock(&cell);
        let scale = scale.clamp(0.0, 1.0);
        let crossed = (scale > 0.0) != (t.thumbnail_scale > 0.0);
        t.thumbnail_scale = scale;
        if !t.path.is_empty()
            && let Ok(key) = PathKey::canonicalize(&t.path)
        {
            let mut settings = lock(&self.settings);
            if scale > 0.0 {
                settings.directory_thumbnail_scales.insert(key, scale);
            } else {
                settings.directory_thumbnail_scales.remove(&key);
            }
        }
        // Crossing the details/thumbnail boundary changes the bands (media moves to the bottom).
        let version = if crossed {
            resort(&mut t);
            Some(t.version)
        } else {
            None
        };
        drop(t);
        if let Some(version) = version {
            self.hub.emit(AppEvent::ListingChanged { tab, version });
        }
        self.hub.emit(AppEvent::TabChanged { tab });
        Ok(())
    }

    pub(crate) fn set_side_pane(&self, tab: TabId, pane: u8) -> AppResult<()> {
        let cell = self.tab(tab)?;
        lock(&cell).side_pane = pane.min(2);
        {
            let mut settings = lock(&self.settings);
            settings.show_preview_pane = pane == 1;
            settings.show_metadata_pane = pane == 2;
        }
        self.hub.emit(AppEvent::TabChanged { tab });
        Ok(())
    }

    // ---- selection ----

    pub(crate) fn select(&self, tab: TabId, op: SelectOp, indices: &[u32]) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let mut t = lock(&cell);
        let listing = t.listing.clone();
        let ids = indices
            .iter()
            .filter_map(|&i| listing.row(i as usize).map(|r| r.id));
        match op {
            SelectOp::Replace => {
                t.selection.clear();
                t.selection.extend(ids);
            }
            SelectOp::Add => t.selection.extend(ids),
            SelectOp::Remove => {
                for id in ids {
                    t.selection.remove(&id);
                }
            }
            SelectOp::Toggle => {
                for id in ids {
                    if !t.selection.remove(&id) {
                        t.selection.insert(id);
                    }
                }
            }
            SelectOp::All => t.selection = listing.rows().iter().map(|r| r.id).collect(),
            SelectOp::Clear => t.selection.clear(),
            SelectOp::Invert => {
                t.selection = listing
                    .rows()
                    .iter()
                    .map(|r| r.id)
                    .filter(|id| !t.selection.contains(id))
                    .collect();
            }
        }
        // Each newly previewed file starts fitted to the pane; actual size is a choice made per file.
        // Spec: PreviewPaneViewModel (FitImageToPane reset on every load).
        t.preview_fit = false;
        drop(t);
        self.hub.emit(AppEvent::TabChanged { tab });
        Ok(())
    }

    /// "3 items selected (1.2 MB)" for the status bar, or `None` with nothing selected. A folder's size
    /// counts only when it is already known, so the total never waits on a scan.
    /// Spec: DirectoryTabView.UpdateSelectionSummary.
    pub(crate) fn selection_summary(&self, tab: TabId) -> AppResult<Option<String>> {
        let cell = self.tab(tab)?;
        let t = lock(&cell);
        if t.selection.is_empty() {
            return Ok(None);
        }
        let (count, bytes) = t
            .listing
            .rows()
            .iter()
            .filter(|r| t.selection.contains(&r.id))
            .fold((0i64, 0i64), |(n, b), r| {
                (n + 1, b + r.size.unwrap_or(0).max(0))
            });
        if count == 0 {
            return Ok(None);
        }
        let noun = if count == 1 { "item" } else { "items" };
        let counted = thousands(count);
        Ok(Some(if bytes > 0 {
            format!(
                "{counted} {noun} selected ({})",
                bb_core::format::byte_size(bytes)
            )
        } else {
            format!("{counted} {noun} selected")
        }))
    }

    /// "Select by pattern" (`select`) and "deselect by pattern": what `pattern` picks out of the
    /// rows on screen, as the dialog says it while typing (`{total, matched, problem, summary}`),
    /// and with `apply` the matches added to the selection or taken out of it.
    /// Spec: Views/SelectPatternDialog, DirectoryTabView.SelectByPattern.
    pub(crate) fn select_by_pattern(
        &self,
        tab: TabId,
        pattern: &str,
        select: bool,
        apply: bool,
    ) -> AppResult<serde_json::Value> {
        use bb_core::commands::{SelectionPattern, SelectionRow};
        use bb_core::format::thousands;
        let rows: Vec<SelectionRow> = {
            let cell = self.tab(tab)?;
            let t = lock(&cell);
            t.listing
                .rows()
                .iter()
                .map(|r| SelectionRow {
                    name: r.name.clone(),
                    full_path: r.full_path.clone(),
                    is_directory: r.is_directory,
                    size_bytes: r.size.unwrap_or(0),
                    modified_utc: r.modified,
                    created_utc: r.created,
                    attributes: r.attributes,
                })
                .collect()
        };
        let found = SelectionPattern::matches(pattern, &rows);
        let total = rows.len();
        let summary = match &found.problem {
            Some(problem) => problem.clone(),
            None if pattern.trim().is_empty() => {
                format!("{} item(s) here.", thousands(total as i64))
            }
            None => format!(
                "{} of {} item(s) match.",
                thousands(found.indexes.len() as i64),
                thousands(total as i64)
            ),
        };
        let usable = found.problem.is_none() && !found.indexes.is_empty();
        if apply && usable {
            let indices: Vec<u32> = found.indexes.iter().map(|&i| i as u32).collect();
            let op = if select {
                SelectOp::Add
            } else {
                SelectOp::Remove
            };
            self.select(tab, op, &indices)?;
        }
        Ok(serde_json::json!({
            "total": total,
            "matched": found.indexes.len(),
            "problem": found.problem,
            "summary": summary,
            "usable": usable,
        }))
    }

    /// Selected rows' positions in the current listing, ascending.
    pub(crate) fn selected_indices(&self, tab: TabId) -> AppResult<Vec<u32>> {
        let cell = self.tab(tab)?;
        let t = lock(&cell);
        Ok(t.listing
            .rows()
            .iter()
            .enumerate()
            .filter(|(_, r)| t.selection.contains(&r.id))
            .map(|(i, _)| i as u32)
            .collect())
    }

    pub(crate) fn selected_paths(&self, tab: TabId) -> AppResult<Vec<String>> {
        let cell = self.tab(tab)?;
        let t = lock(&cell);
        Ok(t.listing
            .rows()
            .iter()
            .filter(|r| t.selection.contains(&r.id))
            .map(|r| r.full_path.clone())
            .collect())
    }

    /// Enter on a row: a folder opens in place, anything else goes to the launcher, whose refusal is
    /// the status line.
    pub(crate) fn open_item(self: &Arc<Self>, tab: TabId, index: u32) -> AppResult<()> {
        let cell = self.tab(tab)?;
        let row = lock(&cell)
            .listing
            .row(index as usize)
            .cloned()
            .ok_or(AppError::Stale)?;
        if row.is_directory {
            return self.navigate(tab, &row.full_path);
        }
        // An archive opens as a folder when the setting says so and the container can be read (or only
        // needs its password). One that cannot be read goes to whatever owns the extension, rather than
        // taking the tab inside it to show an empty folder.
        let inside = crate::commands::ArchivePathCheck::in_archive(&lock(&cell).path);
        if !inside
            && bb_core::archives::ArchiveFormats::is_archive_name(&row.name)
            && lock(&self.settings).enter_archives_on_double_click
        {
            // A name is a claim and only the bytes settle it: one that is not what its name says goes
            // to whatever owns the extension; anything else (an unsupported kind included) is entered,
            // and its listing says what is wrong. Read off this thread, with the list showing it is
            // busy, since a large container takes a while. Spec: DirectoryTabViewModel.EnterArchiveOrLaunchAsync.
            let generation = {
                let mut t = lock(&cell);
                t.is_loading = true;
                t.generation
            };
            self.hub.emit(AppEvent::TabChanged { tab });
            self.schedule(move |inner| {
                let index = inner.fs.read_archive(&row.full_path);
                let Ok(cell) = inner.tab(tab) else { return };
                {
                    let mut t = lock(&cell);
                    if t.generation != generation {
                        return; // the tab went somewhere else meanwhile
                    }
                    t.is_loading = false;
                }
                inner.hub.emit(AppEvent::TabChanged { tab });
                if index.failure != bb_core::archives::ArchiveFailure::Damaged {
                    let _ = inner.navigate(tab, &row.full_path);
                } else {
                    inner.launch_row(tab, &row);
                }
            });
            return Ok(());
        }
        // Nothing inside a container has a path another program can open.
        if inside {
            self.set_status(tab, format!("Extract {} to open it.", row.name));
            return Ok(());
        }
        self.launch_row(tab, &row);
        Ok(())
    }

    /// Hands a file to whatever owns its extension, saying so when that is refused.
    fn launch_row(&self, tab: TabId, row: &Row) {
        if let Err(why) = self.seams.launcher.launch(&row.full_path, false) {
            self.set_status(tab, format!("{} was not launched \u{2014} {why}", row.name));
        }
    }

    // ---- refreshing other tabs after an operation ----

    pub(crate) fn refresh_tabs_showing(self: &Arc<Self>, directories: &[String]) {
        let keys: std::collections::HashSet<String> = directories
            .iter()
            .filter(|d| !d.trim().is_empty())
            .filter_map(|d| PathKey::canonicalize(d).ok())
            .collect();
        if keys.is_empty() {
            return;
        }
        for (id, cell) in self.all_tabs() {
            let showing = {
                let t = lock(&cell);
                !t.path.is_empty()
                    && PathKey::canonicalize(&t.path)
                        .map(|k| keys.contains(&k))
                        .unwrap_or(false)
            };
            if showing {
                self.refresh_view(id);
            }
        }
        // A comparison over these folders is a verdict about a disk that just changed: it says so.
        self.compare_directories_changed(&keys, false);
    }

    /// Steps any tab that is inside a folder that just went away out to the nearest parent that
    /// still exists, before it is reloaded onto a missing path.
    pub(crate) fn leave_gone_folders(self: &Arc<Self>, gone: &[String]) {
        for folder in gone {
            let Ok(gone_key) = PathKey::canonicalize(folder) else {
                continue;
            };
            let Some(parent) = parent_path(&PathKey::normalize_display(folder).unwrap_or_default())
            else {
                continue;
            };
            for (id, cell) in self.all_tabs() {
                let inside = {
                    let t = lock(&cell);
                    !t.path.is_empty()
                        && PathKey::canonicalize(&t.path)
                            .map(|k| k == gone_key || PathKey::is_under_key(&k, &gone_key))
                            .unwrap_or(false)
                };
                if inside {
                    let _ = self.navigate(id, &parent);
                }
            }
        }
    }

    pub(crate) fn refresh_all(self: &Arc<Self>) {
        for (id, _) in self.all_tabs() {
            self.refresh_view(id);
        }
    }
}

enum Kind {
    Directory,
    Search,
    Flat,
}

/// Re-sorts the tab's rows in place (a new snapshot, same ids and selection).
fn resort(t: &mut TabData) {
    let spec = t.sort_spec();
    let mut rows: Vec<Arc<crate::browse::rows::Row>> = t.listing.source_rows().to_vec();
    rows.sort_by(|a, b| spec.compare(a, b));
    t.version += 1;
    t.listing = t.make_listing(rows, t.is_flattened, t.is_flat_browse);
}

/// A copy of a row, id and all, for building the next snapshot of a streaming listing.
fn clone_row(r: &Row) -> Row {
    r.duplicate()
}

/// Index rows from the names-only fallback carry no times; stat just those.
fn hydrate_search_metadata(row: &mut Row) {
    use std::os::windows::fs::MetadataExt;
    if !row.modified.is_unknown() {
        return;
    }
    if let Ok(meta) = std::fs::metadata(&row.full_path) {
        row.modified = bb_core::FileTime(meta.last_write_time() as i64);
        row.created = bb_core::FileTime(meta.creation_time() as i64);
        row.accessed = bb_core::FileTime(meta.last_access_time() as i64);
        row.attributes = meta.file_attributes();
        if !row.is_directory {
            row.size = Some(meta.file_size() as i64);
        }
    }
}

fn search_suffix(outcome: &SearchOutcome, global: bool, any_index: bool) -> String {
    use bb_core::models::{ContentScanLimit, SearchResultSource as Source};
    if outcome.cancelled {
        return " \u{2014} stopped".to_string();
    }
    // What the reading pass did comes first: a content search that stopped at a ceiling found what it
    // found in the part of the disk it reached, and "0 results" alone would report an absence the search
    // never established. Spec: DirectoryTabViewModel (the suffix), ContentLimitSuffix.
    if let Some(scan) = &outcome.content_scan {
        if scan.limit != ContentScanLimit::None {
            let place = scan
                .last_path_examined
                .as_deref()
                .filter(|p| !p.is_empty())
                .map(|p| format!(", up to {p}"))
                .unwrap_or_default();
            return match scan.limit {
                ContentScanLimit::Bytes => format!(
                    " \u{2014} stopped after reading {}{place}",
                    byte_size(scan.bytes_read as i64)
                ),
                _ => format!(
                    " \u{2014} searched the first {} file(s){place}",
                    thousands(scan.files_read as i64)
                ),
            };
        }
        if scan.unreadable > 0 {
            return format!(
                " \u{2014} {} file(s) could not be read",
                thousands(scan.unreadable as i64)
            );
        }
        if scan.truncated > 0 {
            return format!(
                " \u{2014} {} file(s) were only searched to their first {}",
                thousands(scan.truncated as i64),
                byte_size(bb_core::search::ContentSearchRules::MAX_BYTES_PER_FILE as i64)
            );
        }
    }
    if outcome.scope_lacks_metadata {
        return " \u{2014} this drive has no size or date data, so those filters can't match"
            .into();
    }
    if global && !any_index {
        return " \u{2014} the search index is off".to_string();
    }
    match outcome.source {
        Source::LiveScan => " \u{2014} indexing in background\u{2026}".to_string(),
        Source::StaleIndex => " \u{2014} refreshing index\u{2026}".to_string(),
        _ if global && outcome.refresh_pending => " \u{2014} indexing drives\u{2026}".to_string(),
        _ => " \u{2014} indexed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::models::{ContentScanLimit, ContentScanReport, SearchResultSource};

    fn with_scan(scan: ContentScanReport) -> SearchOutcome {
        let mut o = SearchOutcome::new(Vec::new(), false, SearchResultSource::Index, false);
        o.content_scan = Some(scan);
        o
    }

    /// What the reading pass did is said before anything about the index: a content search that hit a
    /// ceiling found what it found in the part of the disk it reached. Spec: DirectoryTabViewModel
    /// (ContentLimitSuffix and the suffix switch).
    #[test]
    fn a_content_pass_that_was_cut_short_says_where_and_why() {
        let files = with_scan(ContentScanReport {
            files_read: 50_000,
            limit: ContentScanLimit::Files,
            last_path_examined: Some(r"C:\Data\m.txt".into()),
            ..Default::default()
        });
        assert_eq!(
            " \u{2014} searched the first 50,000 file(s), up to C:\\Data\\m.txt",
            search_suffix(&files, false, true)
        );
        let unreadable = with_scan(ContentScanReport {
            unreadable: 3,
            ..Default::default()
        });
        assert_eq!(
            " \u{2014} 3 file(s) could not be read",
            search_suffix(&unreadable, false, true)
        );
        let big = with_scan(ContentScanReport {
            truncated: 2,
            ..Default::default()
        });
        assert!(
            search_suffix(&big, false, true)
                .starts_with(" \u{2014} 2 file(s) were only searched to their first ")
        );
        let whole = with_scan(ContentScanReport::default());
        assert_eq!(" \u{2014} indexed", search_suffix(&whole, false, true));
    }
}
