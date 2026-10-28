//! The C ABI between the Rust core and the C++ UI. cbindgen reads this crate alone and writes
//! `native/include/bbcore.h`.
//!
//! Rules that every export keeps:
//! - Every `bb_*` call is made from the UI thread, and none blocks on long work. Results arrive as
//!   events through the sink.
//! - The sink is invoked on Rust threads and **may only enqueue**. Rust never re-enters C++ any other
//!   way.
//! - Every export runs under `catch_unwind`: a panic becomes `BB_PANIC`, never an unwind across the
//!   boundary. The message is readable with `bb_last_error` on the same thread.
//! - A handle or id that names something that is gone (a closed tab, a destroyed pane) answers
//!   `BB_STALE`; ids are never reused within a run.
//! - Strings cross as UTF-16. `BbStr` is a borrowed view: valid for the call when it is an argument,
//!   and for the documented lifetime of its owner when it is a result. `BbOwnedStr` is freed with
//!   `bb_str_free`; a null `ptr` means "none".
//! - Events are Rust-owned; the receiver frees each with `bb_event_free` once it is done with it.

#![allow(clippy::missing_safety_doc)]
#![allow(non_camel_case_types)]

use std::any::Any;
use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

use bb_app::{
    App, AppConfig, AppError, AppEvent, Fakes, Listing, OpId, OpState, PaneId, SelectOp, TabId,
};
use bb_core::commands::{ChordModifiers, FocusKind, FocusState, KeyChord};
use bb_core::delete::DeleteMode;
use bb_core::layout::SplitOrientation;

mod features;
pub use features::*;
mod ops_ffi;
pub use ops_ffi::*;
mod panes;
pub use panes::*;
mod tools;
pub use tools::*;

// ---- statuses and strings ----

/// The outcome of a call. Only `BB_OK` carries a result.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbStatus {
    /// The call succeeded.
    BB_OK = 0,
    /// The call failed; `bb_last_error` has the message.
    BB_ERR = 1,
    /// Rust panicked inside the call; `bb_last_error` has the panic message.
    BB_PANIC = 2,
    /// A handle or id named something that no longer exists.
    BB_STALE = 3,
    /// Another operation holds the app; try again once it finishes.
    BB_BUSY = 4,
}

/// A borrowed UTF-16 string view. Not NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbStr {
    pub ptr: *const u16,
    pub len: u32,
}

impl BbStr {
    /// cbindgen:ignore
    const EMPTY: BbStr = BbStr {
        ptr: ptr::null(),
        len: 0,
    };

    fn of(units: &[u16]) -> BbStr {
        BbStr {
            ptr: units.as_ptr(),
            len: units.len() as u32,
        }
    }

    /// Copies the view into a `String`. The view must be readable for the duration of the call.
    unsafe fn to_string_lossy(self) -> String {
        if self.ptr.is_null() || self.len == 0 {
            return String::new();
        }
        let units = unsafe { std::slice::from_raw_parts(self.ptr, self.len as usize) };
        String::from_utf16_lossy(units)
    }
}

/// A UTF-16 string the caller owns. Free it with `bb_str_free`. `ptr == NULL` means none.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbOwnedStr {
    pub ptr: *mut u16,
    pub len: u32,
}

impl BbOwnedStr {
    fn none() -> BbOwnedStr {
        BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        }
    }

    fn from_str(s: &str) -> BbOwnedStr {
        let boxed: Box<[u16]> = s.encode_utf16().collect();
        let len = boxed.len() as u32;
        BbOwnedStr {
            ptr: Box::into_raw(boxed).cast::<u16>(),
            len,
        }
    }
}

// ---- events ----

/// What an event is about. `BbEvent.id` and `a` mean different things per kind; each variant says.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbEventKind {
    /// A tab's listing was replaced. `id` = tab, `a` = the new listing version. Drop the snapshot you
    /// hold and its text layouts, and ask for a new one.
    BB_EVT_LISTING_CHANGED = 1,
    /// A tab's path, history, flags, status text or selection moved. `id` = tab.
    BB_EVT_TAB_CHANGED = 2,
    /// Panes or tabs were added, removed or activated.
    BB_EVT_LAYOUT_CHANGED = 3,
    /// The settle predicate flipped. `a` = 1 when busy.
    BB_EVT_BUSY_CHANGED = 4,
    /// The undo history gained, lost or moved an entry.
    BB_EVT_UNDO_CHANGED = 5,
    /// An operation finished. `id` = operation, `a` = 1 when it succeeded.
    BB_EVT_OP_FINISHED = 6,
    /// A setting changed. `text` = its key.
    BB_EVT_SETTINGS_CHANGED = 7,
    /// A pane took the focus or changed its tab. `id` = pane.
    BB_EVT_PANE_CHANGED = 8,
    /// A service's state moved; re-read it. `a` = the service: 1 settings page, 2 theme (re-push the
    /// palette), 3 menu bar/toolbars, 4 keymap, 5 palette, 6 Keyboard page, 7 Menus and toolbars
    /// page, 8 saved searches/workspaces/bookmarks, 9 index banner/status, 10 columns, 11 compare,
    /// 12 command availability.
    BB_EVT_MODEL_CHANGED = 9,
    /// A tab shows shell-property columns with cells the host has not read. `id` = tab. Ask with
    /// `bb_props_requests`, answer with `bb_props_supply`.
    BB_EVT_PROPERTIES_NEEDED = 10,
}

/// An event delivered to the sink. Rust owns it; call `bb_event_free` when done. `text` is valid
/// until then.
#[repr(C)]
pub struct BbEvent {
    pub kind: BbEventKind,
    pub id: u64,
    pub a: u64,
    pub b: u64,
    pub text: BbStr,
    pub payload: *const c_void,
}

/// The receiver of events. Called on Rust threads: enqueue and return, do nothing else.
pub type BbEventSink = Option<unsafe extern "C" fn(ctx: *mut c_void, evt: *mut BbEvent)>;

