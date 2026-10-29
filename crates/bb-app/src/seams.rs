//! Every seam the harness must fake, chosen once at construction. Where the C# view models awaited
//! `IProcessLauncher`, `IElevationLauncher`, `IUserConfirm`, `IUserNotice`, `IConflictPrompt`,
//! `IMftIndexService` or `ISystemAppearance`, the app calls one of these. The harness set refuses to
//! launch anything, declines elevation, records every prompt and answers from a script.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bb_core::cancel::CancelToken;
use bb_core::delete::{DeleteOutcome, DeleteProgress, DeleteUndoResult};
use bb_core::elevation::{
    DeleteRetry, ElevatedOperationRunner, ElevatedRun, NewItemRetry, RenameRetry, TransferRetry,
};
use bb_core::launch::ExecutablePath;
use bb_core::mft::{MftIndex, NullMftIndex};
use bb_core::newitem::NewItemOutcome;
use bb_core::rename::RenameOutcome;
use bb_core::theming::SystemAppearance;
use bb_core::transfer::{
    ConflictDefaults, ConflictResolution, Resolutions, TransferOutcome, TransferPlan,
    TransferProgress, TransferUndoResult,
};
use bb_paths::PathKey;
pub use bb_shell::LaunchRequest;

use crate::events::EventHub;
use crate::ops_ui::Questions;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The one place a process is started on the user's behalf. `Err` carries the words for the status
/// line.
pub trait ProcessLauncher: Send + Sync {
    fn launch(&self, path: &str, elevated: bool) -> Result<(), String>;

    /// As `launch`, with arguments and a working directory (an editor on a file, a user's command).
    fn start(&self, request: &LaunchRequest) -> Result<(), String> {
        self.launch(&request.file, request.elevated)
    }

    /// The absolute path `program` resolves to against `PATH`, or None. Callers with a fallback
    /// chain ("Windows Terminal, else PowerShell") ask this first.
    fn resolve(&self, program: &str) -> Option<String> {
        ExecutablePath::resolve_on_disk(program)
    }

    /// Whether Windows could run this file as administrator, when this launcher knows; None leaves
    /// the decision to the well-known list.
    fn can_run_elevated(&self, _path: &str) -> Option<bool> {
        None
    }
}

/// The real launcher: `ShellExecuteExW`, through the one launcher in `bb-shell`.
pub struct SystemProcessLauncher(bb_shell::SystemLauncher);

impl SystemProcessLauncher {
    pub fn new() -> SystemProcessLauncher {
        SystemProcessLauncher(bb_shell::SystemLauncher::new())
    }
}

impl Default for SystemProcessLauncher {
    fn default() -> Self {
        SystemProcessLauncher::new()
    }
}

impl ProcessLauncher for SystemProcessLauncher {
    fn launch(&self, path: &str, elevated: bool) -> Result<(), String> {
        self.0.start(&LaunchRequest {
            file: path.to_string(),
            elevated,
            ..LaunchRequest::default()
        })
    }

    fn start(&self, request: &LaunchRequest) -> Result<(), String> {
        self.0.start(request)
    }

    fn resolve(&self, program: &str) -> Option<String> {
        self.0.resolve(program)
    }

    fn can_run_elevated(&self, path: &str) -> Option<bool> {
        Some(self.0.can_run_elevated(path))
    }
}

/// Runs an offered retry with an administrator token. The harness's runner never has one.
pub type ElevatedRunner = Arc<dyn ElevatedOperationRunner + Send + Sync>;

/// What the harness and the tests get: nothing is ever run. It says it *could* elevate, so that the
/// offer is still made and recorded; the harness's prompt is what declines it.
pub struct NoElevation;

fn none<T>() -> ElevatedRun<T> {
    ElevatedRun::unavailable("elevation is not available in this run")
}

impl ElevatedOperationRunner for NoElevation {
    fn can_elevate(&self) -> bool {
        true
    }

    fn run_transfer(
        &self,
        _: &TransferRetry,
        _: Option<&mut dyn FnMut(TransferProgress)>,
        _: &CancelToken,
    ) -> ElevatedRun<TransferOutcome> {
        none()
    }

