//! What a delete is asked for, what the planner decided, and what the executor did.
//! Spec: Core/Services/Delete/DeleteModels.cs.

use bb_paths::{file_name, parent_key};

/// What the user asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteMode {
    /// The ordinary delete: send the items to the Windows Recycle Bin, where they stay — visible
    /// and restorable in Explorer — until the user empties it.
    Recycle,
    /// Move the items into this app's own holding folder instead. The fallback for volumes with
    /// no working Recycle Bin, and still undoable for exactly one operation.
    Staged,
    /// Shift+Delete: erase in place, hold nothing, and there is no undo.
    Permanent,
}

/// What will actually happen to one item, which is not always what the user asked for: a volume
/// with no Recycle Bin — a network share, removable media with the bin turned off — takes the
/// staged route instead. The planner decides this, so the confirmation can say what is really
/// about to happen rather than what was requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteDisposition {
    /// Into the Windows Recycle Bin.
    Recycle,
    /// Into this app's holding folder.
    Stage,
    /// Erased in place.
    Erase,
}

/// One item the user asked to delete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteSource {
    /// Full path of the item.
    pub path: String,
    /// What the caller believes it is. The planner asks disk rather than trusting this, but it
    /// keeps the call site honest about what was selected.
    pub is_directory: bool,
}

impl DeleteSource {
    pub fn new(path: &str, is_directory: bool) -> DeleteSource {
        DeleteSource {
            path: path.to_string(),
            is_directory,
        }
    }

    pub fn name(&self) -> &str {
        file_name(&self.path)
    }
}

/// Why the planner refused to delete one item. Everything except `InsideADeletedFolder` is worth
/// telling the user about before anything moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteRejection {
    /// The item disappeared between selection and confirmation.
    SourceMissing,
    /// Drive and volume roots are not deletable, whatever the user selected.
    SourceIsRoot,
    /// A location the app refuses to delete outright — the Windows folder, Program Files, the
    /// user profile root. Windows stops the system ones itself now, but the profile root is
    /// entirely writable by its owner and nothing else would be in the way.
    ProtectedLocation,
    /// A folder above this item is being deleted too, so it goes with the ancestor. Reachable
    /// from a flattened search result, where a selection can hold both a folder and something
    /// inside it. An ordinary no-op, not a problem.
    InsideADeletedFolder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedDelete {
    /// The item the planner refused.
    pub source_path: String,
    pub reason: DeleteRejection,
    /// User-facing explanation.
    pub message: String,
}

impl RejectedDelete {
    /// True for refusals that are ordinary no-ops rather than something to report.
    pub fn is_benign(&self) -> bool {
        self.reason == DeleteRejection::InsideADeletedFolder
    }
}

/// One item the planner accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedDelete {
    pub source_path: String,
    pub is_directory: bool,
    /// Where this particular item is going. `Stage` by default, which is the behaviour that
    /// predates the Recycle Bin and the safe answer for any caller that has not thought about it.
    pub disposition: DeleteDisposition,
}

impl PlannedDelete {
    pub fn new(source_path: &str, is_directory: bool) -> PlannedDelete {
        PlannedDelete {
            source_path: source_path.to_string(),
            is_directory,
            disposition: DeleteDisposition::Stage,
        }
    }

    pub fn with_disposition(mut self, disposition: DeleteDisposition) -> PlannedDelete {
        self.disposition = disposition;
        self
    }

    pub fn name(&self) -> &str {
        file_name(&self.source_path)
    }

    /// The folder the item will vanish from — what a listing has to reload afterwards.
    pub fn parent_path(&self) -> String {
        parent_key(&self.source_path).unwrap_or_default()
    }
}

/// The validated answer to "what would deleting these do?".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletePlan {
    /// What the user asked for. Individual items may still differ — see
    /// [`PlannedDelete::disposition`].
    pub mode: DeleteMode,
    pub deletions: Vec<PlannedDelete>,
    pub rejected: Vec<RejectedDelete>,
}

impl DeletePlan {
    pub fn empty(mode: DeleteMode) -> DeletePlan {
        DeletePlan {
            mode,
            deletions: Vec::new(),
            rejected: Vec::new(),
        }
    }

    /// A Shift+Delete: the items are erased instead of being set aside, and there is no undo.
    /// What the dialog and the executor's erase branch actually care about.
    pub fn permanent(&self) -> bool {
        self.mode == DeleteMode::Permanent
    }