#[repr(C)]
struct EventBox {
    event: BbEvent,
    text: Vec<u16>,
}

fn deliver(sink: BbEventSink, ctx: usize, kind: BbEventKind, id: u64, a: u64, text: &str) {
    let Some(sink) = sink else { return };
    let mut boxed = Box::new(EventBox {
        event: BbEvent {
            kind,
            id,
            a,
            b: 0,
            text: BbStr::EMPTY,
            payload: ptr::null(),
        },
        text: text.encode_utf16().collect(),
    });
    boxed.event.text = BbStr::of(&boxed.text);
    let raw = Box::into_raw(boxed);
    // EventBox is repr(C) with `event` first, so a pointer to the box is a pointer to the event.
    let _ = panic::catch_unwind(AssertUnwindSafe(|| unsafe {
        sink(ctx as *mut c_void, raw.cast::<BbEvent>())
    }));
}

type SinkSlot = Arc<Mutex<Option<(unsafe extern "C" fn(*mut c_void, *mut BbEvent), usize)>>>;

fn forward(slot: &SinkSlot, event: AppEvent) {
    let installed = *slot.lock().unwrap_or_else(|p| p.into_inner());
    let Some((sink, ctx)) = installed else { return };
    let sink = Some(sink);
    match event {
        AppEvent::ListingChanged { tab, version } => deliver(
            sink,
            ctx,
            BbEventKind::BB_EVT_LISTING_CHANGED,
            tab.0,
            version,
            "",
        ),
        AppEvent::TabChanged { tab } => {
            deliver(sink, ctx, BbEventKind::BB_EVT_TAB_CHANGED, tab.0, 0, "")
        }
        AppEvent::LayoutChanged => deliver(sink, ctx, BbEventKind::BB_EVT_LAYOUT_CHANGED, 0, 0, ""),
        AppEvent::BusyChanged { busy } => deliver(
            sink,
            ctx,
            BbEventKind::BB_EVT_BUSY_CHANGED,
            0,
            u64::from(busy),
            "",
        ),
        AppEvent::UndoChanged => deliver(sink, ctx, BbEventKind::BB_EVT_UNDO_CHANGED, 0, 0, ""),
        AppEvent::OpFinished { op, ok } => deliver(
            sink,
            ctx,
            BbEventKind::BB_EVT_OP_FINISHED,
            op.0,
            u64::from(ok),
            "",
        ),
        AppEvent::SettingsChanged { key } => {
            deliver(sink, ctx, BbEventKind::BB_EVT_SETTINGS_CHANGED, 0, 0, &key)
        }
        AppEvent::PaneChanged { pane } => {
            deliver(sink, ctx, BbEventKind::BB_EVT_PANE_CHANGED, pane.0, 0, "")
        }
        AppEvent::ModelChanged { model } => deliver(
            sink,
            ctx,
            BbEventKind::BB_EVT_MODEL_CHANGED,
            0,
            u64::from(model as u8),
            "",
        ),
        AppEvent::PropertiesNeeded { tab } => deliver(
            sink,
            ctx,
            BbEventKind::BB_EVT_PROPERTIES_NEEDED,
            tab.0,
            0,
            "",
        ),
    }
}

// ---- errors and the guard every export runs under ----

thread_local! {
    static LAST_ERROR: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
}

fn set_last_error(message: &str) {
    LAST_ERROR.with(|e| {
        let mut e = e.borrow_mut();
        e.clear();
        e.extend(message.encode_utf16());
    });
}

fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".to_string()
    }
}

/// Why a call failed, before it becomes a status.
enum Fail {
    Err(String),
    Stale,
    Busy,
}

impl From<AppError> for Fail {
    fn from(e: AppError) -> Fail {
        match e {
            AppError::Stale => Fail::Stale,
            AppError::Busy => Fail::Busy,
            AppError::Invalid(m) => Fail::Err(m),
        }
    }
}

impl From<String> for Fail {
    fn from(s: String) -> Fail {
        Fail::Err(s)
    }
}

impl From<&str> for Fail {
    fn from(s: &str) -> Fail {
        Fail::Err(s.to_string())
    }
}

/// Runs `f` with the panic and error conventions every export follows.
fn guard(f: impl FnOnce() -> Result<(), Fail>) -> BbStatus {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => BbStatus::BB_OK,
        Ok(Err(Fail::Err(message))) => {
            set_last_error(&message);
            BbStatus::BB_ERR
        }
        Ok(Err(Fail::Stale)) => {
            set_last_error("stale id: the thing it names is gone");
            BbStatus::BB_STALE
        }
        Ok(Err(Fail::Busy)) => {
            set_last_error("busy: another operation is running");
            BbStatus::BB_BUSY
        }
        Err(payload) => {
            set_last_error(&panic_message(&payload));
            BbStatus::BB_PANIC
        }
    }
}

/// For the few exports that return a plain value rather than a status.
fn guard_value<T>(default: T, f: impl FnOnce() -> T) -> T {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(payload) => {
            set_last_error(&panic_message(&payload));
            default
        }
    }
}

/// The app handle. One per process.
pub struct BbApp {
    app: App,
    sink: SinkSlot,
}

unsafe fn app_of<'a>(p: *mut BbApp) -> Result<&'a BbApp, Fail> {
    if p.is_null() {
        return Err(Fail::Err("null app".into()));
    }
    Ok(unsafe { &*p })
}

unsafe fn out<'a, T>(p: *mut T, what: &str) -> Result<&'a mut T, Fail> {
    if p.is_null() {
        return Err(Fail::Err(format!("{what} is null")));
    }
    Ok(unsafe { &mut *p })
}

unsafe fn strings(ptr: *const BbStr, n: u32) -> Vec<String> {
    if ptr.is_null() || n == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(ptr, n as usize) }
        .iter()
        .map(|s| unsafe { s.to_string_lossy() })
        .collect()
}

