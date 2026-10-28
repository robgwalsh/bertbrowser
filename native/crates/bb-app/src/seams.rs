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

pub trait SystemAppearanceSource: Send + Sync {
    fn current(&self) -> SystemAppearance;
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
        let (seams, _) = Seams::harness();
        seams
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
        if *lock(&self.conflict_cancel) {
            return None;
        }
        let posed = lock(&self.conflict_answers);
        Some(
            plan.conflicts()
                .into_iter()
                .filter_map(|t| {
                    let key = PathKey::canonicalize(&t.source_path).ok()?;
                    let answer = posed
                        .get(&key)
                        .copied()
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
}
