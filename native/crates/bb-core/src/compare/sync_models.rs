//! What a sync would do, and what it did. Spec: Core/Services/Compare/SyncModels.cs.

use std::collections::HashMap;

use bb_paths::file_name;

use super::models::CompareVerdict;
use super::transfer_stubs::{
    ConflictResolution, DeleteOutcome, DeletePlan, FailedTransfer, TransferOutcome, TransferPlan,
};

/// What a sync would do about one entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyncActionKind {
    /// Write it to a name the right side does not use.
    Copy,
    /// Write it over what the right side has, which goes to staging first.
    Overwrite,
    /// Remove it from the right side. The only kind that takes anything away.
    Delete,
}

/// One thing a sync would do, as the user is shown it before agreeing to any of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncAction {
    pub relative_key: String,
    /// Where it sits under the two roots, in the casing the side that has it recorded.
    pub relative_display: String,
    pub kind: SyncActionKind,
    /// The left-side path to copy from; empty for a delete.
    pub source_path: String,
    /// The right-side path being written or removed.
    pub target_path: String,
    pub is_directory: bool,
    /// What it weighs, or None when nothing measured it. None is *unknown* and must render blank:
    /// never as zero, and never quietly summed into a total.
    pub bytes: Option<i64>,
    pub verdict: CompareVerdict,
    /// Whether it runs. A folder carries every one of its own entries, so unticking a folder
    /// unticks everything under it.
    pub ticked: bool,
}

impl SyncAction {
    pub fn name(&self) -> &str {
        file_name(&self.target_path)
    }
}

/// Everything a sync would do, and what it deliberately would not. `unknown_count` is the entries
/// no verdict could be reached for: they produce no action at all (you cannot sync what you could
/// not compare) and are surfaced as a caveat rather than silently left out of a list that claims
/// to be complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncPreview {
    pub left_path: String,
    pub right_path: String,
    pub actions: Vec<SyncAction>,
    pub unknown_count: usize,
}

impl SyncPreview {
    pub fn empty(left: &str, right: &str) -> SyncPreview {
        SyncPreview {
            left_path: left.to_string(),
            right_path: right.to_string(),
            actions: Vec::new(),
            unknown_count: 0,
        }
    }

    pub fn ticked(&self) -> Vec<&SyncAction> {
        self.actions.iter().filter(|a| a.ticked).collect()
    }

    fn ticked_of_kind(&self, kind: SyncActionKind) -> usize {
        self.actions
            .iter()
            .filter(|a| a.ticked && a.kind == kind)
            .count()
    }

    pub fn copy_count(&self) -> usize {
        self.ticked_of_kind(SyncActionKind::Copy)
    }

    pub fn overwrite_count(&self) -> usize {
        self.ticked_of_kind(SyncActionKind::Overwrite)
    }

    pub fn delete_count(&self) -> usize {
        self.ticked_of_kind(SyncActionKind::Delete)
    }

    pub fn has_work(&self) -> bool {
        self.actions.iter().any(|a| a.ticked)
    }

    /// What the ticked copies weigh, or None when any one of them was never measured. Withheld
    /// rather than approximated, the same rule the rest of the app follows for a size it does not
    /// have: a total that silently omits the files nobody could measure is worse than no total,
    /// because it looks like one.
    pub fn total_bytes(&self) -> Option<i64> {
        let mut total = 0;
        for action in &self.actions {
            if !action.ticked || action.kind == SyncActionKind::Delete {
                continue;
            }
            total += action.bytes?;
        }
        Some(total)
    }
}

/// A preview turned into the plans the existing executors already know how to carry out.
///
/// `copies` is one plan per destination folder: a `TransferPlan` carries a single destination by
/// construction and a recursive sync writes into many, so the alternative would be a second kind
/// of transfer plan. This way every one of the transfer planner's rules stays in force, per
/// destination, unchanged. `resolutions` is keyed by the canonical source path, as the executor
/// expects. `refused` is anything a planner turned down, kept rather than dropped: a sync that
/// silently does less than it showed is the failure worth reporting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncPlans {
    pub copies: Vec<TransferPlan>,
    pub resolutions: HashMap<String, ConflictResolution>,
    pub removals: DeletePlan,
    pub refused: Vec<String>,
}

impl SyncPlans {
    pub fn item_count(&self) -> usize {
        self.copies.iter().map(|c| c.transfers.len()).sum::<usize>() + self.removals.deletions.len()
    }

    pub fn has_work(&self) -> bool {
        self.item_count() > 0
    }
}

/// What a sync did, as the two kinds of record the existing executors produce. It holds a list
/// of copy outcomes for the same reason it holds a list of plans: one destination folder each.
/// Undo walks them in reverse, so a folder is never removed before what was written inside it.
/// This is what the undo history's `SyncRecord` keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncOutcome {
    pub copies: Vec<TransferOutcome>,
    pub removals: DeleteOutcome,
    pub cancelled: bool,
}

impl SyncOutcome {
    pub fn empty() -> SyncOutcome {
        SyncOutcome {
            copies: Vec::new(),
            removals: DeleteOutcome::empty(false),
            cancelled: false,
        }
    }

    pub fn copied_count(&self) -> usize {
        self.copies
            .iter()
            .flat_map(|c| &c.completed)
            .filter(|t| t.displaced_stage_path.is_none())
            .count()
    }

    pub fn replaced_count(&self) -> usize {
        self.copies
            .iter()
            .flat_map(|c| &c.completed)
            .filter(|t| t.displaced_stage_path.is_some())
            .count()
    }

    pub fn removed_count(&self) -> usize {
        self.removals.deleted.len()
    }

    pub fn failed_copies(&self) -> Vec<&FailedTransfer> {
        self.copies.iter().flat_map(|c| &c.failed).collect()
    }

    /// Whether the whole run can be put back: the copies through the executor's copy-undo, the
    /// removals through the delete executor's. A run that did nothing is not undoable, and
    /// neither is one whose removals were erased outright, which is why the delete half asks its
    /// own outcome rather than trusting the mode it was given.
    pub fn can_undo(&self) -> bool {
        self.copies.iter().any(|c| !c.completed.is_empty()) || self.removals.can_undo()
    }
}