// ---- process-level ----

/// The core version, as a view into static storage.
#[unsafe(no_mangle)]
pub extern "C" fn bb_version() -> BbStr {
    static VERSION: OnceLock<Vec<u16>> = OnceLock::new();
    BbStr::of(VERSION.get_or_init(|| env!("CARGO_PKG_VERSION").encode_utf16().collect()))
}

/// The message behind the last non-OK status on this thread. The view is valid until the next
/// `bb_*` call on the same thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_last_error(out: *mut BbStr) {
    if out.is_null() {
        return;
    }
    let view = LAST_ERROR.with(|e| BbStr::of(&e.borrow()));
    unsafe { out.write(view) };
}

/// Frees a string returned as `BbOwnedStr`. A null `ptr` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_str_free(s: BbOwnedStr) {
    if s.ptr.is_null() {
        return;
    }
    let slice = ptr::slice_from_raw_parts_mut(s.ptr, s.len as usize);
    drop(unsafe { Box::from_raw(slice) });
}

/// Frees an event once the receiver is done with it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_event_free(evt: *mut BbEvent) {
    if evt.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(evt.cast::<EventBox>()) });
}

/// Always panics inside the guard, to prove `BB_PANIC` and that nothing unwinds across the boundary.
#[unsafe(no_mangle)]
pub extern "C" fn bb_selftest_panic() -> BbStatus {
    guard(|| panic!("selftest: deliberate panic"))
}

// ---- the app ----

/// Which seams the app is built over.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbFakes {
    /// The real ones (for now: unattended defaults where the UI has not landed yet).
    BB_FAKES_NONE = 0,
    /// The harness set: nothing is launched, elevation is declined, prompts are recorded.
    BB_FAKES_HARNESS = 1,
}

/// How to build the app. A zero or empty field takes its default.
#[repr(C)]
pub struct BbAppConfig {
    /// The data directory. Required.
    pub data_dir: BbStr,
    pub fakes: BbFakes,
    /// How long typing in a search box waits before it runs. `UINT32_MAX` keeps the default (200);
    /// the harness passes 0.
    pub search_debounce_ms: u32,
    /// How long a folder change waits for quiet before the listing is merged. 0 keeps the default.
    pub watch_debounce_ms: u32,
    /// Where deletes that cannot use the Recycle Bin are held; empty means the root of each volume.
    pub delete_staging_root: BbStr,
    /// Worker threads; 0 keeps the default (4).
    pub workers: u32,
}

/// Builds the app. Free it with `bb_app_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_app_create(
    cfg: *const BbAppConfig,
    out_app: *mut *mut BbApp,
) -> BbStatus {
    guard(|| unsafe { build_app(cfg, u32::MAX, false, out_app) })
}

/// As [`BbAppConfig`], with the settings the harness and the tests need beyond it.
#[repr(C)]
pub struct BbAppConfigEx {
    pub base: BbAppConfig,
    /// How long the Settings page waits for quiet before writing the file. `UINT32_MAX` keeps the
    /// default (400); 0 writes at once.
    pub settings_debounce_ms: u32,
    /// Put the real index client behind a launcher that declines the elevation prompt (the
    /// harness's `--index-declined`).
    pub index_declined: bool,
}

/// As `bb_app_create`, taking the extended configuration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_app_create_ex(
    cfg: *const BbAppConfigEx,
    out_app: *mut *mut BbApp,
) -> BbStatus {
    guard(|| {
        if cfg.is_null() {
            return Err("null config".into());
        }
        let ex = unsafe { &*cfg };
        unsafe {
            build_app(
                &ex.base,
                ex.settings_debounce_ms,
                ex.index_declined,
                out_app,
            )
        }
    })
}

unsafe fn build_app(
    cfg: *const BbAppConfig,
    settings_debounce_ms: u32,
    index_declined: bool,
    out_app: *mut *mut BbApp,
) -> Result<(), Fail> {
    {
        let out_app = unsafe { out(out_app, "out_app")? };
        if cfg.is_null() {
            return Err("null config".into());
        }
        let cfg = unsafe { &*cfg };
        let data_dir = unsafe { cfg.data_dir.to_string_lossy() };
        if data_dir.is_empty() {
            return Err("config.data_dir is required".into());
        }
        let mut config = AppConfig::new(data_dir);
        config.fakes = match cfg.fakes {
            BbFakes::BB_FAKES_NONE => Fakes::None,
            BbFakes::BB_FAKES_HARNESS => Fakes::Harness,
        };
        if cfg.search_debounce_ms != u32::MAX {
            config.search_debounce_ms = u64::from(cfg.search_debounce_ms);
        }
        if cfg.watch_debounce_ms != 0 {
            config.watch_debounce_ms = u64::from(cfg.watch_debounce_ms);
        }
        let staging = unsafe { cfg.delete_staging_root.to_string_lossy() };
        if !staging.is_empty() {
            config.delete_staging_root = Some(staging);
        }
        if cfg.workers != 0 {
            config.workers = cfg.workers as usize;
        }
        if settings_debounce_ms != u32::MAX {
            config.settings_debounce_ms = u64::from(settings_debounce_ms);
        }
        config.index_declined = index_declined;
        let app = App::new(config)?;
        let sink: SinkSlot = Arc::new(Mutex::new(None));
        {
            let sink = sink.clone();
            app.set_event_sink(Some(Arc::new(move |e| forward(&sink, e))));
        }
        *out_app = Box::into_raw(Box::new(BbApp { app, sink }));
        Ok(())
    }
}

/// Stops the app's threads and frees it. The handle is invalid afterwards. Events already queued by
/// the sink stay valid until freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_app_destroy(app: *mut BbApp) {
    if app.is_null() {
        return;
    }
    let boxed = unsafe { Box::from_raw(app) };
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        *boxed.sink.lock().unwrap_or_else(|p| p.into_inner()) = None;
        boxed.app.release_undo_history();
        boxed.app.shutdown();
    }));
}

