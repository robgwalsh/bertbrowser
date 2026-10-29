//! The production seams, driven without a window and without a UAC prompt: an accepted elevation
//! offer really retries (through a fake runner that stands in for the helper), a declined one does
//! not, and the launcher seam carries arguments and a working directory. The real launcher and the
//! real helper are never started here.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bb_app::{App, AppConfig, ElevationLauncher, LaunchRequest, OpState, Seams};
use bb_core::cancel::CancelToken;
use bb_core::delete::{DeleteOutcome, DeleteProgress, DeleteUndoResult};
use bb_core::elevation::{
    DeleteRetry, ElevatedOperationRunner, ElevatedRun, NewItemRetry, RenameRetry, TransferRetry,
};
use bb_core::newitem::{NewItemKind, NewItemOutcome};
use bb_core::rename::RenameOutcome;
use bb_core::transfer::{TransferOutcome, TransferProgress, TransferUndoResult};
use bb_test_support::unique_temp_path;

/// Stands in for the helper: records what it was asked, answers with what the test says.
#[derive(Default)]
struct FakeHelper {
    new_items: Mutex<Vec<String>>,
    can_elevate: bool,
    result: Mutex<Option<ElevatedRun<NewItemOutcome>>>,
}

fn nothing<T>() -> ElevatedRun<T> {
    ElevatedRun::unavailable("not under test")
}

impl ElevatedOperationRunner for FakeHelper {
    fn can_elevate(&self) -> bool {
        self.can_elevate
    }
    fn run_transfer(
        &self,
        _: &TransferRetry,
        _: Option<&mut dyn FnMut(TransferProgress)>,
        _: &CancelToken,
    ) -> ElevatedRun<TransferOutcome> {
        nothing()
    }
    fn run_delete(
        &self,
        _: &DeleteRetry,
        _: Option<&mut dyn FnMut(DeleteProgress)>,
        _: &CancelToken,
    ) -> ElevatedRun<DeleteOutcome> {
        nothing()
    }
    fn run_rename(&self, _: &RenameRetry, _: &CancelToken) -> ElevatedRun<RenameOutcome> {
        nothing()
    }
    fn run_new_item(&self, retry: &NewItemRetry, _: &CancelToken) -> ElevatedRun<NewItemOutcome> {
        self.new_items
            .lock()
            .unwrap()
            .push(format!("{}\\{}", retry.plan.directory, retry.plan.name));
        self.result.lock().unwrap().take().unwrap_or_else(nothing)
    }
    fn undo_transfer(
        &self,
        _: &TransferOutcome,
        _: &CancelToken,
    ) -> ElevatedRun<TransferUndoResult> {
        nothing()
    }
    fn undo_delete(&self, _: &DeleteOutcome, _: &CancelToken) -> ElevatedRun<DeleteUndoResult> {
        nothing()
    }
}

struct Answer(bool);

impl ElevationLauncher for Answer {
    fn offer(&self, _: &str) -> bool {
        self.0
    }
}

/// A directory nobody may create files in, until `Drop` takes the rule off again. Windows only
/// enforces this in a real session, which is why the test tells the tool exactly what to deny.
struct Locked(std::path::PathBuf);