    pub fn has_work(&self) -> bool {
        !self.deletions.is_empty()
    }

    /// Refusals worth surfacing; no-ops are filtered out.
    pub fn problems(&self) -> Vec<&RejectedDelete> {
        self.rejected.iter().filter(|r| !r.is_benign()).collect()
    }

    /// True when the user asked for the Recycle Bin but at least one item cannot go there and will
    /// be held by this app instead. The confirmation says so rather than letting the difference be
    /// discovered later.
    pub fn has_staged_fallback(&self) -> bool {
        self.mode == DeleteMode::Recycle
            && self
                .deletions
                .iter()
                .any(|d| d.disposition == DeleteDisposition::Stage)
    }
}

/// How much one planned item actually amounts to, so the confirmation can say what is about to
/// go rather than just how many rows were selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteMeasurement {
    pub source_path: String,
    pub is_directory: bool,
    pub bytes: u64,
    pub files: usize,
    pub directories: usize,
    /// True when part of the tree could not be read, so the totals are a floor rather than the
    /// answer.
    pub incomplete: bool,
}

/// Every measurement for a plan, plus the totals the dialog leads with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeleteSurvey {
    pub items: Vec<DeleteMeasurement>,
}

impl DeleteSurvey {
    pub fn empty() -> DeleteSurvey {
        DeleteSurvey::default()
    }

    pub fn bytes(&self) -> u64 {
        self.items.iter().map(|i| i.bytes).sum()
    }

    pub fn files(&self) -> usize {
        self.items.iter().map(|i| i.files).sum()
    }

    pub fn directories(&self) -> usize {
        self.items.iter().map(|i| i.directories).sum()
    }

    pub fn incomplete(&self) -> bool {
        self.items.iter().any(|i| i.incomplete)
    }
}

/// One item that is no longer where it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletedItem {
    /// Where it was — the undo target.
    pub source_path: String,
    pub is_directory: bool,
    /// Where it is being held until the delete is committed, or None when it was erased outright
    /// or sent to the Recycle Bin. Undo moves it back from here.
    pub staged_path: Option<String>,
    /// The item's `$R` path inside the Recycle Bin, when that is where it went. Undo restores from
    /// exactly this rather than searching the bin for something with a matching original path,
    /// which is what keeps it correct when the same path has been deleted more than once.
    pub recycled_path: Option<String>,
}

impl DeletedItem {
    pub fn name(&self) -> &str {
        file_name(&self.source_path)
    }

    /// True when the item still exists somewhere and can be put back.
    pub fn is_recoverable(&self) -> bool {
        self.staged_path.is_some() || self.recycled_path.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedDelete {
    /// The item that could not be deleted.
    pub source_path: String,
    /// The failure, phrased for the status bar.
    pub message: String,
    /// Windows refused permission, rather than the item being missing or in use. The only failure
    /// an administrator token could fix.
    pub access_denied: bool,
}

impl FailedDelete {
    pub fn new(source_path: &str, message: String) -> FailedDelete {
        FailedDelete {
            source_path: source_path.to_string(),
            message,
            access_denied: false,
        }
    }
}

/// What actually happened on disk. `deleted` doubles as the undo record: each entry knows where
/// its data is being held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteOutcome {
    pub permanent: bool,
    pub deleted: Vec<DeletedItem>,
    pub failed: Vec<FailedDelete>,
    /// Every holding folder this run created, so the whole lot can be discarded in one go once the
    /// delete can no longer be undone.
    pub staging_directories: Vec<String>,
}

impl DeleteOutcome {
    pub fn empty(permanent: bool) -> DeleteOutcome {
        DeleteOutcome {
            permanent,
            deleted: Vec::new(),
            failed: Vec::new(),
            staging_directories: Vec::new(),
        }
    }

    /// Only a delete that still has the data somewhere can be reversed. A permanent one erased it;
    /// so did the Recycle Bin for any item it declined to hold, which is why this asks each item
    /// rather than trusting the mode.
    pub fn can_undo(&self) -> bool {
        !self.permanent && self.deleted.iter().any(|d| d.is_recoverable())
    }
}

/// Restoring a delete: how many items came back, and what could not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeleteUndoResult {
    pub restored: usize,
    pub failed: Vec<FailedDelete>,
    /// Exactly the items that came back, so a redo deletes those and nothing else.
    pub reverted: Vec<DeletedItem>,
}

/// Progress for the status bar while a delete runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteProgress {
    pub done: usize,
    pub total: usize,
    pub current_name: String,
}