/// Installs the event sink (or removes it with NULL). It is called on Rust threads and must only
/// enqueue; `ctx` is passed back untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_app_set_event_sink(
    app: *mut BbApp,
    sink: BbEventSink,
    ctx: *mut c_void,
) {
    let _ = guard(|| {
        let app = unsafe { app_of(app)? };
        *app.sink.lock().unwrap_or_else(|p| p.into_inner()) = sink.map(|s| (s, ctx as usize));
        Ok(())
    });
}

/// Opens the first tab at `start_path` and returns it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_app_initialize(
    app: *mut BbApp,
    start_path: BbStr,
    out_tab: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let tab = app
            .app
            .initialize(&unsafe { start_path.to_string_lossy() })?;
        if let Some(out_tab) = unsafe { out_tab.as_mut() } {
            *out_tab = tab.0;
        }
        Ok(())
    })
}

/// The settle predicate: `(transferring && !paused) || loading || hydrating`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_app_is_busy(app: *mut BbApp) -> bool {
    guard_value(false, || {
        unsafe { app.as_ref() }.is_some_and(|a| a.app.is_busy())
    })
}

/// The Rust half of the harness's `state` verb, as JSON. Free with `bb_str_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_app_state_json(app: *mut BbApp, out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        *unsafe { out(out_json, "out_json")? } = BbOwnedStr::from_str(&app.app.state_json());
        Ok(())
    })
}

// ---- panes and tabs ----

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbOrientation {
    /// Children sit side by side.
    BB_VERTICAL = 0,
    /// Children are stacked.
    BB_HORIZONTAL = 1,
}

/// Splits `pane` (0 = the active pane), putting a new pane with one tab at `path` beside it. Returns
/// the new pane.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_pane_split(
    app: *mut BbApp,
    pane: u64,
    orientation: BbOrientation,
    path: BbStr,
    out_pane: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let o = match orientation {
            BbOrientation::BB_VERTICAL => SplitOrientation::Vertical,
            BbOrientation::BB_HORIZONTAL => SplitOrientation::Horizontal,
        };
        let pane = if pane == 0 {
            app.app.active_pane()
        } else {
            PaneId(pane)
        };
        let new_pane = app
            .app
            .split_pane(pane, o, &unsafe { path.to_string_lossy() })?;
        if let Some(out_pane) = unsafe { out_pane.as_mut() } {
            *out_pane = new_pane.0;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_pane_close(app: *mut BbApp, pane: u64) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.close_pane(PaneId(pane))?))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_pane_activate(app: *mut BbApp, pane: u64) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.activate_pane(PaneId(pane))?))
}

/// Opens a tab in `pane` (0 = the active pane). Returns the new tab.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_open(
    app: *mut BbApp,
    pane: u64,
    path: BbStr,
    activate: bool,
    out_tab: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let pane = (pane != 0).then_some(PaneId(pane));
        let tab = app
            .app
            .open_tab(pane, &unsafe { path.to_string_lossy() }, activate)?;
        if let Some(out_tab) = unsafe { out_tab.as_mut() } {
            *out_tab = tab.0;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_close(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.close_tab(TabId(tab))?))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_activate(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.activate_tab(TabId(tab))?))
}

/// Navigates to a folder. The listing arrives as `BB_EVT_LISTING_CHANGED`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_navigate(app: *mut BbApp, tab: u64, path: BbStr) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .navigate(TabId(tab), &unsafe { path.to_string_lossy() })?)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_back(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.back(TabId(tab))?))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_forward(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.forward(TabId(tab))?))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_up(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.up(TabId(tab))?))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_refresh(app: *mut BbApp, tab: u64) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.refresh(TabId(tab))?))
}

/// Sets the folder search box (`global` false) or the whole-PC one (true). Empty text clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_set_search(
    app: *mut BbApp,
    tab: u64,
    text: BbStr,
    global: bool,
) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }.app.set_search(
            TabId(tab),
            &unsafe { text.to_string_lossy() },
            global,
        )?)
    })
}

/// Turns the flat branch view on or off.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_set_flat(app: *mut BbApp, tab: u64, on: bool) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.set_flat_view(TabId(tab), on)?))
}

/// Lowers how many rows a flat listing shows before it stops and says so (the harness's `flat-cap`);
/// takes effect on the next flat listing. The default is the real ceiling, 50,000.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_flat_set_cap(app: *mut BbApp, cap: u32) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.set_flat_cap(cap as usize)?))
}

/// Sorts by a column id; the same column again flips the direction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_set_sort(app: *mut BbApp, tab: u64, column: BbStr) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .set_sort(TabId(tab), &unsafe { column.to_string_lossy() })?)
    })
}

/// The thumbnail slider, 0 (details list) to 1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_set_thumbnail_scale(
    app: *mut BbApp,
    tab: u64,
    scale: f64,
) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .set_thumbnail_scale(TabId(tab), scale)?)
    })
}

/// 0 none, 1 preview, 2 metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_set_side_pane(app: *mut BbApp, tab: u64, pane: u32) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .set_side_pane(TabId(tab), pane.min(255) as u8)?)
    })
}

/// Enter on a row: a folder opens in place, anything else goes to the launcher.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_open_item(app: *mut BbApp, tab: u64, index: u32) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.open_item(TabId(tab), index)?))
}

