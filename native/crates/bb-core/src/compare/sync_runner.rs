//! Running a sync's plans, in order, through the executors that already exist. Spec:
//! Core/Services/Compare/SyncRunner.cs.
//!
//! It opens no file and creates no directory. Every byte goes through the transfer executor and
//! every removal through the delete executor, so all of their rules are in force unchanged: the
//! plan is re-validated against live disk at the moment of the write, a displaced entry goes to
//! staging rather than being erased, one item's failure never affects the others, and a cancel
//! leaves nothing half-written.
//!
//! **Copies run before removals.** A cancel half-way then leaves the right side holding more
//! than it started with rather than less, which is the direction to fail in: the copies can be
//! repeated harmlessly, whereas a delete that ran before its replacement arrived is a gap.

use super::sync_models::{SyncOutcome, SyncPlans};
use super::transfer_stubs::{
    DeleteOutcome, DeleteProgress, DeleteUndoResult, SyncDeletes, SyncTransfers, TransferOutcome,
    TransferProgress, TransferVerb,
};
use crate::cancel::CancelToken;

pub struct SyncRunner<'a> {
    transfers: &'a dyn SyncTransfers,
    deletes: &'a dyn SyncDeletes,
}

/// A progress report re-based onto the whole run's denominator.
type OffsetTransfer<'p> = Box<dyn Fn(&TransferProgress) + 'p>;
type OffsetDelete<'p> = Box<dyn Fn(&DeleteProgress) + 'p>;

/// Reversing a sync: how many items came back, and what could not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncUndoResult {
    pub restored: usize,
    pub failed: Vec<String>,
    /// The run cut down to exactly what went back: the record a redo runs again.
    pub reverted: SyncOutcome,
}