    fn run_delete(
        &self,
        _: &DeleteRetry,
        _: Option<&mut dyn FnMut(DeleteProgress)>,
        _: &CancelToken,
    ) -> ElevatedRun<DeleteOutcome> {
        none()
    }

    fn run_rename(&self, _: &RenameRetry, _: &CancelToken) -> ElevatedRun<RenameOutcome> {
        none()
    }

    fn run_new_item(&self, _: &NewItemRetry, _: &CancelToken) -> ElevatedRun<NewItemOutcome> {
        none()
    }

    fn undo_transfer(
        &self,
        _: &TransferOutcome,
        _: &CancelToken,
    ) -> ElevatedRun<TransferUndoResult> {
        none()
    }

    fn undo_delete(&self, _: &DeleteOutcome, _: &CancelToken) -> ElevatedRun<DeleteUndoResult> {
        none()
    }
}

/// Offered when an operation was refused for permissions. `true` means "retry it elevated".
pub trait ElevationLauncher: Send + Sync {
    fn offer(&self, what: &str) -> bool;
}

/// A yes/no question with a confirm button label.
pub trait UserConfirm: Send + Sync {
    fn ask(&self, message: &str, title: &str, confirm_label: &str) -> bool;
}

/// A message the user has to be told.
pub trait UserNotice: Send + Sync {
    fn say(&self, message: &str, title: &str);
}

/// How to settle clashes in a transfer, keyed by `PathKey::canonicalize` of the source path, or
/// `None` when the user cancelled.
pub trait AppConflictPrompt: Send + Sync {
    fn ask(&self, plan: &TransferPlan) -> Option<Resolutions>;
}

/// What Windows looks like. The host pushes it (`set`) when the OS says it changed; the harness
/// poses it. Reading never touches the OS from here, so a scripted run cannot inherit the setting of
/// whichever machine took the screenshots.
pub trait SystemAppearanceSource: Send + Sync {
    fn current(&self) -> SystemAppearance;

    /// Records a change the host observed.
    fn set(&self, appearance: SystemAppearance);
}

/// The appearance the host last pushed; dark until it says otherwise.
pub struct PushedAppearance(Mutex<SystemAppearance>);

impl Default for PushedAppearance {
    fn default() -> Self {
        PushedAppearance(Mutex::new(SystemAppearance::DARK))
    }
}

impl SystemAppearanceSource for PushedAppearance {
    fn current(&self) -> SystemAppearance {
        *lock(&self.0)
    }

    fn set(&self, appearance: SystemAppearance) {
        *lock(&self.0) = appearance;
    }
}

pub struct Seams {
    pub launcher: Arc<dyn ProcessLauncher>,
    pub elevation: Arc<dyn ElevationLauncher>,
    pub confirm: Arc<dyn UserConfirm>,
    pub notice: Arc<dyn UserNotice>,
    pub conflicts: Arc<dyn AppConflictPrompt>,
    pub appearance: Arc<dyn SystemAppearanceSource>,
    pub index: Arc<dyn MftIndex>,
    /// Runs a retry the user agreed to with administrator rights.
    pub elevated: ElevatedRunner,
}

impl Seams {
    /// The harness set, with the recorders the script's `assert-*` verbs read.
    pub fn harness() -> (Seams, Arc<HarnessProbes>) {
        let probes = Arc::new(HarnessProbes::default());
        let seams = Seams {
            launcher: probes.clone(),
            elevation: probes.clone(),
            confirm: probes.clone(),
            notice: probes.clone(),
            conflicts: probes.clone(),
            appearance: probes.clone(),
            index: Arc::new(NullMftIndex),
            elevated: Arc::new(NoElevation),
        };
        (seams, probes)
    }

    /// What a build with no window to ask through gets: nothing is launched, elevation is declined,
    /// questions are answered "no" and clashes keep both. Used by tests of the layers above.
    pub fn unattended() -> Seams {
        let (mut seams, _) = Seams::harness();
        seams.appearance = Arc::new(PushedAppearance::default());
        seams
    }

    /// The product's: the real launcher, the real elevated-retry runner and the real index client
    /// (which only ever *attaches* at startup; the banner offers the prompt). Questions are still
    /// the unattended answers until [`interactive`](Self::interactive) puts a window behind them.
    pub fn system(data_dir: &std::path::Path) -> Seams {
        let mut seams = Seams::unattended();
        seams.launcher = Arc::new(SystemProcessLauncher::new());
        seams.elevated = Arc::new(bb_shell::SystemElevation::new());
        seams.index = Arc::new(bb_shell::indexing::real_index_client(
            &data_dir.to_string_lossy(),
        ));
        seams
    }