/// A tab's scalars. Text lives behind `bb_tab_text`.
#[repr(C)]
pub struct BbTabState {
    pub id: u64,
    pub pane: u64,
    /// Bumped on every new listing snapshot.
    pub version: u64,
    pub count: u32,
    pub selected: u32,
    pub can_back: bool,
    pub can_forward: bool,
    pub can_up: bool,
    /// The rows come from many folders (a search, or a flat view).
    pub is_flattened: bool,
    /// The flattening is a browse of a real folder, not a search result.
    pub is_flat_browse: bool,
    pub is_flat_view: bool,
    pub is_global_search: bool,
    pub sort_descending: bool,
    pub shows_content_matches: bool,
    pub is_search_running: bool,
    pub has_error: bool,
    /// 0 none, 1 preview, 2 metadata.
    pub side_pane: u32,
    pub thumbnail_scale: f64,
    /// The preview pane's mode (0 auto, 1 raw, 2 hex) and its two per-tab switches (wrap, 100%).
    pub preview_mode: u32,
    pub preview_wrap: bool,
    pub preview_fit: bool,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_state(
    app: *mut BbApp,
    tab: u64,
    out_state: *mut BbTabState,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let out_state = unsafe { out(out_state, "out_state")? };
        let i = app.app.tab_info(TabId(tab))?;
        *out_state = BbTabState {
            id: i.id.0,
            pane: i.pane.0,
            version: i.version,
            count: i.count as u32,
            selected: i.selected as u32,
            can_back: i.can_back,
            can_forward: i.can_forward,
            can_up: i.can_up,
            is_flattened: i.is_flattened,
            is_flat_browse: i.is_flat_browse,
            is_flat_view: i.is_flat_view,
            is_global_search: i.is_global_search,
            sort_descending: i.sort_desc,
            shows_content_matches: i.shows_content_matches,
            is_search_running: i.is_search_running,
            has_error: i.error.is_some(),
            side_pane: u32::from(i.side_pane),
            thumbnail_scale: i.thumbnail_scale,
            preview_mode: u32::from(i.preview_mode),
            preview_wrap: i.preview_wrap,
            preview_fit: i.preview_fit,
        };
        Ok(())
    })
}

/// Which text of a tab to read.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbTabText {
    BB_TEXT_PATH = 0,
    BB_TEXT_TITLE = 1,
    BB_TEXT_STATUS = 2,
    /// The error message; none (NULL) when the listing did not fail.
    BB_TEXT_ERROR = 3,
    /// The message shown over an empty result; none when there is no such message.
    BB_TEXT_EMPTY = 4,
    /// A note the listing is not saying for itself (a flat view stopped at its ceiling); none if none.
    BB_TEXT_NOTICE = 5,
    /// The text of the active search box.
    BB_TEXT_SEARCH = 6,
    /// The id of the column the list is sorted by.
    BB_TEXT_SORT_BY = 7,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_text(
    app: *mut BbApp,
    tab: u64,
    which: BbTabText,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let out_text = unsafe { out(out_text, "out_text")? };
        let i = app.app.tab_info(TabId(tab))?;
        let value: Option<String> = match which {
            BbTabText::BB_TEXT_PATH => Some(i.path),
            BbTabText::BB_TEXT_TITLE => Some(i.title),
            BbTabText::BB_TEXT_STATUS => Some(i.status),
            BbTabText::BB_TEXT_ERROR => i.error,
            BbTabText::BB_TEXT_EMPTY => i.empty_message,
            BbTabText::BB_TEXT_NOTICE => i.notice,
            BbTabText::BB_TEXT_SEARCH => Some(i.search_text),
            BbTabText::BB_TEXT_SORT_BY => Some(i.sort_by),
        };
        *out_text = value.map_or_else(BbOwnedStr::none, |v| BbOwnedStr::from_str(&v));
        Ok(())
    })
}

/// The breadcrumb as lines of `name<TAB>full path`, root first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_breadcrumb(
    app: *mut BbApp,
    tab: u64,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let out_text = unsafe { out(out_text, "out_text")? };
        let crumbs = app.app.breadcrumb(TabId(tab))?;
        let text = crumbs
            .iter()
            .map(|c| format!("{}\t{}", c.name, c.full_path))
            .collect::<Vec<_>>()
            .join("\n");
        *out_text = BbOwnedStr::from_str(&text);
        Ok(())
    })
}

/// The columns the tab shows, as lines of
/// `id<TAB>header<TAB>width<TAB>cell index<TAB>right aligned (0/1)<TAB>sortable (0/1)`. The cell
/// index is what `bb_listing_cell` takes, or -1 for a column whose values this build cannot read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_columns(
    app: *mut BbApp,
    tab: u64,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let out_text = unsafe { out(out_text, "out_text")? };
        let columns = app.app.columns_with_cells(TabId(tab))?;
        let text = columns
            .iter()
            .map(|(c, cell)| {
                format!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    c.spec.id,
                    c.spec.header,
                    c.width,
                    cell,
                    u8::from(c.spec.right_aligned),
                    u8::from(c.spec.sortable)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        *out_text = BbOwnedStr::from_str(&text);
        Ok(())
    })
}

// ---- the listing ----

/// An immutable, reference-counted snapshot of a tab's rows in display order. Every view it hands out
/// stays valid until the last reference is released.
pub struct BbListing {
    _opaque: (),
}

fn listing_of<'a>(l: *const BbListing) -> Result<&'a Listing, Fail> {
    if l.is_null() {
        return Err(Fail::Err("null listing".into()));
    }
    Ok(unsafe { &*(l as *const Listing) })
}

/// The tab's current snapshot, with one reference for the caller (release it with
/// `bb_listing_release`). NULL when the tab is gone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_listing(app: *mut BbApp, tab: u64) -> *const BbListing {
    guard_value(ptr::null(), || {
        let Some(app) = (unsafe { app.as_ref() }) else {
            return ptr::null();
        };
        match app.app.listing(TabId(tab)) {
            Ok(listing) => Arc::into_raw(listing) as *const BbListing,
            Err(e) => {
                set_last_error(&e.to_string());
                ptr::null()
            }
        }
    })
}

/// Takes another reference to a snapshot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_retain(l: *const BbListing) {
    if l.is_null() {
        return;
    }
    unsafe { Arc::increment_strong_count(l as *const Listing) };
}

