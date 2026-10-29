//! The host's startup, in the order `WinMain` needs it. Spec: App.xaml.cs (`Main`, `OnStartup`),
//! Services/SingleInstance.cs, Services/StartupTrace.cs, Services/UpdateService.cs.
//!
//! 1. `bb_startup`: the installer's hooks, before anything else. A hook exits the process, so this
//!    must not be gated behind an instance check.
//! 2. `bb_launch_claim`: parse the command line, claim the per-user instance, hand the request to the
//!    running copy if there is one (then the caller exits).
//! 3. `bb_app_create` (with the data directory migrated first on a first native launch) and the
//!    window.
//! 4. `bb_launch_serve`: once the window exists, answer other launches, open what was left of the
//!    command line, and do the work that acts on the machine (repair the folder-handler registration,
//!    check for updates, sweep abandoned delete staging), which a benchmark launch skips.
//!
//! Nothing here starts a process: the update check hands off to the installer's own updater and the
//! launcher seam does the rest.

use super::*;

use std::path::{Path, PathBuf};

use bb_core::benchmarking::StartupMarks;
use bb_core::cli::{CommandLine, CommandLineRequest};
use bb_core::settings::LegacyData;
use bb_shell::startup_trace::{self, StartupTrace};
use bb_shell::updates::{self, UpdateOutcome};
use bb_shell::{FolderHandlerRegistry, SingleInstance, foreground};

/// `argv[1..]`, from the wide command line the host parsed.
unsafe fn args_of(argc: i32, argv: *const *const u16) -> Vec<String> {
    if argv.is_null() || argc <= 1 {
        return Vec::new();
    }
    (1..argc as usize)
        .map(|i| {
            let p = unsafe { *argv.add(i) };
            if p.is_null() {
                return String::new();
            }
            let mut len = 0usize;
            while unsafe { *p.add(len) } != 0 {
                len += 1;
            }
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) })
        })
        .collect()
}

/// Runs the installer's hooks (`--veloapp-*`) and applies an update that is waiting. Call it first
/// in `WinMain`. Returns normally for an ordinary launch; for a hook the process exits inside it.
/// `argv` is the whole command line with the program name first, as `CommandLineToArgvW` gives it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_startup(argc: i32, argv: *const *const u16) -> BbStatus {
    guard(|| {
        let trace = startup_trace::global();
        trace.mark(StartupMarks::MAIN);
        let args = unsafe { args_of(argc, argv) };
        // Uninstalling deletes the whole install directory, so a folder-handler registration left
        // behind would point the Windows shell at an executable that is gone, and since it owns the
        // Directory and Drive open verbs, every folder double-click on the machine would fail. The
        // hook runs inside the uninstall with a 30-second budget and touches nothing but the registry.
        updates::run_installer_hooks(args, || {
            FolderHandlerRegistry::system().try_unregister();
        });
        trace.mark(StartupMarks::VELOPACK_DONE);
        Ok(())
    })
}

/// What one launch is: its request, whether another copy took it, and the instance claim.
pub struct BbLaunch {
    instance: SingleInstance,
    request: CommandLineRequest,
    handed_off: bool,
    start_path: Vec<u16>,
    start_path_text: Option<String>,
}

fn is_existing_directory(path: &str) -> bool {
    Path::new(path).is_dir()
}

/// Parses the command line and claims the per-user instance. If another copy is running and takes
/// the request, `bb_launch_handed_off` is true and the caller should free this and exit. Otherwise
/// (this is the first copy, or the first could not be reached) carry on starting normally.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_launch_claim(
    argc: i32,
    argv: *const *const u16,
    data_dir: BbStr,
    out_launch: *mut *mut BbLaunch,
) -> BbStatus {
    guard(|| {
        let out_launch = unsafe { out(out_launch, "out_launch")? };
        let args = unsafe { args_of(argc, argv) };
        let data_dir = PathBuf::from(unsafe { data_dir.to_string_lossy() });
        let request = CommandLine::parse(&args);

        let instance = SingleInstance::claim(&data_dir);
        let trace = startup_trace::global();
        trace.mark(StartupMarks::INSTANCE_CLAIMED);
        trace.set_instance(instance.is_first());
        let handed_off = !instance.is_first() && instance.hand_off(&request);

        // Someone who named a folder on the command line asked for that folder. Reopening last
        // session's panes over the top of it would bury the thing they asked for.
        let start_path_text = request
            .targets
            .iter()
            .find(|t| is_existing_directory(&t.path))
            .map(|t| t.path.clone());
        let start_path = start_path_text
            .as_deref()
            .map(|p| p.encode_utf16().collect())
            .unwrap_or_default();
        *out_launch = Box::into_raw(Box::new(BbLaunch {
            instance,
            request,
            handed_off,
            start_path,
            start_path_text,
        }));
        Ok(())
    })
}

