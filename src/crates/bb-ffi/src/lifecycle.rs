//! Process-level calls and the app handle: create, destroy, sink, initialize.

use super::*;

// ---- process-level ----

/// The version this build reports: `BB_VERSION` when the build sets it (CI stamps the release or
/// unstable version that way - tools/set-version.ps1), else the workspace's committed version. An
/// environment variable rather than a rewrite of Cargo.toml, because every workspace crate inherits
/// that version and would recompile; cargo rebuilds only this crate when `BB_VERSION` changes.
const APP_VERSION: &str = match option_env!("BB_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// The core version, as a view into static storage.
#[unsafe(no_mangle)]
pub extern "C" fn bb_version() -> BbStr {
    static VERSION: OnceLock<Vec<u16>> = OnceLock::new();
    BbStr::of(VERSION.get_or_init(|| APP_VERSION.encode_utf16().collect()))
}

/// Whether this build is on the unstable release channel, decided from its own version string.
#[unsafe(no_mangle)]
pub extern "C" fn bb_is_unstable() -> bool {
    bb_core::updates::ReleaseChannel::is_unstable(Some(APP_VERSION))
}

/// What the title-bar icon's tip says about this copy, as JSON: `version` (what this build reports),
/// `source` (where it came from, in words), and for a development or local build - whose version
/// does not change from one build to the next - `built` ("3 min ago") and `location` (the folder the
/// exe runs from). Built when the tip is shown, never at startup: it reads the install's manifest and
/// the exe's date. Free with `bb_str_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_version_json(out_json: *mut BbOwnedStr) -> BbStatus {
    static INSTALLED: OnceLock<Option<String>> = OnceLock::new();
    guard(|| {
        use bb_core::updates::InstallSource;
        let source = InstallSource::of(
            INSTALLED
                .get_or_init(bb_shell::updates::installed_version)
                .as_deref(),
        );
        let exe = std::env::current_exe().ok();
        let (built, location) = match (&exe, source.is_unpublished()) {
            (Some(exe), true) => (
                std::fs::metadata(exe)
                    .and_then(|m| m.modified())
                    .map(|t| {
                        let t = bb_core::time::FileTime::from_system_time(t);
                        bb_core::format::relative_time(t, bb_core::time::FileTime::now())
                    })
                    .unwrap_or_default(),
                exe.parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ),
            _ => (String::new(), String::new()),
        };
        let json = serde_json::json!({
            "version": APP_VERSION,
            "source": source.description(),
            "built": built,
            "location": location,
        });
        put(out_json, &json.to_string())
    })
}

/// The message behind the last non-OK status on this thread. The view is valid until the next
/// `bb_*` call on the same thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_last_error(out: *mut BbStr) {
    let view = LAST_ERROR.with(|e| BbStr::of(&e.borrow()));
    set_opt(out, view);
}

/// Frees a string returned as `BbOwnedStr`. A null `ptr` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_str_free(s: BbOwnedStr) {
    if s.ptr.is_null() {
        return;
    }
    let slice = ptr::slice_from_raw_parts_mut(s.ptr, s.len as usize);
    // SAFETY: a non-null `BbOwnedStr` was made by `BbOwnedStr::from_str` from a boxed `[u16]` of this
    // exact length, and the caller frees it once.
    drop(unsafe { Box::from_raw(slice) });
}

/// Frees an event once the receiver is done with it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_event_free(evt: *mut BbEvent) {
    if evt.is_null() {
        return;
    }
    // SAFETY: a non-null event was made by `deliver` as a boxed `EventBox` (`event` is its first
    // field), and the receiver frees it once.
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
    guard(|| build_app(cfg, u32::MAX, false, out_app))
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
        let ex = config_ref(cfg)?;
        build_app(
            &ex.base,
            ex.settings_debounce_ms,
            ex.index_declined,
            out_app,
        )
    })
}

/// The caller's configuration, or an error for NULL.
fn config_ref<'a, T>(cfg: *const T) -> Result<&'a T, Fail> {
    opt_ref(cfg).ok_or_else(|| Fail::Err("null config".into()))
}

pub(crate) fn build_app(
    cfg: *const BbAppConfig,
    settings_debounce_ms: u32,
    index_declined: bool,
    out_app: *mut *mut BbApp,
) -> Result<(), Fail> {
    {
        let out_app = out(out_app, "out_app")?;
        let cfg = config_ref(cfg)?;
        let data_dir = cfg.data_dir.to_string_lossy();
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
        let staging = cfg.delete_staging_root.to_string_lossy();
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
        if config.fakes == Fakes::None {
            // The product: a first native launch over a data directory the C# app used moves its
            // files aside before anything opens the database, and a benchmark launch is told to
            // leave the machine alone.
            if let Some(notice) = startup::migrate_legacy_data(&config.data_dir) {
                startup::set_startup_notice(notice);
            }
            config.machine_effects = !startup::trace().enabled();
        }
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
    // SAFETY: `app` came from `bb_app_create` and the caller makes no call on it after this one.
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
        let app = app_of(app)?;
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
        let app = app_of(app)?;
        let tab = app.app.initialize(&start_path.to_string_lossy())?;
        if let Some(out_tab) = opt_mut(out_tab) {
            *out_tab = tab.0;
        }
        Ok(())
    })
}

/// The settle predicate: `(transferring && !paused) || loading || hydrating`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_app_is_busy(app: *mut BbApp) -> bool {
    guard_value(false, || opt_ref(app).is_some_and(|a| a.app.is_busy()))
}

/// The Rust half of the harness's `state` verb, as JSON. Free with `bb_str_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_app_state_json(app: *mut BbApp, out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let app = app_of(app)?;
        *out(out_json, "out_json")? = BbOwnedStr::from_str(&app.app.state_json());
        Ok(())
    })
}