/// Gives a reference back. The snapshot is freed with the last one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_release(l: *const BbListing) {
    if l.is_null() {
        return;
    }
    drop(unsafe { Arc::from_raw(l as *const Listing) });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_len(l: *const BbListing) -> u32 {
    guard_value(0, || listing_of(l).map(|l| l.len() as u32).unwrap_or(0))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_version(l: *const BbListing) -> u64 {
    guard_value(0, || listing_of(l).map(|l| l.version).unwrap_or(0))
}

/// `BbRow.flags` bits.
pub const BB_ROW_DIRECTORY: u32 = 1;
pub const BB_ROW_HIDDEN: u32 = 2;
pub const BB_ROW_MEDIA: u32 = 4;
/// A folder size that is a floor, because some folders could not be read.
pub const BB_ROW_SIZE_INCOMPLETE: u32 = 8;
/// Bits 4..6 are what a folder comparison says about the row (`compare_view::state_number`: 0 nothing,
/// 1 not compared, 2 same, 3 only here, 4 newer, 5 older, 6 differs): `(flags >> 4) & 7`.
pub const BB_ROW_COMPARE_SHIFT: u32 = 4;

/// One row, as views into the snapshot. Ask for visible rows at paint time only.
#[repr(C)]
pub struct BbRow {
    /// Stable per path within a tab's browsing of one place, so hover survives a watcher merge.
    pub row_id: u64,
    pub name: BbStr,
    pub full_path: BbStr,
    /// The folder a search or flat result came from, relative to where it was searched; empty
    /// otherwise.
    pub relative_dir: BbStr,
    /// Bytes, or -1 for a folder whose size is not known (blank, never zero).
    pub size: i64,
    /// FILETIME ticks (100 ns since 1601 UTC); 0 when unknown.
    pub modified: i64,
    pub created: i64,
    pub accessed: i64,
    /// The Win32 `FILE_ATTRIBUTE_*` mask.
    pub attributes: u32,
    /// `BB_ROW_*` bits.
    pub flags: u32,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_row(
    l: *const BbListing,
    index: u32,
    out_row: *mut BbRow,
) -> BbStatus {
    guard(|| {
        let listing = listing_of(l)?;
        let out_row = unsafe { out(out_row, "out_row")? };
        let row = listing
            .row(index as usize)
            .ok_or_else(|| Fail::Err(format!("row {index} is out of range")))?;
        let mut flags = 0;
        if row.is_directory {
            flags |= BB_ROW_DIRECTORY;
        }
        if row.is_hidden() {
            flags |= BB_ROW_HIDDEN;
        }
        if row.is_media {
            flags |= BB_ROW_MEDIA;
        }
        if row.size_incomplete {
            flags |= BB_ROW_SIZE_INCOMPLETE;
        }
        flags |= (u32::from(row.compare.load(std::sync::atomic::Ordering::Relaxed)) & 7)
            << BB_ROW_COMPARE_SHIFT;
        *out_row = BbRow {
            row_id: row.id,
            name: BbStr::of(row.name_wide()),
            full_path: BbStr::of(row.path_wide()),
            relative_dir: BbStr::of(row.relative_dir_wide()),
            size: row.size.unwrap_or(-1),
            modified: row.modified.0,
            created: row.created.0,
            accessed: row.accessed.0,
            attributes: row.attributes,
            flags,
        };
        Ok(())
    })
}

/// The formatted text of one cell. `column` is a cell index from `bb_tab_columns`. The view is valid
/// as long as the snapshot is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_listing_cell(
    l: *const BbListing,
    index: u32,
    column: u32,
    out_text: *mut BbStr,
) -> BbStatus {
    guard(|| {
        let listing = listing_of(l)?;
        let out_text = unsafe { out(out_text, "out_text")? };
        let cell = listing
            .cell(index as usize, column)
            .ok_or_else(|| Fail::Err(format!("row {index} is out of range")))?;
        *out_text = BbStr::of(cell);
        Ok(())
    })
}

// ---- selection ----

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbSelectOp {
    /// Select exactly these rows.
    BB_SELECT_REPLACE = 0,
    BB_SELECT_ADD = 1,
    BB_SELECT_TOGGLE = 2,
    BB_SELECT_REMOVE = 3,
    BB_SELECT_ALL = 4,
    BB_SELECT_CLEAR = 5,
    BB_SELECT_INVERT = 6,
}

/// One gesture, one call: `idx` are positions in the snapshot the caller holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_select(
    app: *mut BbApp,
    tab: u64,
    op: BbSelectOp,
    idx: *const u32,
    n: u32,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let indices: &[u32] = if idx.is_null() || n == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(idx, n as usize) }
        };
        let op = match op {
            BbSelectOp::BB_SELECT_REPLACE => SelectOp::Replace,
            BbSelectOp::BB_SELECT_ADD => SelectOp::Add,
            BbSelectOp::BB_SELECT_TOGGLE => SelectOp::Toggle,
            BbSelectOp::BB_SELECT_REMOVE => SelectOp::Remove,
            BbSelectOp::BB_SELECT_ALL => SelectOp::All,
            BbSelectOp::BB_SELECT_CLEAR => SelectOp::Clear,
            BbSelectOp::BB_SELECT_INVERT => SelectOp::Invert,
        };
        Ok(app.app.select(TabId(tab), op, indices)?)
    })
}

/// The selected rows' positions in the current snapshot, ascending. Writes up to `cap` of them and
/// reports the total in `out_count`, so a caller can size a second call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_selection(
    app: *mut BbApp,
    tab: u64,
    out_idx: *mut u32,
    cap: u32,
    out_count: *mut u32,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let selected = app.app.selected_indices(TabId(tab))?;
        if !out_idx.is_null() {
            let n = selected.len().min(cap as usize);
            unsafe { ptr::copy_nonoverlapping(selected.as_ptr(), out_idx, n) };
        }
        if let Some(out_count) = unsafe { out_count.as_mut() } {
            *out_count = selected.len() as u32;
        }
        Ok(())
    })
}

