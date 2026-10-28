//! The slice of the transfer and delete areas a sync needs, named as the C# names them so the
//! orchestrator can swap each `use` line for the real module. Spec:
//! Core/Services/Transfer/TransferModels.cs, Core/Services/Delete/DeleteModels.cs,
//! TransferExecutor.cs, DeleteExecutor.cs, TransferRedo.cs, DeleteRedo.cs.
//!
//! The planners and executors are reached through traits (`TransferPlanning`, `DeletePlanning`,
//! `SyncTransfers`, `SyncDeletes`) so the sync runner is testable with fakes and the real
//! executors, ported in parallel, slot in behind them.

use std::collections::HashMap;

use bb_paths::{file_name, parent_key};

use crate::cancel::CancelToken;

// TODO(merge): replaced by crate::transfer::TransferVerb
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferVerb {
    Move,
    Copy,
}

// TODO(merge): replaced by crate::transfer::ConflictResolution
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConflictResolution {
    Skip,
    KeepBoth,
    Replace,
    /// Take over the existing name, staging what was there. Only a sync or a merged drop/paste
    /// may ask for it, because they keep the record that lets the displaced entry come back.
    Overwrite,
}

// TODO(merge): replaced by crate::transfer::PlannedTransfer
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedTransfer {
    pub source_path: String,
    pub is_directory: bool,
    pub destination_path: String,
    /// True when `destination_path` is already taken.
    pub conflicts: bool,
}

impl PlannedTransfer {
    pub fn name(&self) -> &str {
        file_name(&self.source_path)
    }
}

// TODO(merge): replaced by crate::transfer::RejectedTransfer (carries a TransferRejection too)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedTransfer {
    pub source_path: String,
    pub message: String,
    /// True for refusals that are ordinary no-ops rather than something to report.
    pub is_benign: bool,
}

// TODO(merge): replaced by crate::transfer::TransferPlan
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferPlan {
    pub verb: TransferVerb,
    pub destination_directory: String,
    pub transfers: Vec<PlannedTransfer>,
    pub rejected: Vec<RejectedTransfer>,
}

impl TransferPlan {
    pub fn empty(verb: TransferVerb, destination: &str) -> TransferPlan {
        TransferPlan {
            verb,
            destination_directory: destination.to_string(),
            transfers: Vec::new(),
            rejected: Vec::new(),
        }
    }

    pub fn has_work(&self) -> bool {
        !self.transfers.is_empty()
    }

    /// Refusals worth surfacing; no-ops are filtered out.
    pub fn problems(&self) -> impl Iterator<Item = &RejectedTransfer> {
        self.rejected.iter().filter(|r| !r.is_benign)
    }
}

// TODO(merge): replaced by crate::transfer::CompletedTransfer (which also carries `Written`)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedTransfer {
    /// Where it came from: the undo target.
    pub source_path: String,
    /// Where it ended up, after any conflict resolution.
    pub final_path: String,
    pub is_directory: bool,
    /// When the transfer replaced an existing entry, the staging path that entry was moved aside
    /// to; None otherwise. Undo restores from here.
    pub displaced_stage_path: Option<String>,
}

// TODO(merge): replaced by crate::transfer::FailedTransfer
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedTransfer {
    pub source_path: String,
    pub message: String,
    pub access_denied: bool,
}

// TODO(merge): replaced by crate::transfer::TransferOutcome
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferOutcome {
    pub verb: TransferVerb,
    pub destination_directory: String,
    pub completed: Vec<CompletedTransfer>,
    pub skipped: Vec<String>,
    pub failed: Vec<FailedTransfer>,
    /// Every staging folder this run created, so the whole lot can be discarded in one go once the
    /// transfer can no longer be undone.
    pub staging_directories: Vec<String>,
    pub cancelled: bool,
}

impl TransferOutcome {
    /// Only a move is worth undoing through `Undo`; a copy's undo is `undo_copies`, and a caller
    /// holding a sync outcome is the only one allowed to ask for it.
    pub fn can_undo(&self) -> bool {
        self.verb == TransferVerb::Move && !self.completed.is_empty()
    }
}

// TODO(merge): replaced by crate::transfer::TransferProgress
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferProgress {
    pub done: usize,
    pub total: usize,
    pub current_name: String,
    pub bytes_done: i64,
    pub current_item_bytes: i64,
    pub current_item_total: i64,
}

// TODO(merge): replaced by crate::transfer::TransferUndoResult
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferUndoResult {
    pub restored: usize,
    pub failed: Vec<FailedTransfer>,
    /// Exactly the items that went back, so a redo acts on those and nothing else.
    pub reverted: Vec<CompletedTransfer>,
}

// TODO(merge): replaced by crate::transfer::TransferPlanner (`plan`)
pub trait TransferPlanning {
    fn plan(&self, sources: &[String], destination: &str, verb: TransferVerb) -> TransferPlan;
}

