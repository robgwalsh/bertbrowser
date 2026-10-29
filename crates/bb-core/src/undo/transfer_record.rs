//! A move, or a merged paste, in the undo history.
//! Spec: Core/Services/UndoHistory/TransferRecord.cs.
//!
//! Applied, it holds the outcome the executor returned, and with it whatever a Replace displaced
//! into staging. Undone, it holds only the completions the undo actually reversed, with no staging:
//! the undo has put the displaced entries back, and the redo will displace them again under
//! `TransferRedo`'s rules.
//!
//! **A paste's undo is never retried elevated.** See `UndoHost`: the helper's undo would treat it
//! as a move.

use bb_paths::file_name;

use super::host::UndoHost;
use super::models::{HeldEntry, StepReport, StepResult, UndoItemDetail, UndoKind, UndoRefresh};
use super::record::UndoableRecord;
use super::text::UndoText;
use crate::cancel::CancelToken;
use crate::rename::paths::{directory_exists, directory_name};
use crate::transfer::{
    TransferExecutor, TransferOutcome, TransferProgress, TransferRedo, TransferVerb,
};

#[derive(Clone)]
pub struct TransferRecord {
    outcome: TransferOutcome,
    is_applied: bool,
    description: String,
}

impl TransferRecord {
    /// The record for a move, or a merged paste the caller undertakes to keep; None for anything
    /// the history cannot take back.
    pub fn for_outcome(outcome: TransferOutcome) -> Option<TransferRecord> {
        if !outcome.can_undo() && !outcome.can_undo_copy {
            return None;
        }

        let place = UndoText::folder_name(&outcome.destination_directory);
        let count = UndoText::items(outcome.completed.len());
        let description = match outcome.verb {
            TransferVerb::Move => format!("Move {count} to {place}"),
            TransferVerb::Copy => format!("Paste {count} into {place}"),
        };
        Some(TransferRecord {
            outcome,
            is_applied: true,
            description,
        })
    }

    pub fn outcome(&self) -> &TransferOutcome {
        &self.outcome
    }

    fn undo(&self, host: &dyn UndoHost) -> StepResult {
        let cancel = CancelToken::none();
        let (result, note) = if self.outcome.verb == TransferVerb::Copy {
            (
                host.transfers().undo_copies(&self.outcome, &cancel),
                String::new(),
            )
        } else {
            let first = host.transfers().undo(&self.outcome, &cancel);
            let elevated = host.elevate_move_undo(&self.outcome, first);
            (elevated.outcome, elevated.note)
        };

        let next = TransferRecord {
            outcome: TransferOutcome {
                completed: result.reverted.clone(),
                skipped: Vec::new(),
                failed: Vec::new(),
                staging_directories: Vec::new(),
                cancelled: false,
                ..self.outcome.clone()
            },
            is_applied: false,
            description: self.description.clone(),
        };

        StepResult::new(
            Box::new(next),
            StepReport::with_note(
                result.reverted.len(),
                result.failed.iter().map(|f| f.message.clone()).collect(),
                &note,
            ),
            Self::refresh_for(&self.outcome),
        )
    }

    fn redo(&self, host: &dyn UndoHost) -> StepResult {
        let (plan, resolutions) = TransferRedo::plan_for(
            self.outcome.verb,
            &self.outcome.destination_directory,
            &self.outcome.completed,
            &self.outcome.pruned_directories,
        );

        let mut progress = host.begin_progress(&plan, &format!("Redoing — {}…", self.description));
        let outcome = host.transfers().execute(
            &plan,
            Some(&resolutions),
            &CancelToken::none(),
            progress
                .as_deref_mut()
                .map(|sink| sink as &mut dyn FnMut(TransferProgress)),
            None,
        );
        drop(progress);
        host.end_progress();

        let elevated = host.elevate_transfer(&plan, outcome, &resolutions);
        let note = elevated.note;
        let mut merged = elevated.outcome;
        if merged.verb == TransferVerb::Copy {
            merged = TransferExecutor::stamp_copies(merged);
            merged.can_undo_copy = true;
        }

        // A redo that moved nothing leaves the entry where it was, so nothing will ever retire this
        // outcome: take away any staging folder it made and emptied again, never one still holding.
        if merged.completed.is_empty() {
            host.transfers().purge_staging(&merged);
        }

        let failures: Vec<String> = merged
            .failed
            .iter()
            .map(|f| f.message.clone())
            .chain(merged.skipped.iter().map(|s| {
                format!(
                    "{}: its old place is taken now, so it was left where it is.",
                    file_name(s)
                )
            }))
            .collect();

        let report = StepReport::with_note(merged.completed.len(), failures, &note);
        let refresh = Self::refresh_for(&merged);
        StepResult::new(
            Box::new(TransferRecord {
                outcome: merged,
                is_applied: true,
                description: self.description.clone(),
            }),
            report,
            refresh,
        )
    }

    /// Both ends of every item, the destination, and the parents of any folder a merge emptied:
    /// the same set a drop refreshes.
    fn refresh_for(outcome: &TransferOutcome) -> UndoRefresh {
        UndoRefresh::of(
            outcome
                .completed
                .iter()
                .flat_map(|c| {
                    [
                        directory_name(&c.source_path),
                        directory_name(&c.final_path),
                    ]
                })
                .chain(outcome.pruned_directories.iter().map(|p| directory_name(p)))
                .chain(std::iter::once(Some(outcome.destination_directory.clone()))),
        )
    }
}

impl UndoableRecord for TransferRecord {
    fn kind(&self) -> UndoKind {
        match self.outcome.verb {
            TransferVerb::Move => UndoKind::Move,
            TransferVerb::Copy => UndoKind::Paste,
        }
    }

    fn is_applied(&self) -> bool {
        self.is_applied
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn item_count(&self) -> usize {
        self.outcome.completed.len()
    }

    fn details(&self) -> Vec<UndoItemDetail> {
        self.outcome
            .completed
            .iter()
            .map(|c| UndoItemDetail::new(&c.source_path, &c.final_path, c.is_directory))
            .collect()
    }

    fn held(&self) -> Vec<HeldEntry> {
        if !self.is_applied {
            return Vec::new();
        }
        self.outcome
            .completed
            .iter()
            .filter_map(|c| {
                c.displaced_stage_path
                    .as_ref()
                    .map(|staged| HeldEntry::new(staged, &c.final_path, directory_exists(staged)))
            })
            .collect()
    }

    fn step(&self, host: &dyn UndoHost) -> StepResult {
        if self.is_applied {
            self.undo(host)
        } else {
            self.redo(host)
        }
    }

    fn retire(&self) {
        if self.is_applied {
            TransferExecutor::commit_staging(&self.outcome);
        }
    }
}