/// The selected rows' full paths, one per line.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_tab_selection_paths(
    app: *mut BbApp,
    tab: u64,
    out_text: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let out_text = unsafe { out(out_text, "out_text")? };
        *out_text = BbOwnedStr::from_str(&app.app.selected_paths(TabId(tab))?.join("\n"));
        Ok(())
    })
}

// ---- operations ----

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbDeleteMode {
    /// Into the Windows Recycle Bin.
    BB_DELETE_RECYCLE = 0,
    /// Into this app's holding folder; undoable for the session.
    BB_DELETE_STAGED = 1,
    /// Erased in place; no undo.
    BB_DELETE_PERMANENT = 2,
}

/// Moves the sources into a folder. Queued behind a running operation. The outcome arrives as
/// `BB_EVT_OP_FINISHED`; a move is on the undo history.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_move(
    app: *mut BbApp,
    src: *const BbStr,
    n: u32,
    dest: BbStr,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let op = app.app.op_move(unsafe { strings(src, n) }, &unsafe {
            dest.to_string_lossy()
        })?;
        if let Some(out_op) = unsafe { out_op.as_mut() } {
            *out_op = op.0;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_copy(
    app: *mut BbApp,
    src: *const BbStr,
    n: u32,
    dest: BbStr,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let op = app.app.op_copy(unsafe { strings(src, n) }, &unsafe {
            dest.to_string_lossy()
        })?;
        if let Some(out_op) = unsafe { out_op.as_mut() } {
            *out_op = op.0;
        }
        Ok(())
    })
}

/// Deletes the sources. `BB_BUSY` while another operation runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_delete(
    app: *mut BbApp,
    src: *const BbStr,
    n: u32,
    mode: BbDeleteMode,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let mode = match mode {
            BbDeleteMode::BB_DELETE_RECYCLE => DeleteMode::Recycle,
            BbDeleteMode::BB_DELETE_STAGED => DeleteMode::Staged,
            BbDeleteMode::BB_DELETE_PERMANENT => DeleteMode::Permanent,
        };
        let op = app.app.op_delete(unsafe { strings(src, n) }, mode)?;
        if let Some(out_op) = unsafe { out_op.as_mut() } {
            *out_op = op.0;
        }
        Ok(())
    })
}

/// Renames the sources by a pattern ("Holiday" numbers across the selection, each keeping its
/// extension). `BB_BUSY` while another operation runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_rename(
    app: *mut BbApp,
    src: *const BbStr,
    n: u32,
    pattern: BbStr,
    out_op: *mut u64,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let op = app.app.op_rename(unsafe { strings(src, n) }, &unsafe {
            pattern.to_string_lossy()
        })?;
        if let Some(out_op) = unsafe { out_op.as_mut() } {
            *out_op = op.0;
        }
        Ok(())
    })
}

/// Pauses or resumes the running transfer, mid-file. The queue stays held while paused.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_pause(app: *mut BbApp, paused: bool) -> BbStatus {
    guard(|| {
        unsafe { app_of(app)? }.app.op_pause(paused);
        Ok(())
    })
}

/// Cancels an operation: one still waiting never starts, the running one is told to stop (and a
/// pause is lifted so it can notice).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_cancel(app: *mut BbApp, op: u64) -> BbStatus {
    guard(|| Ok(unsafe { app_of(app)? }.app.op_cancel(OpId(op))?))
}

/// Where an operation is.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbOpPhase {
    BB_OP_QUEUED = 0,
    BB_OP_RUNNING = 1,
    BB_OP_DONE = 2,
}

#[repr(C)]
pub struct BbOpInfo {
    pub phase: BbOpPhase,
    /// Meaningful once `phase` is `BB_OP_DONE`.
    pub ok: bool,
}

/// An operation's phase, and (in `out_message`, when non-NULL) the sentence it ended with.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_op_state(
    app: *mut BbApp,
    op: u64,
    out_info: *mut BbOpInfo,
    out_message: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let state = app.app.op_state(OpId(op)).ok_or(Fail::Stale)?;
        let (phase, ok, message) = match state {
            OpState::Queued => (BbOpPhase::BB_OP_QUEUED, false, String::new()),
            OpState::Running => (BbOpPhase::BB_OP_RUNNING, false, String::new()),
            OpState::Done { ok, message } => (BbOpPhase::BB_OP_DONE, ok, message),
        };
        if let Some(info) = unsafe { out_info.as_mut() } {
            *info = BbOpInfo { phase, ok };
        }
        if let Some(m) = unsafe { out_message.as_mut() } {
            *m = BbOwnedStr::from_str(&message);
        }
        Ok(())
    })
}

/// Undoes `steps` entries of the history, or redoes `-steps` when negative. `BB_BUSY` while another
/// operation runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_undo_step(app: *mut BbApp, steps: i32, out_op: *mut u64) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let op = app.app.undo_step(steps)?;
        if let Some(out_op) = unsafe { out_op.as_mut() } {
            *out_op = op.0;
        }
        Ok(())
    })
}

#[repr(C)]
pub struct BbUndoState {
    pub can_undo: bool,
    pub can_redo: bool,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_undo_state(app: *mut BbApp, out_state: *mut BbUndoState) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let out_state = unsafe { out(out_state, "out_state")? };
        *out_state = BbUndoState {
            can_undo: app.app.can_undo(),
            can_redo: app.app.can_redo(),
        };
        Ok(())
    })
}

// ---- keys and commands ----

/// A key press, translated by the UI: `modifiers` is a mask (1 Ctrl, 2 Alt, 4 Shift) and `key` the
/// key's name in this app's vocabulary (`P`, `F7`, `Del`, `Num+`). Rust decides what it means.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbKeyChord {
    pub modifiers: u32,
    pub key: BbStr,
}

