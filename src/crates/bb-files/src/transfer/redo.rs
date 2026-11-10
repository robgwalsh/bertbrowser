//! The plan that does an undone transfer again, built from what the undo actually put back.
//! Spec: Core/Services/Transfer/TransferRedo.cs.
//!
//! **Every item lands exactly where it landed the first time.** Each one carries its old final path
//! as its destination, so a Keep-both that produced `img (2).jpg` produces it again rather than a
//! fresh number, and an expanded merge's items land deep in the tree rather than at the top.
//!
//! **Only a name the first run displaced may be displaced again**: Replace for a move, Overwrite
//! for a copy, since that is what the undo just put back there. Everything else is Skip. A redo
//! never numbers a newcomer and never displaces something it has no record of: a name taken since
//! the undo is reported and left alone, because nobody chose to replace it.

use super::models::{
    CompletedTransfer, ConflictResolution, PlannedTransfer, Resolutions, TransferPlan,
    TransferVerb, key_of,
};

pub struct TransferRedo;

impl TransferRedo {
    pub fn plan_for(
        verb: TransferVerb,
        destination_directory: &str,
        reverted: &[CompletedTransfer],
        pruned_directories: &[String],
    ) -> (TransferPlan, Resolutions) {
        let mut transfers = Vec::with_capacity(reverted.len());
        let mut resolutions = Resolutions::new();
        let displace = match verb {
            TransferVerb::Move => ConflictResolution::Replace,
            TransferVerb::Copy => ConflictResolution::Overwrite,
        };

        // The undo walked them in reverse, so its list is newest-first; the redo wants the original
        // order back, so a folder is written before anything that was copied into it.
        for item in reverted.iter().rev() {
            let displaced = item.displaced_stage_path.is_some();
            transfers.push(PlannedTransfer::new(
                &item.source_path,
                item.is_directory,
                &item.final_path,
                displaced,
            ));
            resolutions.insert(
                key_of(&item.source_path),
                if displaced {
                    displace
                } else {
                    ConflictResolution::Skip
                },
            );
        }

        let mut plan = TransferPlan::new(verb, destination_directory, transfers, Vec::new());
        if verb == TransferVerb::Move {
            plan.prune_directories = pruned_directories.to_vec();
        }
        (plan, resolutions)
    }
}
