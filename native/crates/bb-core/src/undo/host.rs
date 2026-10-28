//! What a record needs from the app to take a step: the executors, the elevated retry, and a
//! progress surface for a redo that writes bytes. Spec: Core/Services/UndoHistory/IUndoHost.cs.
//!
//! **The elevated retry stays in the app**, where the prompt and the helper process live, and comes
//! in through here. There is deliberately no member for retrying the undo of a *copy*:
//! `ElevationHost::undo_transfer` hardcodes a move, so handed a paste's outcome it would move the
//! pasted files back into the folder they came from instead of removing them, corrupting both
//! sides. [`UndoHost::elevate_move_undo`] refuses anything but a move for the same reason.

use super::archive_edit_record::{ArchiveEdits, UnportedArchiveEdits};
use super::metadata_edit_record::{MetadataEdits, UnportedMetadataEdits};
use crate::compare::SyncRunner;
use crate::delete::{DeleteExecutor, DeleteOutcome, DeletePlan, DeleteUndoResult};
use crate::elevation::Elevated;
use crate::rename::{RenameExecutor, RenameOutcome, RenamePlan};
use crate::timestamps::TimestampExecutor;
use crate::transfer::{
    Resolutions, TransferExecutor, TransferOutcome, TransferPlan, TransferProgress,
    TransferUndoResult, TransferVerb,
};

/// A progress sink a redo writes through while it runs.
pub type ProgressSink<'a> = Box<dyn FnMut(TransferProgress) + 'a>;

pub trait UndoHost {
    fn transfers(&self) -> &TransferExecutor;

    fn deletes(&self) -> &DeleteExecutor<'_>;

    fn renames(&self) -> &RenameExecutor;

    fn archive_edits(&self) -> &dyn ArchiveEdits;

    /// No elevated retry goes with this one, for the reason archives have none.
    fn metadata_edits(&self) -> &dyn MetadataEdits;

    fn timestamps(&self) -> &TimestampExecutor<'_>;

    fn sync(&self) -> SyncRunner<'_>;

    fn elevate_transfer(
        &self,
        plan: &TransferPlan,
        outcome: TransferOutcome,
        resolutions: &Resolutions,
    ) -> Elevated<TransferOutcome>;

    fn elevate_delete(&self, plan: &DeletePlan, outcome: DeleteOutcome) -> Elevated<DeleteOutcome>;

    fn elevate_rename(&self, plan: &RenamePlan, outcome: RenameOutcome) -> Elevated<RenameOutcome>;

    /// The retry for putting a move back. Panics for anything but a move.
    fn elevate_move_undo(
        &self,
        outcome: &TransferOutcome,
        result: TransferUndoResult,
    ) -> Elevated<TransferUndoResult>;

    fn elevate_delete_undo(
        &self,
        outcome: &DeleteOutcome,
        result: DeleteUndoResult,
    ) -> Elevated<DeleteUndoResult>;

    /// Puts a progress surface up for a redo that writes, and returns its sink. Paired with
    /// [`UndoHost::end_progress`].
    fn begin_progress(&self, plan: &TransferPlan, headline: &str) -> Option<ProgressSink<'_>>;

    fn end_progress(&self);
}

/// A host with no elevation and no progress surface: every retry is a pass-through. For tests, and
/// for anything that runs where there is nobody to ask.
pub struct PlainUndoHost<'a> {
    transfers: &'a TransferExecutor,
    deletes: &'a DeleteExecutor<'a>,
    renames: &'a RenameExecutor,
    archive_edits: &'a dyn ArchiveEdits,
    metadata_edits: UnportedMetadataEdits,
    timestamps: TimestampExecutor<'static>,
}

impl<'a> PlainUndoHost<'a> {
    pub fn new(
        transfers: &'a TransferExecutor,
        deletes: &'a DeleteExecutor<'a>,
        renames: &'a RenameExecutor,
        archive_edits: &'a dyn ArchiveEdits,
    ) -> PlainUndoHost<'a> {
        PlainUndoHost {
            transfers,
            deletes,
            renames,
            archive_edits,
            metadata_edits: UnportedMetadataEdits,
            timestamps: TimestampExecutor::local(),
        }
    }

    /// A host over the given executors with no archive editing behind it.
    pub fn without_archives(
        transfers: &'a TransferExecutor,
        deletes: &'a DeleteExecutor<'a>,
        renames: &'a RenameExecutor,
    ) -> PlainUndoHost<'a> {
        Self::new(transfers, deletes, renames, &UnportedArchiveEdits)
    }
}

impl UndoHost for PlainUndoHost<'_> {
    fn transfers(&self) -> &TransferExecutor {
        self.transfers
    }

    fn deletes(&self) -> &DeleteExecutor<'_> {
        self.deletes
    }

    fn renames(&self) -> &RenameExecutor {
        self.renames
    }

    fn archive_edits(&self) -> &dyn ArchiveEdits {
        self.archive_edits
    }

    fn metadata_edits(&self) -> &dyn MetadataEdits {
        &self.metadata_edits
    }

    fn timestamps(&self) -> &TimestampExecutor<'_> {
        &self.timestamps
    }

    fn sync(&self) -> SyncRunner<'_> {
        SyncRunner::new(self.transfers, self.deletes)
    }

    fn elevate_transfer(
        &self,
        _plan: &TransferPlan,
        outcome: TransferOutcome,
        _resolutions: &Resolutions,
    ) -> Elevated<TransferOutcome> {
        Elevated::plain(outcome)
    }

    fn elevate_delete(
        &self,
        _plan: &DeletePlan,
        outcome: DeleteOutcome,
    ) -> Elevated<DeleteOutcome> {
        Elevated::plain(outcome)
    }

    fn elevate_rename(
        &self,
        _plan: &RenamePlan,
        outcome: RenameOutcome,
    ) -> Elevated<RenameOutcome> {
        Elevated::plain(outcome)
    }

    fn elevate_move_undo(
        &self,
        outcome: &TransferOutcome,
        result: TransferUndoResult,
    ) -> Elevated<TransferUndoResult> {
        assert!(
            outcome.verb == TransferVerb::Move,
            "Only a move's undo can be retried elevated."
        );
        Elevated::plain(result)
    }

    fn elevate_delete_undo(
        &self,
        _outcome: &DeleteOutcome,
        result: DeleteUndoResult,
    ) -> Elevated<DeleteUndoResult> {
        Elevated::plain(result)
    }

    fn begin_progress(&self, _plan: &TransferPlan, _headline: &str) -> Option<ProgressSink<'_>> {
        None
    }

    fn end_progress(&self) {}
}