/// What has the keyboard, as far as a shortcut needs to know.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbFocusKind {
    BB_FOCUS_FILE_LIST = 0,
    /// A box somebody is typing in.
    BB_FOCUS_TEXT_EDITABLE = 1,
    /// Selectable text nobody can type in.
    BB_FOCUS_TEXT_READ_ONLY = 2,
    BB_FOCUS_OTHER = 3,
}

/// `BbFocusState.flags` bits.
pub const BB_FOCUS_SETTINGS_OPEN: u32 = 1;
pub const BB_FOCUS_PALETTE_OPEN: u32 = 2;
/// A modal dialog is up but the window still receives keys behind it.
pub const BB_FOCUS_CONTENT_BLOCKED: u32 = 4;
/// The Keyboard page is waiting for a chord.
pub const BB_FOCUS_RECORDING: u32 = 8;
/// A menu or dropdown has the keyboard.
pub const BB_FOCUS_IN_POPUP: u32 = 16;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbFocusState {
    pub focus: BbFocusKind,
    pub flags: u32,
}

/// Which command, if any, owns this key press in this state. `out_command` is NULL when it is nobody's
/// and must carry on to the focused control. Says nothing about whether the command is available: a
/// claimed chord is the command's even when it can do nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_key_dispatch(
    app: *mut BbApp,
    chord: BbKeyChord,
    focus: BbFocusState,
    out_command: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let out_command = unsafe { out(out_command, "out_command")? };
        *out_command = BbOwnedStr::none();
        let mut modifiers = ChordModifiers::NONE;
        if chord.modifiers & 1 != 0 {
            modifiers = modifiers | ChordModifiers::CTRL;
        }
        if chord.modifiers & 2 != 0 {
            modifiers = modifiers | ChordModifiers::ALT;
        }
        if chord.modifiers & 4 != 0 {
            modifiers = modifiers | ChordModifiers::SHIFT;
        }
        let key = unsafe { chord.key.to_string_lossy() };
        // A key this app has no name for cannot be bound, so it is nobody's.
        let Some(chord) = KeyChord::create(modifiers, &key) else {
            return Ok(());
        };
        let mut state = FocusState::new(match focus.focus {
            BbFocusKind::BB_FOCUS_FILE_LIST => FocusKind::FileList,
            BbFocusKind::BB_FOCUS_TEXT_EDITABLE => FocusKind::TextEditable,
            BbFocusKind::BB_FOCUS_TEXT_READ_ONLY => FocusKind::TextReadOnly,
            BbFocusKind::BB_FOCUS_OTHER => FocusKind::Other,
        });
        state.settings_open = focus.flags & BB_FOCUS_SETTINGS_OPEN != 0;
        state.palette_open = focus.flags & BB_FOCUS_PALETTE_OPEN != 0;
        state.content_blocked = focus.flags & BB_FOCUS_CONTENT_BLOCKED != 0;
        state.recording = focus.flags & BB_FOCUS_RECORDING != 0;
        state.in_popup = focus.flags & BB_FOCUS_IN_POPUP != 0;
        if let Some(id) = app.app.key_dispatch(&chord, &state) {
            *out_command = BbOwnedStr::from_str(&id);
        }
        Ok(())
    })
}

/// Runs a command against the active tab. Fails with the reason `bb_command_why_not` gives.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_command_run(app: *mut BbApp, id: BbStr) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .command_run(&unsafe { id.to_string_lossy() })?)
    })
}

/// Why a command cannot run now, in words fit to show beside it; `out_reason` is NULL when it can.
/// `BB_ERR` for an id nobody knows. This is the one gate a menu row, a key and the palette share.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_command_why_not(
    app: *mut BbApp,
    id: BbStr,
    out_reason: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let out_reason = unsafe { out(out_reason, "out_reason")? };
        *out_reason = app
            .app
            .command_why_not(&unsafe { id.to_string_lossy() })?
            .map_or_else(BbOwnedStr::none, |r| BbOwnedStr::from_str(&r));
        Ok(())
    })
}

// ---- settings and theme ----

/// One setting as JSON text (`null` for an unset optional). Keys are the settings file's.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_settings_get(
    app: *mut BbApp,
    key: BbStr,
    out_json: *mut BbOwnedStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let out_json = unsafe { out(out_json, "out_json")? };
        *out_json = BbOwnedStr::from_str(&app.app.settings_get(&unsafe { key.to_string_lossy() })?);
        Ok(())
    })
}

/// Sets one setting from JSON text, validating it against the model. It takes effect at once; a
/// setting the Settings page owns is written on a 400 ms debounce (`bb_settings_flush` writes it
/// now), one a service owns (keymap, bars, theme) is written at once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_settings_set(
    app: *mut BbApp,
    key: BbStr,
    json_value: BbStr,
) -> BbStatus {
    guard(|| {
        Ok(unsafe { app_of(app)? }
            .app
            .settings_set(&unsafe { key.to_string_lossy() }, &unsafe {
                json_value.to_string_lossy()
            })?)
    })
}

/// How many colour tokens a palette holds.
#[unsafe(no_mangle)]
pub extern "C" fn bb_theme_token_count() -> u32 {
    bb_core::theming::ThemeToken::COUNT as u32
}

/// The current theme's colours as 0xAARRGGBB, one per token in token order. Writes up to `cap` and
/// reports how many exist in `out_count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_theme_palette(
    app: *mut BbApp,
    out_argb: *mut u32,
    cap: u32,
    out_count: *mut u32,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        let palette = app.app.theme_palette();
        if !out_argb.is_null() {
            let n = palette.len().min(cap as usize);
            unsafe { ptr::copy_nonoverlapping(palette.as_ptr(), out_argb, n) };
        }
        if let Some(out_count) = unsafe { out_count.as_mut() } {
            *out_count = palette.len() as u32;
        }
        Ok(())
    })
}
