//! Every seam the harness must fake, chosen once at construction. Where the C# view models awaited
//! `IProcessLauncher`, `IElevationLauncher`, `IUserConfirm`, `IUserNotice`, `IConflictPrompt`,
//! `IMftIndexService` or `ISystemAppearance`, the app calls one of these. The harness set refuses to
//! launch anything, declines elevation, records every prompt and answers from a script.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bb_core::mft::{MftIndex, NullMftIndex};
use bb_core::theming::SystemAppearance;
use bb_core::transfer::{ConflictDefaults, ConflictResolution, Resolutions, TransferPlan};
use bb_paths::PathKey;

use crate::events::EventHub;
use crate::ops_ui::Questions;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The one place a process is started on the user's behalf. `Err` carries the words for the status
/// line.
pub trait ProcessLauncher: Send + Sync {
    fn launch(&self, path: &str, elevated: bool) -> Result<(), String>;
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
        };
        (seams, probes)
    }

    /// What a build with no window to ask through gets for now: nothing is launched, elevation is
    /// declined, questions are answered "no" and clashes keep both. The native UI replaces these
    /// with event-raising ones (`NeedsAnswer`) when the dialogs land.
    pub fn unattended() -> Seams {
        let (mut seams, _) = Seams::harness();
        seams.appearance = Arc::new(PushedAppearance::default());
        seams
    }

    /// A window that can ask: a clash or an offer of administrator rights parks the transfer
    /// thread until the host answers (`ops_do`), and raises `Model::Conflicts` / `Model::Elevation`
    /// so the host knows to put the question up.
    pub(crate) fn interactive(hub: EventHub, questions: Arc<Questions>) -> Seams {
        let mut seams = Seams::unattended();
        let prompts = Arc::new(UiPrompts { hub, questions });
        seams.conflicts = prompts.clone();
        seams.elevation = prompts;
        seams
    }
}

struct UiPrompts {
    hub: EventHub,
    questions: Arc<Questions>,
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