/// The executor as a sync drives it: `TransferExecutor.Execute`, `UndoCopies`, `CommitStaging`
/// and `TransferRedo.PlanFor`.
// TODO(merge): replaced by crate::transfer::TransferExecutor and crate::transfer::TransferRedo
pub trait SyncTransfers {
    fn execute(
        &self,
        plan: &TransferPlan,
        resolutions: &HashMap<String, ConflictResolution>,
        cancel: &CancelToken,
        progress: Option<&dyn Fn(&TransferProgress)>,
    ) -> TransferOutcome;

    fn undo_copies(&self, outcome: &TransferOutcome, cancel: &CancelToken) -> TransferUndoResult;

    fn commit_staging(&self, outcome: &TransferOutcome);

    /// `TransferRedo.PlanFor`: the plan that puts `completed` back where it went, with the
    /// resolutions that displace only what was displaced before.
    fn redo_plan(
        &self,
        verb: TransferVerb,
        destination: &str,
        completed: &[CompletedTransfer],
    ) -> (TransferPlan, HashMap<String, ConflictResolution>);
}

// TODO(merge): replaced by crate::delete::DeleteMode
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeleteMode {
    Recycle,
    Staged,
    Permanent,
}

// TODO(merge): replaced by crate::delete::DeleteSource
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteSource {
    pub path: String,
    pub is_directory: bool,
}

impl DeleteSource {
    pub fn name(&self) -> &str {
        file_name(&self.path)
    }
}

// TODO(merge): replaced by crate::delete::PlannedDelete (which also carries a DeleteDisposition)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedDelete {
    pub source_path: String,
    pub is_directory: bool,
}

impl PlannedDelete {
    pub fn name(&self) -> &str {
        file_name(&self.source_path)
    }

    /// The folder the item will vanish from.
    pub fn parent_path(&self) -> String {
        parent_key(&self.source_path).unwrap_or_default()
    }
}

// TODO(merge): replaced by crate::delete::RejectedDelete (carries a DeleteRejection too)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedDelete {
    pub source_path: String,
    pub message: String,
    pub is_benign: bool,
}

// TODO(merge): replaced by crate::delete::DeletePlan
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletePlan {
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
    pub fn permanent(&self) -> bool {
        self.mode == DeleteMode::Permanent
    }

    pub fn has_work(&self) -> bool {
        !self.deletions.is_empty()
    }

    pub fn problems(&self) -> impl Iterator<Item = &RejectedDelete> {
        self.rejected.iter().filter(|r| !r.is_benign)
    }
}

// TODO(merge): replaced by crate::delete::DeletedItem
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletedItem {
    pub source_path: String,
    pub is_directory: bool,
    /// Where it is being held until the delete is committed, or None when it was erased outright
    /// or sent to the Recycle Bin.
    pub staged_path: Option<String>,
    /// The item's `$R` path inside the Recycle Bin, when that is where it went.
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

// TODO(merge): replaced by crate::delete::FailedDelete
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedDelete {
    pub source_path: String,
    pub message: String,
    pub access_denied: bool,
}

// TODO(merge): replaced by crate::delete::DeleteOutcome
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteOutcome {
    pub permanent: bool,
    pub deleted: Vec<DeletedItem>,
    pub failed: Vec<FailedDelete>,
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

    /// Only a delete that still has the data somewhere can be reversed. A permanent one erased
    /// it; so did the Recycle Bin for any item it declined to hold, which is why this asks each
    /// item rather than trusting the mode.
    pub fn can_undo(&self) -> bool {
        !self.permanent && self.deleted.iter().any(DeletedItem::is_recoverable)
    }
}

// TODO(merge): replaced by crate::delete::DeleteUndoResult
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteUndoResult {
    pub restored: usize,
    pub failed: Vec<FailedDelete>,
    /// Exactly the items that came back, so a redo deletes those and nothing else.
    pub reverted: Vec<DeletedItem>,
}

// TODO(merge): replaced by crate::delete::DeleteProgress
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteProgress {
    pub done: usize,
    pub total: usize,
    pub current_name: String,
}

// TODO(merge): replaced by crate::delete::DeletePlanner (`plan`)
pub trait DeletePlanning {
    fn plan(&self, sources: &[DeleteSource], mode: DeleteMode) -> DeletePlan;
}

/// The delete executor as a sync drives it: `DeleteExecutor.Execute`, `Undo`, `CommitStaging` and
/// `DeleteRedo.PlanFor`.
// TODO(merge): replaced by crate::delete::DeleteExecutor and crate::delete::DeleteRedo
pub trait SyncDeletes {
    fn execute(
        &self,
        plan: &DeletePlan,
        cancel: &CancelToken,
        progress: Option<&dyn Fn(&DeleteProgress)>,
    ) -> DeleteOutcome;

    fn undo(&self, outcome: &DeleteOutcome, cancel: &CancelToken) -> DeleteUndoResult;

    fn commit_staging(&self, outcome: &DeleteOutcome);

    /// `DeleteRedo.PlanFor`: a plan that deletes exactly what an undo put back.
    fn redo_plan(&self, reverted: &[DeletedItem]) -> DeletePlan;
}
