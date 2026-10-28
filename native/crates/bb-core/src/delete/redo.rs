//! The plan that deletes again what an undo put back. Spec: Core/Services/Delete/DeleteRedo.cs.
//!
//! Each item goes the way it went the first time: back to the Recycle Bin if that is where it was,
//! otherwise into the holding folder. The executor still re-asks whether the bin will take it, and
//! falls back to holding rather than erasing: a redo is never the thing that makes a delete
//! permanent.

use super::models::{DeleteDisposition, DeleteMode, DeletePlan, DeletedItem, PlannedDelete};

pub struct DeleteRedo;

impl DeleteRedo {
    pub fn plan_for(reverted: &[DeletedItem]) -> DeletePlan {
        // The undo walked them in reverse; the redo goes in the order they were first deleted.
        let deletions: Vec<PlannedDelete> = reverted
            .iter()
            .rev()
            .map(|d| {
                PlannedDelete::new(&d.source_path, d.is_directory).with_disposition(
                    if d.recycled_path.is_some() {
                        DeleteDisposition::Recycle
                    } else {
                        DeleteDisposition::Stage
                    },
                )
            })
            .collect();

        let mode = if deletions
            .iter()
            .any(|d| d.disposition == DeleteDisposition::Recycle)
        {
            DeleteMode::Recycle
        } else {
            DeleteMode::Staged
        };
        DeletePlan {
            mode,
            deletions,
            rejected: Vec::new(),
        }
    }
}