    /// A window that can ask: a clash or an offer of administrator rights parks the transfer
    /// thread until the host answers (`ops_do`), and raises `Model::Conflicts` / `Model::Elevation`
    /// so the host knows to put the question up.
    pub(crate) fn interactive(
        hub: EventHub,
        questions: Arc<Questions>,
        data_dir: &std::path::Path,
    ) -> Seams {
        let mut seams = Seams::system(data_dir);
        let prompts = Arc::new(UiPrompts {
            hub,
            questions,
            ui_thread: std::thread::current().id(),
        });
        seams.conflicts = prompts.clone();
        seams.elevation = prompts.clone();
        seams.confirm = prompts.clone();
        seams.notice = prompts;
        seams
    }
}

struct UiPrompts {
    hub: EventHub,
    questions: Arc<Questions>,
    /// The thread the app was made on, which is the host's: a question asked there could never be
    /// answered, because the answer arrives through that same thread.
    ui_thread: std::thread::ThreadId,
}

impl UserConfirm for UiPrompts {
    fn ask(&self, message: &str, title: &str, confirm_label: &str) -> bool {
        if std::thread::current().id() == self.ui_thread {
            debug_assert!(false, "a confirmation asked on the UI thread: {title}");
            return false;
        }
        self.questions
            .ask_confirm(&self.hub, message, title, confirm_label)
    }
}

impl UserNotice for UiPrompts {
    fn say(&self, message: &str, title: &str) {
        self.questions.tell(&self.hub, message, title);
    }
}

impl AppConflictPrompt for UiPrompts {
    fn ask(&self, plan: &TransferPlan) -> Option<Resolutions> {
        self.questions.ask_conflicts(&self.hub, plan)
    }
}

impl ElevationLauncher for UiPrompts {
    fn offer(&self, what: &str) -> bool {
        let model = serde_json::from_str(what).unwrap_or(serde_json::Value::Null);
        self.questions.ask_elevation(&self.hub, model)
    }
}

/// Everything the harness fakes, in one object, so a script can pose an answer and read back what
/// the app tried.
pub struct HarnessProbes {
    launches: Mutex<Vec<(String, bool)>>,
    elevation_offers: Mutex<Vec<String>>,
    confirms: Mutex<Vec<String>>,
    confirm_answer: Mutex<bool>,
    notices: Mutex<Vec<String>>,
    conflict_answers: Mutex<HashMap<String, ConflictResolution>>,
    conflict_cancel: Mutex<bool>,
    conflict_asks: Mutex<usize>,
    conflict_all: Mutex<Option<ConflictResolution>>,
    conflict_names: Mutex<HashMap<String, ConflictResolution>>,
    conflict_model: Mutex<serde_json::Value>,
    appearance: Mutex<SystemAppearance>,
}

impl Default for HarnessProbes {
    fn default() -> Self {
        HarnessProbes {
            launches: Mutex::default(),
            elevation_offers: Mutex::default(),
            confirms: Mutex::default(),
            confirm_answer: Mutex::new(false),
            notices: Mutex::default(),
            conflict_answers: Mutex::default(),
            conflict_cancel: Mutex::new(false),
            conflict_asks: Mutex::new(0),
            conflict_all: Mutex::new(None),
            conflict_names: Mutex::default(),
            conflict_model: Mutex::new(serde_json::Value::Null),
            appearance: Mutex::new(SystemAppearance::DARK),
        }
    }
}

impl HarnessProbes {
    /// Launch attempts (path, elevated). The launcher refuses every one.
    pub fn launches(&self) -> Vec<(String, bool)> {
        lock(&self.launches).clone()
    }

    pub fn elevation_offers(&self) -> Vec<String> {
        lock(&self.elevation_offers).clone()
    }

    pub fn confirms(&self) -> Vec<String> {
        lock(&self.confirms).clone()
    }

    pub fn notices(&self) -> Vec<String> {
        lock(&self.notices).clone()
    }

