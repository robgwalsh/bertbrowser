//! The undo history's host in the app: the plain host's executors, plus the one thing it cannot do - offer
//! an elevated second pass when Windows refused part of an undo or a redo for permissions, exactly as the
//! forward operations do (`Inner::elevate`). A step that was only refused for want of administrator rights
//! is otherwise stuck: the history can neither take it back nor do it again. Spec:
//! ShellViewModel.UndoHistory (the host's ElevateAsync / ElevateMoveUndoAsync / ElevateDeleteUndoAsync).

use bb_core::archives::ArchiveEditExecutor;
use bb_core::cancel::CancelToken;
use bb_core::compare::SyncRunner;
use bb_core::delete::{DeleteExecutor, DeleteOutcome, DeletePlan, DeleteUndoResult};
use bb_core::elevation::{Elevated, ElevatedRetry, ElevationOperation};
use bb_core::metadata::MetadataEditExecutor;
use bb_core::rename::{RenameExecutor, RenameOutcome, RenamePlan};
use bb_core::timestamps::TimestampExecutor;
use bb_core::transfer::{
    Resolutions, TransferExecutor, TransferOutcome, TransferPlan, TransferUndoResult, TransferVerb,
};
use bb_core::undo::{PlainUndoHost, ProgressSink, UndoHost};

use crate::app::Inner;

pub(crate) struct AppUndoHost<'a> {
    inner: &'a Inner,
    plain: PlainUndoHost<'a>,
}

impl<'a> AppUndoHost<'a> {
    pub(crate) fn new(inner: &'a Inner, plain: PlainUndoHost<'a>) -> AppUndoHost<'a> {
        AppUndoHost { inner, plain }
    }
}

impl UndoHost for AppUndoHost<'_> {
    fn transfers(&self) -> &TransferExecutor {
        self.plain.transfers()
    }

    fn deletes(&self) -> &DeleteExecutor<'_> {
        self.plain.deletes()
    }

    fn renames(&self) -> &RenameExecutor {
        self.plain.renames()
    }

    fn archive_edits(&self) -> &ArchiveEditExecutor<'_> {
        self.plain.archive_edits()
    }

    fn metadata_edits(&self) -> &MetadataEditExecutor<'_> {
        self.plain.metadata_edits()
    }

    fn timestamps(&self) -> &TimestampExecutor<'_> {
        self.plain.timestamps()
    }

    fn sync(&self) -> SyncRunner<'_> {
        self.plain.sync()
    }

    /// A redone move or copy that Windows refused in part.
    fn elevate_transfer(
        &self,
        plan: &TransferPlan,
        outcome: TransferOutcome,
        resolutions: &Resolutions,
    ) -> Elevated<TransferOutcome> {
        let Some(retry) = ElevatedRetry::retry_for_transfer(plan, &outcome, Some(resolutions))
        else {
            return Elevated::plain(outcome);
        };
        let items: Vec<String> = retry
            .plan
            .transfers
            .iter()
            .map(|t| t.source_path.clone())
            .collect();
        let operation = if plan.verb == TransferVerb::Move {
            ElevationOperation::TransferMove
        } else {
            ElevationOperation::TransferCopy
        };
        let (second, note) = self.inner.elevate(operation, &items, false, |runner| {
            runner.run_transfer(&retry, None, &CancelToken::new())
        });
        match second {
            Some(second) => match ElevatedRetry::merge_transfer(&outcome, &retry, &second) {
                Ok(merged) => Elevated::new(merged, &note),
                Err(e) => Elevated::new(outcome, &format!(" \u{2014} {e}")),
            },
            None => Elevated::new(outcome, &note),
        }
    }

    /// A redone delete that Windows refused in part.
    fn elevate_delete(&self, plan: &DeletePlan, outcome: DeleteOutcome) -> Elevated<DeleteOutcome> {
        let Some(retry) = ElevatedRetry::retry_for_delete(plan, &outcome) else {
            return Elevated::plain(outcome);
        };
        let items: Vec<String> = retry
            .plan
            .deletions
            .iter()
            .map(|d| d.source_path.clone())
            .collect();
        let (second, note) =
            self.inner
                .elevate(ElevationOperation::Delete, &items, false, |runner| {
                    runner.run_delete(&retry, None, &CancelToken::new())
                });
        match second {
            Some(second) => match ElevatedRetry::merge_delete(&outcome, &retry, &second) {
                Ok(merged) => Elevated::new(merged, &note),
                Err(e) => Elevated::new(outcome, &format!(" \u{2014} {e}")),
            },
            None => Elevated::new(outcome, &note),
        }
    }

    /// A rename undone or redone that Windows refused in part.
    fn elevate_rename(&self, plan: &RenamePlan, outcome: RenameOutcome) -> Elevated<RenameOutcome> {
        let Some(retry) = ElevatedRetry::retry_for_rename(plan, &outcome) else {
            return Elevated::plain(outcome);
        };
        let items: Vec<String> = retry
            .plan
            .renames
            .iter()
            .map(|r| r.source_path.clone())
            .collect();
        let (second, note) =
            self.inner
                .elevate(ElevationOperation::Rename, &items, true, |runner| {
                    runner.run_rename(&retry, &CancelToken::new())
                });
        match second {
            Some(second) => Elevated::new(
                ElevatedRetry::merge_rename(&outcome, &retry, &second),
                &note,
            ),
            None => Elevated::new(outcome, &note),
        }
    }

    /// Putting a move back, where Windows refused part of it. Only a move: the elevated helper's undo
    /// hardcodes a move, and handed a copy it would move the pasted files back into their source.
    fn elevate_move_undo(
        &self,
        outcome: &TransferOutcome,
        result: TransferUndoResult,
    ) -> Elevated<TransferUndoResult> {
        if outcome.verb != TransferVerb::Move {
            return Elevated::plain(result);
        }
        let Some(retry) = ElevatedRetry::undo_retry_for_transfer(outcome, &result) else {
            return Elevated::plain(result);
        };
        let items: Vec<String> = retry
            .outcome
            .completed
            .iter()
            .map(|c| c.source_path.clone())
            .collect();
        let (second, note) =
            self.inner
                .elevate(ElevationOperation::TransferUndo, &items, true, |runner| {
                    runner.undo_transfer(&retry.outcome, &CancelToken::new())
                });
        match second {
            Some(second) => Elevated::new(
                ElevatedRetry::merge_transfer_undo(&result, &retry, &second),
                &note,
            ),
            None => Elevated::new(result, &note),
        }
    }

    /// Bringing deleted items back, where Windows refused part of it.
    fn elevate_delete_undo(
        &self,
        outcome: &DeleteOutcome,
        result: DeleteUndoResult,
    ) -> Elevated<DeleteUndoResult> {
        let Some(retry) = ElevatedRetry::undo_retry_for_delete(outcome, &result) else {
            return Elevated::plain(result);
        };
        let items: Vec<String> = retry
            .outcome
            .deleted
            .iter()
            .map(|d| d.source_path.clone())
            .collect();
        let (second, note) =
            self.inner
                .elevate(ElevationOperation::DeleteUndo, &items, true, |runner| {
                    runner.undo_delete(&retry.outcome, &CancelToken::new())
                });
        match second {
            Some(second) => Elevated::new(
                ElevatedRetry::merge_delete_undo(&result, &retry, &second),
                &note,
            ),
            None => Elevated::new(result, &note),
        }
    }

    fn begin_progress(&self, plan: &TransferPlan, headline: &str) -> Option<ProgressSink<'_>> {
        self.plain.begin_progress(plan, headline)
    }

    fn end_progress(&self) {
        self.plain.end_progress()
    }
}