/// True when a running copy took this launch's request: free the launch and exit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_launch_handed_off(launch: *const BbLaunch) -> bool {
    guard_value(false, || {
        unsafe { launch.as_ref() }.is_some_and(|l| l.handed_off)
    })
}

/// The folder the command line named for the window to open on, or empty. A view valid until
/// `bb_launch_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_launch_start_path(launch: *const BbLaunch) -> BbStr {
    guard_value(BbStr::EMPTY, || match unsafe { launch.as_ref() } {
        Some(l) => BbStr::of(&l.start_path),
        None => BbStr::EMPTY,
    })
}

/// True when `BERTBROWSER_STARTUP_TRACE` is set: this launch is a measurement. The window is to be
/// parked offscreen without activation or a taskbar button.
#[unsafe(no_mangle)]
pub extern "C" fn bb_trace_enabled() -> bool {
    startup_trace::global().enabled()
}

/// Records a mark on the way up (`windowBuilt`, `windowShown`, `contentRendered`, ...) when tracing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_trace_mark(name: BbStr) {
    let _ = guard(|| {
        startup_trace::global().mark(&unsafe { name.to_string_lossy() });
        Ok(())
    });
}

/// Carries out a command that starts another program for the user and that `bb_command_execute`
/// handed back as the host's (`file.run-as-admin`, `file.open-terminal`, `file.open-vscode`, a
/// `custom:` command): the program, its arguments and its working directory are decided in Rust
/// from the selection, and started through the one launcher. The outcome is on the status line.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_launch_command(app: *mut BbApp, id: BbStr) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        Ok(app.app.launch_command(&unsafe { id.to_string_lossy() })?)
    })
}

/// As `bb_launch_command`, on `folder` rather than the selection: the folder tree's menu.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_launch_command_at(
    app: *mut BbApp,
    id: BbStr,
    folder: BbStr,
) -> BbStatus {
    guard(|| {
        let app = unsafe { app_of(app)? };
        Ok(app
            .app
            .launch_command_at(&unsafe { id.to_string_lossy() }, &unsafe {
                folder.to_string_lossy()
            })?)
    })
}

/// Who opens folders and drives on this machine, as JSON: `{"state": "NotRegistered" |
/// "RegisteredToThisApp" | "RegisteredToThisAppStale" | "RegisteredToAnotherApp", "other": <program
/// or null>, "executable": <this app's path>}`. Reads the registry; free with `bb_str_free`. Product
/// only: the harness never calls it, so a script cannot read the machine's registry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_folder_handler_state(out_json: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        let handler = FolderHandlerRegistry::system();
        let text = serde_json::json!({
            "state": format!("{:?}", handler.state()),
            "other": handler.other_program(),
            "executable": handler.executable(),
        })
        .to_string();
        *unsafe { out(out_json, "out_json")? } = BbOwnedStr::from_str(&text);
        Ok(())
    })
}

/// Registers this app as the shell's handler for folders and drives (`open_here`), or removes
/// exactly what it registered. `BB_ERR` when the registry refused, in which case nothing is left
/// half-written. Writes `HKEY_CURRENT_USER` only. Product only, and only ever on the user's say-so.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_folder_handler_set(open_here: bool) -> BbStatus {
    guard(|| {
        if FolderHandlerRegistry::system().set(open_here) {
            Ok(())
        } else {
            Err(
                "The folder handler could not be changed: Windows refused the registry write."
                    .into(),
            )
        }
    })
}

/// Frees the launch and releases the per-user instance claim. Call it last, after the app.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_launch_free(launch: *mut BbLaunch) {
    if !launch.is_null() {
        drop(unsafe { Box::from_raw(launch) });
    }
}