impl<'a> SyncRunner<'a> {
    pub fn new(transfers: &'a dyn SyncTransfers, deletes: &'a dyn SyncDeletes) -> SyncRunner<'a> {
        SyncRunner { transfers, deletes }
    }

    pub fn run(
        &self,
        plans: &SyncPlans,
        cancel: &CancelToken,
        progress: Option<&dyn Fn(&TransferProgress)>,
    ) -> SyncOutcome {
        // One denominator for the whole run, counted before it starts: a bar that restarts at
        // each destination folder would say a recursive sync was nearly done several times over.
        let total = plans.item_count();
        let mut done = 0;
        let mut cancelled = false;

        let mut copies: Vec<TransferOutcome> = Vec::new();
        for plan in &plans.copies {
            if cancel.is_cancelled() {
                cancelled = true;
                break;
            }

            let offset = Self::offset(progress, done, total);
            let outcome =
                self.transfers
                    .execute(plan, &plans.resolutions, cancel, offset.as_deref());

            done += plan.transfers.len();
            let stop = outcome.cancelled;
            copies.push(outcome);
            if stop {
                cancelled = true;
                break;
            }
        }

        let mut removals = DeleteOutcome::empty(plans.removals.permanent());
        if !cancelled && plans.removals.has_work() && !cancel.is_cancelled() {
            let as_delete = Self::as_delete(progress, done, total);
            removals = self
                .deletes
                .execute(&plans.removals, cancel, as_delete.as_deref());
        }

        SyncOutcome {
            copies,
            removals,
            cancelled: cancelled || cancel.is_cancelled(),
        }
    }

    /// Puts back everything the run did: the copies through the executor's copy-undo, the
    /// removals through the delete executor's undo.
    ///
    /// In the reverse order they ran, so what was deleted comes back before the copies that may
    /// have replaced it are unwound, and a folder is never removed before what was written inside
    /// it. Each half reports its own failures; neither stops the other, because a sync that could
    /// only be half undone still has the other half worth undoing.
    pub fn undo(&self, outcome: &SyncOutcome, cancel: &CancelToken) -> SyncUndoResult {
        let delete_undo = if outcome.removals.can_undo() {
            self.deletes.undo(&outcome.removals, cancel)
        } else {
            DeleteUndoResult {
                restored: 0,
                failed: Vec::new(),
                reverted: Vec::new(),
            }
        };

        let mut restored = delete_undo.restored;
        let mut failed: Vec<String> = delete_undo
            .failed
            .iter()
            .map(|f| f.message.clone())
            .collect();

        let mut reverted_copies: Vec<TransferOutcome> = Vec::new();
        for copy in outcome.copies.iter().rev() {
            let undo = self.transfers.undo_copies(copy, cancel);
            restored += undo.restored;
            failed.extend(undo.failed.iter().map(|f| f.message.clone()));

            // Only what really went back, in the shape a redo can run again. Staging is emptied:
            // the undo has already purged what it emptied, and anything left there belongs to an
            // item that could not be put back.
            if !undo.reverted.is_empty() {
                reverted_copies.insert(
                    0,
                    TransferOutcome {
                        completed: undo.reverted,
                        skipped: Vec::new(),
                        failed: Vec::new(),
                        staging_directories: Vec::new(),
                        ..copy.clone()
                    },
                );
            }
        }

        let reverted = SyncOutcome {
            copies: reverted_copies,
            removals: DeleteOutcome {
                deleted: delete_undo.reverted,
                failed: Vec::new(),
                staging_directories: Vec::new(),
                ..outcome.removals.clone()
            },
            cancelled: false,
        };
        SyncUndoResult {
            restored,
            failed,
            reverted,
        }
    }

    /// Does an undone sync again, from what its undo put back: the copies first, then the
    /// removals, the same order [`run`](Self::run) keeps and for the same reason. Each half is
    /// rebuilt by the executors' redo planners, so a copy lands under the name it had, a replaced
    /// file is replaced again and nothing else is, and a removal goes back where it went.
    pub fn redo(
        &self,
        reverted: &SyncOutcome,
        cancel: &CancelToken,
        progress: Option<&dyn Fn(&TransferProgress)>,
    ) -> SyncOutcome {
        let total = reverted
            .copies
            .iter()
            .map(|c| c.completed.len())
            .sum::<usize>()
            + reverted.removals.deleted.len();
        let mut done = 0;
        let mut cancelled = false;

        let mut copies: Vec<TransferOutcome> = Vec::new();
        for copy in &reverted.copies {
            if cancel.is_cancelled() {
                cancelled = true;
                break;
            }

            let (plan, resolutions) = self.transfers.redo_plan(
                TransferVerb::Copy,
                &copy.destination_directory,
                &copy.completed,
            );
            let offset = Self::offset(progress, done, total);
            let outcome = self
                .transfers
                .execute(&plan, &resolutions, cancel, offset.as_deref());
            done += plan.transfers.len();
            let stop = outcome.cancelled;
            copies.push(outcome);
            if stop {
                cancelled = true;
                break;
            }
        }

        let mut removals = DeleteOutcome::empty(reverted.removals.permanent);
        if !cancelled && !reverted.removals.deleted.is_empty() && !cancel.is_cancelled() {
            let as_delete = Self::as_delete(progress, done, total);
            removals = self.deletes.execute(
                &self.deletes.redo_plan(&reverted.removals.deleted),
                cancel,
                as_delete.as_deref(),
            );
        }

        SyncOutcome {
            copies,
            removals,
            cancelled: cancelled || cancel.is_cancelled(),
        }
    }

    /// Commits what the run set aside, once it can no longer be undone. Until this runs, a
    /// replaced file is still on disk in a staging folder and a removed one is still in the
    /// Recycle Bin.
    pub fn retire(&self, outcome: &SyncOutcome) {
        for copy in &outcome.copies {
            self.transfers.commit_staging(copy);
        }

        self.deletes.commit_staging(&outcome.removals);
    }

    fn offset<'p>(
        inner: Option<&'p dyn Fn(&TransferProgress)>,
        done: usize,
        total: usize,
    ) -> Option<OffsetTransfer<'p>> {
        inner.map(move |report| {
            Box::new(move |value: &TransferProgress| {
                report(&TransferProgress {
                    done: done + value.done,
                    total,
                    ..value.clone()
                })
            }) as OffsetTransfer<'p>
        })
    }

    /// The removals reported on the same bar as the copies. Byte figures are deliberately left at
    /// zero: a delete moves no bytes, and carrying the copies' running total through would make
    /// the throughput read as though it were still writing.
    fn as_delete<'p>(
        inner: Option<&'p dyn Fn(&TransferProgress)>,
        done: usize,
        total: usize,
    ) -> Option<OffsetDelete<'p>> {
        inner.map(move |report| {
            Box::new(move |value: &DeleteProgress| {
                report(&TransferProgress {
                    done: done + value.done,
                    total,
                    current_name: value.current_name.clone(),
                    bytes_done: 0,
                    current_item_bytes: 0,
                    current_item_total: 0,
                })
            }) as OffsetDelete<'p>
        })
    }
}