    pub fn conflict_asks(&self) -> usize {
        *lock(&self.conflict_asks)
    }

    pub fn set_confirm_answer(&self, yes: bool) {
        *lock(&self.confirm_answer) = yes;
    }

    /// Poses the answer for the clash on `source_path`; unposed clashes take their default.
    pub fn answer_conflict(&self, source_path: &str, resolution: ConflictResolution) {
        if let Ok(key) = PathKey::canonicalize(source_path) {
            lock(&self.conflict_answers).insert(key, resolution);
        }
    }

    pub fn cancel_conflicts(&self, cancel: bool) {
        *lock(&self.conflict_cancel) = cancel;
        if cancel {
            lock(&self.conflict_names).clear();
        }
    }

    /// Answers every clash the same way from now on.
    pub fn answer_all_conflicts(&self, resolution: ConflictResolution) {
        *lock(&self.conflict_all) = Some(resolution);
        lock(&self.conflict_names).clear();
        *lock(&self.conflict_cancel) = false;
    }

    /// Answers clashes by the label the dialog shows (`photos\2024\a.jpg`, `/` or `\`) or by
    /// leaf name; what is not named keeps its default.
    pub fn answer_named_conflicts(&self, answers: HashMap<String, ConflictResolution>) {
        *lock(&self.conflict_names) = answers
            .into_iter()
            .map(|(k, v)| (crate::ops_ui::label_key(&k), v))
            .collect();
        *lock(&self.conflict_all) = None;
        *lock(&self.conflict_cancel) = false;
    }

    /// The dialog's model for the last plan that was asked about (or `conflict-plan`ned).
    pub fn last_conflict_model(&self) -> serde_json::Value {
        lock(&self.conflict_model).clone()
    }

    /// Notes a plan as the last one offered without asking.
    pub fn offer_conflicts(&self, plan: &TransferPlan) {
        *lock(&self.conflict_model) = crate::ops_ui::conflict_model(plan, "Ctrl+Z");
    }

    pub fn set_appearance(&self, appearance: SystemAppearance) {
        *lock(&self.appearance) = appearance;
    }
}

impl ProcessLauncher for HarnessProbes {
    fn launch(&self, path: &str, elevated: bool) -> Result<(), String> {
        lock(&self.launches).push((path.to_string(), elevated));
        Err("it was not launched: launching is refused in this run".to_string())
    }
}

impl ElevationLauncher for HarnessProbes {
    fn offer(&self, what: &str) -> bool {
        lock(&self.elevation_offers).push(what.to_string());
        false
    }
}

impl UserConfirm for HarnessProbes {
    fn ask(&self, message: &str, _title: &str, _confirm_label: &str) -> bool {
        lock(&self.confirms).push(message.to_string());
        *lock(&self.confirm_answer)
    }
}

impl UserNotice for HarnessProbes {
    fn say(&self, message: &str, _title: &str) {
        lock(&self.notices).push(message.to_string());
    }
}

impl AppConflictPrompt for HarnessProbes {
    fn ask(&self, plan: &TransferPlan) -> Option<Resolutions> {
        *lock(&self.conflict_asks) += 1;
        self.offer_conflicts(plan);
        if *lock(&self.conflict_cancel) {
            return None;
        }
        let posed = lock(&self.conflict_answers);
        let named = lock(&self.conflict_names);
        let all = *lock(&self.conflict_all);
        Some(
            plan.conflicts()
                .into_iter()
                .filter_map(|t| {
                    let key = PathKey::canonicalize(&t.source_path).ok()?;
                    let by_name = named
                        .get(&crate::ops_ui::label_key(&plan.label_for(t)))
                        .or_else(|| named.get(&crate::ops_ui::label_key(t.name())))
                        .copied();
                    let answer = by_name
                        .or_else(|| posed.get(&key).copied())
                        .or(all)
                        .unwrap_or_else(|| ConflictDefaults::for_transfer(t));
                    Some((key, answer))
                })
                .collect(),
        )
    }
}

impl SystemAppearanceSource for HarnessProbes {
    fn current(&self) -> SystemAppearance {
        *lock(&self.appearance)
    }

    fn set(&self, appearance: SystemAppearance) {
        *lock(&self.appearance) = appearance;
    }
}