fn post_close(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    // SAFETY: posting a message to a window handle; a stale one is a failed post.
    unsafe { PostMessageW(hwnd as _, WM_CLOSE, 0, 0) };
}

/// Once the main window exists: answer other launches (raising `hwnd`, unless the foreground is
/// full screen), open the rest of the command line, and start the background work. `app` must
/// outlive the launch. A benchmark launch (`bb_trace_enabled`) stops after the listener: everything
/// that acts on the machine is left out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_launch_serve(
    launch: *mut BbLaunch,
    app: *mut BbApp,
    hwnd: *mut c_void,
) -> BbStatus {
    guard(|| {
        let launch = unsafe { launch.as_mut() }.ok_or("null launch")?;
        let app = unsafe { app_of(app)? }.app.clone();
        let hwnd = hwnd as isize;
        let trace = startup_trace::global();

        if let Some(notice) = take_startup_notice() {
            app.post_status(&notice);
        }

        // Anything the first target could not cover: extra paths, /select, --new-tab.
        if !launch.request.targets.is_empty() {
            let rest =
                bb_app::remaining_after_start(&launch.request, launch.start_path_text.as_deref());
            let _ = app.open_request(&rest);
        }

        let for_requests = app.clone();
        launch.instance.start_listening(move |request| {
            // Restore and raise: the request is worthless if the window it opens in stays behind
            // whatever the user was looking at. The raise asks first; see `bb_shell::foreground`.
            if hwnd != 0 {
                foreground::raise(hwnd);
            }
            let _ = for_requests.open_request(&request);
        });

        if trace.enabled() {
            let watching = app.clone();
            trace.watch(
                move || {
                    // The same signal the harness waits on: a tab with a path in it that is no
                    // longer loading.
                    watching.active_tab().is_some_and(|tab| {
                        watching.tab_info(tab).is_ok_and(|t| !t.path.is_empty())
                            && !watching.is_busy()
                    })
                },
                move || {
                    if startup_trace::global().exit_after() {
                        post_close(hwnd);
                    }
                },
            );
            return Ok(());
        }

        let background = app.clone();
        let _ = std::thread::Builder::new()
            .name("bertbrowser startup work".to_string())
            .spawn(move || {
                // The backstop behind the uninstall hook: if this app owns the shell's folder verb
                // and the registration has gone stale (an install moved, a write interrupted
                // part-way) put a live path back. Never creates one that is absent.
                FolderHandlerRegistry::system().repair_if_stale();

                // A delete holds its items until the undo record is retired, which normally happens
                // by the time the app closes. A crash leaves them behind, so sweep up anything a
                // previous session abandoned: only batches over a day old, so a second copy running
                // right now keeps its pending undo.
                bb_core::delete::DeleteExecutor::purge_abandoned_staging(
                    std::time::Duration::from_secs(24 * 60 * 60),
                );

                // Updates are mandatory: staged to apply when the process exits, whether or not the
                // user restarts now. A failed check never takes the app down; next launch retries.
                let feed = updates::VelopackFeed::from_environment();
                let outcome = updates::check_and_stage(&feed, &|version| {
                    background.ask(&updates::ready_message(version), "Update ready", "Restart now")
                });
                if let UpdateOutcome::Staged { version, restarted: false } = outcome {
                    background.post_status(&format!(
                        "BertBrowser {version} has been downloaded and will be installed when you close the app."
                    ));
                }
            });
        Ok(())
    })
}

/// Moves a C# build's data aside, once. The first native launch over a data directory the C# app
/// used: its database, settings, themes and templates are not read (the schema and the settings
/// shape are both fresh) but they are the user's, so they are moved into `legacy-<date>` beside
/// them, never deleted. Returns a sentence for the status line when something could not be moved.
pub fn migrate_legacy_data(data_dir: &Path) -> Option<String> {
    let today = bb_core::FileTime::now().to_local();
    match LegacyData::migrate(data_dir, today) {
        Ok(outcome) if outcome.failures.is_empty() => None,
        Ok(outcome) => Some(format!(
            "The earlier version's data could not be moved aside ({}): {}",
            outcome.failures.len(),
            outcome.failures[0].error
        )),
        Err(e) => Some(format!("The data directory could not be prepared: {e}")),
    }
}