fn icacls(args: &[&str]) -> bool {
    std::process::Command::new("icacls")
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

impl Locked {
    fn new() -> Option<Locked> {
        let dir = unique_temp_path("bb-locked", "");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.to_string_lossy().into_owned();
        // Deny Everyone "add file" and "add subdirectory" (no inheritance needed: the item goes in this folder).
        if !icacls(&[&path, "/deny", "*S-1-1-0:(AD,WD)"]) {
            return None;
        }
        let locked = Locked(dir);
        // Prove the rule bites here (an elevated test process would sail through it).
        if std::fs::write(locked.0.join("probe"), b"x").is_ok() {
            return None;
        }
        Some(locked)
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        let path = self.0.to_string_lossy().into_owned();
        icacls(&[&path, "/remove:d", "*S-1-1-0"]);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn app_over(helper: Arc<FakeHelper>, agree: bool, dir: &std::path::Path) -> (App, bb_app::TabId) {
    let data = unique_temp_path("bb-seam-data", "");
    let mut config = AppConfig::harness(&data);
    config.watch_debounce_ms = 40;
    let app = App::with_seams(config, |seams: &mut Seams| {
        seams.elevated = helper;
        seams.elevation = Arc::new(Answer(agree));
    })
    .unwrap();
    let tab = app.initialize(&dir.to_string_lossy()).unwrap();
    assert!(app.wait_idle(40, 15_000));
    (app, tab)
}

fn finished(app: &App, op: bb_app::OpId) -> String {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        if let Some(OpState::Done { message, .. }) = app.op_state(op) {
            return message;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the operation never finished");
}

#[test]
fn an_accepted_offer_carries_the_refused_create_out_through_the_helper() {
    let Some(locked) = Locked::new() else {
        eprintln!("skipped: could not deny create here (an elevated session?)");
        return;
    };
    let created = locked.0.join("n.txt").to_string_lossy().into_owned();
    let helper = Arc::new(FakeHelper {
        can_elevate: true,
        result: Mutex::new(Some(ElevatedRun::ran(NewItemOutcome {
            created_path: Some(created.clone()),
            failed: None,
        }))),
        ..FakeHelper::default()
    });
    let (app, tab) = app_over(helper.clone(), true, &locked.0);

    let op = app.new_item(tab, "n.txt", NewItemKind::File).unwrap();
    let message = finished(&app, op);

    assert_eq!(
        1,
        helper.new_items.lock().unwrap().len(),
        "the helper was asked once"
    );
    assert!(helper.new_items.lock().unwrap()[0].ends_with("\\n.txt"));
    assert_eq!(
        "Created 'n.txt'", message,
        "the elevated half's outcome is the operation's"
    );
}

#[test]
fn a_declined_offer_never_reaches_the_helper_and_the_refusal_stands() {
    let Some(locked) = Locked::new() else {
        eprintln!("skipped: could not deny create here (an elevated session?)");
        return;
    };
    let helper = Arc::new(FakeHelper {
        can_elevate: true,
        ..FakeHelper::default()
    });
    let (app, tab) = app_over(helper.clone(), false, &locked.0);

    let op = app.new_item(tab, "n.txt", NewItemKind::File).unwrap();
    let message = finished(&app, op);

    assert!(helper.new_items.lock().unwrap().is_empty());
    assert!(!message.starts_with("Created"), "{message}");
}

#[test]
fn an_account_that_cannot_elevate_is_told_so_and_never_asked() {
    let Some(locked) = Locked::new() else {
        eprintln!("skipped: could not deny create here (an elevated session?)");
        return;
    };
    let helper = Arc::new(FakeHelper::default());
    let (app, tab) = app_over(helper.clone(), true, &locked.0);

    let op = app.new_item(tab, "n.txt", NewItemKind::File).unwrap();
    let message = finished(&app, op);

    assert!(helper.new_items.lock().unwrap().is_empty());
    assert!(
        message.contains("cannot provide administrator permission"),
        "{message}"
    );
}

#[test]
fn a_helper_that_fails_is_reported_in_words() {
    let Some(locked) = Locked::new() else {
        eprintln!("skipped: could not deny create here (an elevated session?)");
        return;
    };
    let helper = Arc::new(FakeHelper {
        can_elevate: true,
        result: Mutex::new(Some(ElevatedRun::unavailable("the helper did not connect"))),
        ..FakeHelper::default()
    });
    let (app, tab) = app_over(helper, true, &locked.0);

    let op = app.new_item(tab, "n.txt", NewItemKind::File).unwrap();
    assert!(finished(&app, op).contains("the helper did not connect"));
}

/// The seam carries a program, its arguments and its working directory; the default turns a request
/// into the plain `launch` the harness records, so a script's `assert-launched` keeps working.
#[test]
fn a_request_with_arguments_reaches_the_harness_launcher_as_a_plain_launch() {
    let (seams, probes) = Seams::harness();
    let request = LaunchRequest {
        file: r"C:\Tools\ide.exe".into(),
        arguments: r#""C:\x\a.sln""#.into(),
        working_directory: r"C:\x".into(),
        elevated: true,
    };
    assert!(
        seams.launcher.start(&request).is_err(),
        "the harness refuses every launch"
    );
    assert_eq!(
        vec![(r"C:\Tools\ide.exe".to_string(), true)],
        probes.launches()
    );
}

/// The real enumeration of "This PC": whatever is plugged in, it answers within its deadline and lists
/// only folders that are not the file system (never a drive or a known folder).
#[test]
fn the_real_portable_device_list_answers_in_time_and_lists_no_drive() {
    let dir = unique_temp_path("bb-devices", "");
    std::fs::create_dir_all(&dir).unwrap();
    let seams = Seams::system(&dir);
    let started = Instant::now();
    let devices = seams.devices.list();
    assert!(
        started.elapsed() < Duration::from_secs(7),
        "it took {:?}",
        started.elapsed()
    );
    for d in &devices {
        assert!(!d.name.is_empty());
        let bytes = d.shell_path.as_bytes();
        let drive_like = bytes.len() >= 2 && bytes[1] == b':';
        assert!(
            !drive_like,
            "{} is a drive, not a device: {}",
            d.name, d.shell_path
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
