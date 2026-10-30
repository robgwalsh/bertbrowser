//! The app object: shared state, the pane and tab registry, and the settle predicate. The verbs live
//! beside it: navigation and listings in `tabs`, operations in `ops`, commands in `commands`.
//! Spec: ShellViewModel, PaneViewModel.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use bb_core::cancel::CancelToken;
use bb_core::columns::ColumnSetting;
use bb_core::data::{Db, DirSizeRepository, FsIndexRepository};
use bb_core::fs::{
    IndexCrawler, IndexWatcher, IndexWatcherService, LocalFileSystem, SearchService,
};
use bb_core::layout::{LayoutTree, NodeId, SplitOrientation};
use bb_core::settings::{AppPaths, AppSettings};
use bb_core::transfer::{PauseGate, TransferProgress};
use bb_core::undo::UndoStack;

use crate::config::{AppConfig, Fakes};
use crate::dirwatch::DirWatcher;
use crate::events::{AppEvent, EventHub, EventSink};
use crate::ids::{AppError, AppResult, OpId, PaneId, TabId};
use crate::pool::WorkerPool;
use crate::rows::Listing;
use crate::seams::{HarnessProbes, Seams};

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The lock if nobody holds it, None while someone does. For what the UI thread reads of the undo
/// history: an undo walk holds it throughout, and one parked on the user's answer to an elevation
/// offer would otherwise hold the thread that has to show the question.
pub(crate) fn try_lock<T>(m: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match m.try_lock() {
        Ok(g) => Some(g),
        Err(std::sync::TryLockError::Poisoned(p)) => Some(p.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// Never reports a watcher: a search then takes the stale-while-revalidate path, exactly what a
/// first search in a fresh session does.
pub(crate) struct NoWatchers;

impl IndexWatcher for NoWatchers {
    fn is_watching(&self, _root_key: &str) -> bool {
        false
    }

    fn watch(&self, _root_key: &str, _display_path: &str) {}
}

pub(crate) struct TabData {
    pub id: TabId,
    pub pane: PaneId,
    pub path: String,
    pub back: Vec<String>,
    pub forward: Vec<String>,
    pub search_text: String,
    pub global_search_text: String,
    pub is_flat_view: bool,
    pub listing: Arc<Listing>,
    pub version: u64,
    pub sort_by: String,
    pub sort_desc: bool,
    pub thumbnail_scale: f64,
    /// 0 none, 1 preview, 2 metadata.
    pub side_pane: u8,
    pub selection: HashSet<u64>,
    pub interner: HashMap<Arc<str>, u64>,
    pub next_row: u64,
    pub status: String,
    pub error: Option<String>,
    /// The archive being shown wants a password: what "Unlock archive" is for.
    pub archive_locked: bool,
    pub empty_message: Option<String>,
    pub notice: Option<String>,
    /// The rows come from many folders (a search, or a flat view).
    pub is_flattened: bool,
    /// The flattening is a browse of a real folder: it still has a "here".
    pub is_flat_browse: bool,
    pub shows_content_matches: bool,
    pub is_search_running: bool,
    /// A reload has started and its rows have not arrived yet: the activity bar under the nav row.
    pub is_loading: bool,
    /// Bumped by every refresh; a job that finds it moved discards its result.
    pub generation: u64,
    /// The generation of a search the index re-ran: that run's own re-crawl does not run it again,
    /// or a root nothing watches (so never fresh) would search and crawl for ever.
    pub index_rerun: u64,
    pub search_gen: u64,
    pub cancel: CancelToken,
    /// What Stop cancels: the search pass alone, so it returns what it found so far. Navigating
    /// cancels both, and `cancel` is what says the results no longer belong on screen.
    pub search_stop: CancelToken,
    pub watcher: Option<DirWatcher>,
    /// The columns this tab shows; None is the catalogue default. Arranged here (`columns_customized`)
    /// or inherited from the saved default.
    pub columns: Option<Vec<ColumnSetting>>,
    pub columns_customized: bool,
    /// Shell-property values the host supplied, by row key.
    pub props: HashMap<Arc<str>, crate::rows::RowProps>,
    /// The (modified, size) of a file when the host read its properties: a file written since is read again.
    pub prop_stamps: HashMap<Arc<str>, (i64, i64)>,
    /// The host has not yet looked at the property cells this listing needs.
    pub props_pending: bool,
    /// Cells handed to the host and not yet answered: (row key, upper-cased canonical name).
    pub props_inflight: HashSet<(Arc<str>, String)>,
    /// Paths to select once the listing that contains them arrives.
    pub pending_selection: Vec<String>,
    /// Bumped when a pending selection lands, with the first row it selected: the view scrolls that
    /// row into view and puts its cursor there, once per bump.
    pub reveal_seq: u64,
    pub reveal_row: Option<usize>,
    /// The preview pane's mode (0 auto, 1 raw, 2 hex) and its two per-tab switches.
    pub preview_mode: u8,
    pub preview_wrap: bool,
    pub preview_fit: bool,
    /// What the comparison on screen says about this tab's rows, while there is one.
    pub compare: Option<Arc<crate::compare_view::TabCompare>>,
}

impl TabData {
    pub(crate) fn new(id: TabId, pane: PaneId, settings: &AppSettings) -> TabData {
        TabData {
            id,
            pane,
            path: String::new(),
            back: Vec::new(),
            forward: Vec::new(),
            search_text: String::new(),
            global_search_text: String::new(),
            is_flat_view: false,
            listing: Arc::new(Listing::new(id.0, 0, Vec::new(), false, false)),
            version: 0,
            sort_by: bb_core::columns::ColumnCatalog::NAME.to_string(),
            sort_desc: false,
            thumbnail_scale: 0.0,
            side_pane: if settings.show_metadata_pane {
                2
            } else if settings.show_preview_pane {
                1
            } else {
                0
            },
            selection: HashSet::new(),
            interner: HashMap::new(),
            next_row: 0,
            status: "Ready".to_string(),
            error: None,
            archive_locked: false,
            empty_message: None,
            notice: None,
            is_flattened: false,
            is_flat_browse: false,
            shows_content_matches: false,
            is_search_running: false,
            is_loading: false,
            generation: 0,
            index_rerun: u64::MAX,
            search_gen: 0,
            cancel: CancelToken::new(),
            search_stop: CancelToken::new(),
            watcher: None,
            columns: settings.file_list_columns.clone(),
            columns_customized: false,
            props: HashMap::new(),
            prop_stamps: HashMap::new(),
            props_pending: false,
            props_inflight: HashSet::new(),
            pending_selection: Vec::new(),
            reveal_seq: 0,
            reveal_row: None,
            preview_mode: 0,
            preview_wrap: false,
            preview_fit: false,
            compare: None,
        }
    }

    /// Consumes the pending selection against the current listing.
    pub(crate) fn apply_pending_selection(&mut self) {
        if self.pending_selection.is_empty() {
            return;
        }
        let wanted: HashSet<String> = self
            .pending_selection
            .iter()
            .filter_map(|p| bb_paths::PathKey::canonicalize(p).ok())
            .collect();
        let rows = self.listing.rows();
        let first = rows.iter().position(|r| wanted.contains(&*r.key));
        let found: Vec<u64> = rows
            .iter()
            .filter(|r| wanted.contains(&*r.key))
            .map(|r| r.id)
            .collect();
        // Kept until it lands: a first listing that does not contain the item yet must not eat it.
        if !found.is_empty() {
            self.selection = found.into_iter().collect();
            self.pending_selection.clear();
            self.reveal_seq += 1;
            self.reveal_row = first;
        }
    }

    pub(crate) fn active_search_text(&self) -> &str {
        if self.global_search_text.is_empty() {
            &self.search_text
        } else {
            &self.global_search_text
        }
    }

    pub(crate) fn is_global_search(&self) -> bool {
        !self.global_search_text.is_empty()
    }
}

pub(crate) type TabCell = Arc<Mutex<TabData>>;

pub(crate) struct Pane {
    pub tabs: Vec<TabId>,
    pub active: Option<TabId>,
    pub closed_paths: Vec<String>,
}

pub(crate) struct Shell {
    pub layout: LayoutTree<PaneId>,
    pub panes: HashMap<PaneId, Pane>,
    pub active_pane: PaneId,
    pub tabs: HashMap<TabId, TabCell>,
}

impl Shell {
    pub(crate) fn leaf(&self, pane: PaneId) -> Option<NodeId> {
        self.layout.find_leaf(|p| *p == pane)
    }

    pub(crate) fn active_tab_id(&self) -> Option<TabId> {
        self.panes.get(&self.active_pane).and_then(|p| p.active)
    }
}

/// What an operation is doing, for the harness and the state dump.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpState {
    Queued,
    Running,
    Done { ok: bool, message: String },
}

pub(crate) type JobBody = Box<dyn FnOnce(&Arc<Inner>, OpId, &CancelToken) -> (bool, String) + Send>;

pub(crate) struct Job {
    pub id: OpId,
    pub cancel: CancelToken,
    pub body: JobBody,
}

#[derive(Default)]
pub(crate) struct QueueShared {
    pub state: Mutex<Queue>,
    pub cv: Condvar,
}

#[derive(Default)]
pub(crate) struct Queue {
    pub jobs: VecDeque<Job>,
    /// "This app is writing": set when a job is accepted, cleared only when the queue is empty.
    pub draining: bool,
    pub running: Option<OpId>,
    pub shutdown: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct UndoSummary {
    pub undo: Option<String>,
    pub redo: Option<String>,
    pub entries: usize,
}

pub(crate) struct Inner {
    pub config: AppConfig,
    pub hub: EventHub,
    pub seams: Seams,
    pub probes: Option<Arc<HarnessProbes>>,
    pub settings: Mutex<AppSettings>,
    pub settings_path: std::path::PathBuf,
    /// How much of each file `content:` reads (bytes); the live `searchContentMaxBytes`.
    pub content_budget: Arc<AtomicI64>,
    pub dir_sizes: DirSizeRepository,
    pub search: SearchService,
    /// The real file system with archives made to look like folders (a path inside one lists its
    /// entries); an ordinary path never reaches the archive code.
    pub fs: Arc<bb_core::archives::ArchiveAwareFileSystemService>,
    pub pool: WorkerPool,
    pub shell: Mutex<Shell>,
    pub next_id: AtomicU64,
    pub loading: AtomicUsize,
    pub hydrating: AtomicUsize,
    pub last_busy: AtomicBool,
    pub queue: Arc<QueueShared>,
    pub gate: PauseGate,
    pub undo: Mutex<UndoStack>,
    pub undo_summary: Mutex<UndoSummary>,
    pub ops: Mutex<HashMap<OpId, OpState>>,
    pub transfer_progress: Mutex<Option<TransferProgress>>,
    pub transfer_thread: Mutex<Option<JoinHandle<()>>>,
    pub db: Arc<Db>,
    pub keymap: Mutex<Arc<bb_core::commands::Keymap>>,
    /// The state of the services this layer owns beyond tabs and operations.
    pub extra: crate::state::Extra,
}

/// The app. Cheap to clone; every clone is the same app.
#[derive(Clone)]
pub struct App {
    pub(crate) inner: Arc<Inner>,
}

impl App {
    pub fn new(config: AppConfig) -> AppResult<App> {
        App::with_seams(config, |_| {})
    }

    /// As `new`, letting the caller swap any seam before the app is built over them (a test's fake
    /// elevated runner, a host's own launcher).
    pub fn with_seams(config: AppConfig, adjust: impl FnOnce(&mut Seams)) -> AppResult<App> {
        let invalid =
            |what: &str, e: &dyn std::fmt::Display| AppError::Invalid(format!("{what}: {e}"));

        let paths = AppPaths::new(&config.data_dir);
        std::fs::create_dir_all(paths.data_dir()).map_err(|e| invalid("data directory", &e))?;
        let settings_path = paths.settings_path();
        let mut settings = AppSettings::load(&settings_path);
        // Pin the answer to "does the theme follow Windows" now: a later save has no file-loaded bit
        // to derive it from, and flipping a user's theme because of how we wrote their file is the
        // surprise this guards against.
        let follows = settings.follows_system_theme();
        settings.follow_system_theme.get_or_insert(follows);

        let db = Arc::new(Db::new(paths.db_path()));
        db.migrate().map_err(|e| invalid("database", &e))?;
        let repository = Arc::new(FsIndexRepository::new(db.clone()));
        let crawler = Arc::new(IndexCrawler::new(repository.clone()));

        let hub = EventHub::default();
        let questions = Arc::new(crate::ops_ui::Questions::default());
        let passwords = Arc::new(crate::ops_arch::SessionPasswords::default());
        let (mut seams, probes) = match config.fakes {
            Fakes::Harness => {
                let (seams, probes) = Seams::harness();
                (seams, Some(probes))
            }
            Fakes::None => (
                Seams::interactive(hub.clone(), questions.clone(), paths.data_dir()),
                None,
            ),
        };
        if config.index_declined {
            seams.index = crate::index::declined_index();
        }
        adjust(&mut seams);
        let watchers: Arc<dyn IndexWatcher> = match config.fakes {
            Fakes::Harness => Arc::new(NoWatchers),
            Fakes::None => Arc::new(IndexWatcherService::new(repository.clone())),
        };
        // Read by every search, and moved by the Preview page. Spec: App.xaml.cs (per search).
        let content_budget = Arc::new(AtomicI64::new(i64::from(settings.search_content_max_bytes)));
        let fs = Arc::new(
            bb_core::archives::ArchiveAwareFileSystemService::with_passwords(
                Arc::new(LocalFileSystem),
                Arc::new(bb_core::archives::LibraryArchiveReader::default()),
                passwords.clone(),
                bb_core::archives::ArchiveCache::default(),
            ),
        );
        let search = SearchService::new(repository, crawler, watchers, seams.index.clone())
            .with_content_budget({
                let budget = content_budget.clone();
                Box::new(move || budget.load(Ordering::Relaxed).max(0) as u64)
            })
            .with_archives(fs.clone());

        let (layout, _) = LayoutTree::new(PaneId(1));
        let mut panes = HashMap::new();
        panes.insert(
            PaneId(1),
            Pane {
                tabs: Vec::new(),
                active: None,
                closed_paths: Vec::new(),
            },
        );

        let undo = UndoStack::new(settings.effective_undo_budget());
        let inner = Arc::new(Inner {
            pool: WorkerPool::new(config.workers),
            hub,
            seams,
            probes,
            dir_sizes: DirSizeRepository::new(db.clone()),
            search,
            fs,
            shell: Mutex::new(Shell {
                layout,
                panes,
                active_pane: PaneId(1),
                tabs: HashMap::new(),
            }),
            next_id: AtomicU64::new(100),
            loading: AtomicUsize::new(0),
            hydrating: AtomicUsize::new(0),
            last_busy: AtomicBool::new(false),
            queue: Arc::new(QueueShared::default()),
            gate: PauseGate::new(),
            undo: Mutex::new(undo),
            undo_summary: Mutex::new(UndoSummary::default()),
            ops: Mutex::new(HashMap::new()),
            transfer_progress: Mutex::new(None),
            transfer_thread: Mutex::new(None),
            keymap: Mutex::new(Arc::new(crate::commands::build_keymap(
                settings.key_bindings.as_ref(),
            ))),
            extra: crate::state::Extra::new(
                &settings,
                &paths,
                &config,
                &db,
                crate::ops_ui::OpsUi::new(questions, passwords),
            ),
            settings: Mutex::new(settings),
            settings_path,
            content_budget,
            db,
            config,
        });
        inner.rebuild_keymap();
        inner.theme_initialize();
        inner.start_transfer_thread();
        inner.index_attach();
        Ok(App { inner })
    }

    /// Installs (or removes) the sink. It is called on Rust threads and must only enqueue.
    pub fn set_event_sink(&self, sink: Option<EventSink>) {
        self.inner.hub.set(sink);
    }

    /// The probes behind the harness seams, when the app was built over them.
    pub fn probes(&self) -> Option<Arc<HarnessProbes>> {
        self.inner.probes.clone()
    }

    /// Opens the first tab at `start_path`.
    pub fn initialize(&self, start_path: &str) -> AppResult<TabId> {
        let pane = lock(&self.inner.shell).active_pane;
        self.inner.open_tab(pane, start_path, true)
    }

    /// The settle predicate: `(transferring && !paused) || loading || hydrating`.
    pub fn is_busy(&self) -> bool {
        self.inner.busy()
    }

    pub fn is_transferring(&self) -> bool {
        lock(&self.inner.queue.state).draining
    }

    pub fn is_paused(&self) -> bool {
        self.inner.gate.is_paused()
    }

    pub fn op_state(&self, op: OpId) -> Option<OpState> {
        lock(&self.inner.ops).get(&op).cloned()
    }

    /// Stops accepting work and joins the threads. Idempotent.
    pub fn shutdown(&self) {
        self.inner.shutdown();
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Last reference gone. The transfer thread holds only a `Weak` between jobs, so it can be
        // told to end; it is not joined here because it may be the thread running this drop.
        lock(&self.queue.state).shutdown = true;
        self.queue.cv.notify_all();
        self.pool.shutdown();
    }
}

impl Inner {
    pub(crate) fn new_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn busy(&self) -> bool {
        let transferring = lock(&self.queue.state).draining;
        (transferring && !self.gate.is_paused())
            || self.loading.load(Ordering::SeqCst) > 0
            || self.hydrating.load(Ordering::SeqCst) > 0
    }

    /// Raises `BusyChanged` when the predicate flipped since the last call.
    pub(crate) fn update_busy(&self) {
        let busy = self.busy();
        if self.last_busy.swap(busy, Ordering::SeqCst) != busy {
            self.hub.emit(AppEvent::BusyChanged { busy });
        }
    }

    pub(crate) fn tab(&self, id: TabId) -> AppResult<TabCell> {
        lock(&self.shell)
            .tabs
            .get(&id)
            .cloned()
            .ok_or(AppError::Stale)
    }

    /// Runs `job` on a worker, counting it as "loading" from this call until it ends.
    pub(crate) fn schedule(self: &Arc<Self>, job: impl FnOnce(&Arc<Inner>) + Send + 'static) {
        self.loading.fetch_add(1, Ordering::SeqCst);
        let inner = self.clone();
        self.pool.execute(move || {
            struct Done(Arc<Inner>);
            impl Drop for Done {
                fn drop(&mut self) {
                    self.0.loading.fetch_sub(1, Ordering::SeqCst);
                    self.0.update_busy();
                }
            }
            let done = Done(inner);
            job(&done.0);
        });
    }

    /// Runs `job` on a worker without counting it as "loading": for background reads nobody is waiting on, which
    /// must not hold the app busy (the startup trace, the busy event) for as long as they take.
    pub(crate) fn schedule_quiet(self: &Arc<Self>, job: impl FnOnce(&Arc<Inner>) + Send + 'static) {
        let inner = self.clone();
        self.pool.execute(move || job(&inner));
    }

    pub(crate) fn set_status(&self, tab: TabId, text: impl Into<String>) {
        if let Ok(cell) = self.tab(tab) {
            lock(&cell).status = text.into();
            self.hub.emit(AppEvent::TabChanged { tab });
        }
    }

    /// The status line of whatever tab has the focus.
    pub(crate) fn set_active_status(&self, text: impl Into<String>) {
        let active = lock(&self.shell).active_tab_id();
        if let Some(tab) = active {
            self.set_status(tab, text);
        }
    }

    // ---- panes and tabs ----

    pub(crate) fn open_tab(
        self: &Arc<Self>,
        pane: PaneId,
        path: &str,
        activate: bool,
    ) -> AppResult<TabId> {
        self.open_tab_with(pane, path, activate, |_| {})
    }

    /// As `open_tab`, arranging the new tab (`init`) before it navigates.
    pub(crate) fn open_tab_with(
        self: &Arc<Self>,
        pane: PaneId,
        path: &str,
        activate: bool,
        init: impl FnOnce(&mut TabData),
    ) -> AppResult<TabId> {
        let id = TabId(self.new_id());
        let (data, default_columns) = {
            let settings = lock(&self.settings);
            (
                TabData::new(id, pane, &settings),
                settings.file_list_columns.clone(),
            )
        };
        let cell: TabCell = Arc::new(Mutex::new(data));
        {
            let mut shell = lock(&self.shell);
            let template = shell
                .panes
                .get(&pane)
                .ok_or(AppError::Stale)?
                .active
                .and_then(|t| shell.tabs.get(&t).cloned());
            if let Some(template) = template {
                // A new tab starts as its neighbour was arranged: sort, the flat view, columns it
                // arranged itself (otherwise it keeps following the default), the side pane. Not the
                // preview's mode: a new tab's preview starts Auto. Spec: PaneFactory.CloneTab.
                let t = lock(&template);
                let mut mine = lock(&cell);
                mine.sort_by = t.sort_by.clone();
                mine.sort_desc = t.sort_desc;
                mine.is_flat_view = t.is_flat_view;
                // A tab whose arrangement is the saved default is following it, whatever its flag
                // says, and so is the copy.
                if t.columns_customized && t.columns != default_columns {
                    mine.columns = t.columns.clone();
                    mine.columns_customized = true;
                }
                mine.side_pane = t.side_pane;
            }
            init(&mut lock(&cell));
            shell.tabs.insert(id, cell);
            let p = shell.panes.get_mut(&pane).ok_or(AppError::Stale)?;
            p.tabs.push(id);
            if activate || p.active.is_none() {
                p.active = Some(id);
            }
        }
        self.hub.emit(AppEvent::LayoutChanged);
        if !path.is_empty() {
            self.navigate(id, path)?;
        }
        Ok(id)
    }

    pub(crate) fn close_tab(self: &Arc<Self>, tab: TabId) -> AppResult<()> {
        let (pane, last) = {
            let shell = lock(&self.shell);
            let cell = shell.tabs.get(&tab).ok_or(AppError::Stale)?;
            let pane = lock(cell).pane;
            let last = shell
                .panes
                .get(&pane)
                .map(|p| p.tabs.len() <= 1)
                .unwrap_or(true);
            (pane, last)
        };
        if last {
            return self.close_pane(pane);
        }
        let removed = {
            let mut shell = lock(&self.shell);
            let cell = shell.tabs.remove(&tab).ok_or(AppError::Stale)?;
            let p = shell.panes.get_mut(&pane).ok_or(AppError::Stale)?;
            let index = p.tabs.iter().position(|t| *t == tab).unwrap_or(0);
            p.tabs.retain(|t| *t != tab);
            if p.active == Some(tab) {
                p.active = p.tabs.get(index.min(p.tabs.len() - 1)).copied();
            }
            cell
        };
        self.compare_tab_closed(tab);
        {
            let mut t = removed.lock().unwrap_or_else(|p| p.into_inner());
            t.cancel.cancel();
            t.search_stop.cancel();
            t.watcher = None;
            if !t.path.is_empty() {
                let path = t.path.clone();
                drop(t);
                let mut shell = lock(&self.shell);
                if let Some(p) = shell.panes.get_mut(&pane) {
                    p.closed_paths.push(path);
                    if p.closed_paths.len() > 16 {
                        p.closed_paths.remove(0);
                    }
                }
            }
        }
        self.hub.emit(AppEvent::LayoutChanged);
        Ok(())
    }

    pub(crate) fn activate_tab(&self, tab: TabId) -> AppResult<()> {
        {
            let mut shell = lock(&self.shell);
            let pane = lock(shell.tabs.get(&tab).ok_or(AppError::Stale)?).pane;
            let p = shell.panes.get_mut(&pane).ok_or(AppError::Stale)?;
            p.active = Some(tab);
            shell.active_pane = pane;
        }
        self.hub.emit(AppEvent::LayoutChanged);
        Ok(())
    }

    pub(crate) fn step_tab(&self, pane: PaneId, step: i64) -> AppResult<()> {
        {
            let mut shell = lock(&self.shell);
            let p = shell.panes.get_mut(&pane).ok_or(AppError::Stale)?;
            if p.tabs.len() < 2 {
                return Ok(());
            }
            let current = p
                .active
                .and_then(|a| p.tabs.iter().position(|t| *t == a))
                .unwrap_or(0);
            let n = p.tabs.len() as i64;
            let next = (((current as i64 + step) % n) + n) % n;
            p.active = Some(p.tabs[next as usize]);
        }
        self.hub.emit(AppEvent::LayoutChanged);
        Ok(())
    }

    pub(crate) fn split_pane(
        self: &Arc<Self>,
        pane: PaneId,
        orientation: SplitOrientation,
        path: &str,
    ) -> AppResult<PaneId> {
        let new_pane = PaneId(self.new_id());
        {
            let mut shell = lock(&self.shell);
            let node = shell.leaf(pane).ok_or(AppError::Stale)?;
            shell.layout.split(node, orientation, new_pane);
            shell.panes.insert(
                new_pane,
                Pane {
                    tabs: Vec::new(),
                    active: None,
                    closed_paths: Vec::new(),
                },
            );
            shell.active_pane = new_pane;
        }
        self.open_tab(new_pane, path, true)?;
        self.hub.emit(AppEvent::LayoutChanged);
        Ok(new_pane)
    }

    pub(crate) fn close_pane(&self, pane: PaneId) -> AppResult<()> {
        let tabs = {
            let mut shell = lock(&self.shell);
            let node = shell.leaf(pane).ok_or(AppError::Stale)?;
            if !shell.layout.close(node) {
                return Err(AppError::Invalid("the last pane cannot be closed".into()));
            }
            let removed = shell
                .panes
                .remove(&pane)
                .map(|p| p.tabs)
                .unwrap_or_default();
            let cells: Vec<TabCell> = removed
                .iter()
                .filter_map(|t| shell.tabs.remove(t))
                .collect();
            if shell.active_pane == pane {
                let next = shell
                    .layout
                    .leaves()
                    .first()
                    .map(|n| *shell.layout.value(*n));
                if let Some(next) = next {
                    shell.active_pane = next;
                }
            }
            cells
        };
        for cell in tabs {
            let mut t = lock(&cell);
            t.cancel.cancel();
            t.search_stop.cancel();
            t.watcher = None;
        }
        self.compare_pane_closed(pane);
        self.hub.emit(AppEvent::LayoutChanged);
        Ok(())
    }

    pub(crate) fn activate_pane(&self, pane: PaneId) -> AppResult<()> {
        {
            let mut shell = lock(&self.shell);
            if !shell.panes.contains_key(&pane) {
                return Err(AppError::Stale);
            }
            shell.active_pane = pane;
        }
        self.hub.emit(AppEvent::PaneChanged { pane });
        Ok(())
    }

    pub(crate) fn step_pane(&self, step: i64) {
        let moved = {
            let mut shell = lock(&self.shell);
            match shell.leaf(shell.active_pane) {
                Some(node) => {
                    let next = shell.layout.next_leaf(node, step);
                    let pane = *shell.layout.value(next);
                    shell.active_pane = pane;
                    Some(pane)
                }
                None => None,
            }
        };
        if let Some(pane) = moved {
            self.hub.emit(AppEvent::PaneChanged { pane });
        }
    }

    /// Every tab, panes in on-screen order.
    pub(crate) fn all_tabs(&self) -> Vec<(TabId, TabCell)> {
        let shell = lock(&self.shell);
        shell
            .tabs
            .iter()
            .map(|(id, cell)| (*id, cell.clone()))
            .collect()
    }

    fn shutdown(&self) {
        // What closing the window saves: the arrangement, and anything the page still holds.
        if !lock(&self.shell).tabs.is_empty() {
            self.session_save();
        }
        let _ = self.save_settings_now();
        {
            let mut queue = lock(&self.queue.state);
            queue.shutdown = true;
            for job in queue.jobs.drain(..) {
                job.cancel.cancel();
            }
        }
        self.queue.cv.notify_all();
        if let Some(thread) = lock(&self.transfer_thread).take()
            && thread.thread().id() != std::thread::current().id()
        {
            let _ = thread.join();
        }
        for (_, cell) in self.all_tabs() {
            let mut t = lock(&cell);
            t.cancel.cancel();
            t.search_stop.cancel();
            t.watcher = None;
        }
        self.db.clear_pool();
    }
}