/// The trace this launch writes, for the callers in this crate.
pub(crate) fn trace() -> &'static StartupTrace {
    startup_trace::global()
}

static STARTUP_NOTICE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// A sentence for the status line once there is a tab to show it on.
pub(crate) fn set_startup_notice(text: String) {
    *STARTUP_NOTICE.lock().unwrap_or_else(|p| p.into_inner()) = Some(text);
}

fn take_startup_notice() -> Option<String> {
    STARTUP_NOTICE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = bb_test_support::unique_temp_path(&format!("bb-startup-{tag}"), "");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    #[test]
    fn the_command_line_loses_its_program_name_and_keeps_the_rest() {
        let (a, b, c) = (
            wide(r"C:\app\BertBrowser.exe"),
            wide("--new-tab"),
            wide(r"C:\Users\A B"),
        );
        let argv = [a.as_ptr(), b.as_ptr(), c.as_ptr()];
        let args = unsafe { args_of(3, argv.as_ptr()) };
        assert_eq!(
            vec!["--new-tab".to_string(), r"C:\Users\A B".to_string()],
            args
        );
        assert!(unsafe { args_of(1, argv.as_ptr()) }.is_empty());
        assert!(unsafe { args_of(0, std::ptr::null()) }.is_empty());
    }

    #[test]
    fn an_ordinary_launch_returns_from_the_installer_hooks() {
        // Not installed by Velopack and not a hook: nothing happens and the launch carries on.
        let program = wide(r"C:\app\BertBrowser.exe");
        let argv = [program.as_ptr()];
        assert_eq!(BbStatus::BB_OK, unsafe { bb_startup(1, argv.as_ptr()) });
    }

    #[test]
    fn the_first_native_launch_over_a_csharp_data_directory_moves_it_aside_once() {
        let dir = temp("legacy");
        std::fs::write(dir.join("bertbrowser.db"), b"old db").unwrap();
        std::fs::write(dir.join("bertbrowser.db-wal"), b"wal").unwrap();
        std::fs::write(dir.join("settings.json"), b"{}").unwrap();
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        std::fs::write(dir.join("instance.pipe"), b"not ours to move").unwrap();

        assert_eq!(None, migrate_legacy_data(&dir));

        assert!(!dir.join("bertbrowser.db").exists());
        assert!(!dir.join("settings.json").exists());
        assert!(dir.join("instance.pipe").exists(), "only the C# files move");
        let legacy = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().starts_with("legacy-"))
            .expect("a legacy folder");
        assert_eq!(
            b"old db",
            std::fs::read(legacy.path().join("bertbrowser.db"))
                .unwrap()
                .as_slice()
        );
        assert!(
            legacy.path().join("themes").is_dir(),
            "moved, never deleted"
        );

        // The native app now writes its own files under the same names; a second launch leaves them.
        std::fs::write(dir.join("bertbrowser.db"), b"native db").unwrap();
        assert_eq!(None, migrate_legacy_data(&dir));
        assert_eq!(
            b"native db",
            std::fs::read(dir.join("bertbrowser.db"))
                .unwrap()
                .as_slice()
        );
    }

    #[test]
    fn a_fresh_data_directory_just_gets_its_marker() {
        let dir = temp("fresh").join("child");
        assert_eq!(None, migrate_legacy_data(&dir));
        assert!(dir.join("native.marker").is_file());
        assert!(
            std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().starts_with("legacy-"))
        );
    }

    #[test]
    fn a_failed_move_is_a_sentence_for_the_status_line_not_a_crash() {
        let dir = temp("blocked");
        std::fs::write(dir.join("settings.json"), b"{}").unwrap();
        // Hold the file open without sharing delete/rename so the move is refused.
        use std::os::windows::fs::OpenOptionsExt;
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1) // FILE_SHARE_READ only: no rename
            .open(dir.join("settings.json"))
            .unwrap();
        let notice = migrate_legacy_data(&dir).expect("a notice");
        assert!(notice.contains("could not be moved aside"), "{notice}");
        assert!(dir.join("settings.json").exists(), "never lost");
    }
}
