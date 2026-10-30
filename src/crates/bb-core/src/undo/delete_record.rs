//! A delete in the undo history: to the Recycle Bin or into the holding folder. A permanent delete
//! never gets one: there is nothing left to put back.
//! Spec: Core/Services/UndoHistory/DeleteRecord.cs.
//!
//! Only items this app holds count against the budget. What went to the Recycle Bin is held by
//! Windows, under Windows' own limits, and retiring the entry leaves it there.

use super::host::UndoHost;
use super::models::{HeldEntry, StepReport, StepResult, UndoItemDetail, UndoKind, UndoRefresh};
use super::record::UndoableRecord;
use super::text::UndoText;
use crate::cancel::CancelToken;
use crate::delete::{DeleteExecutor, DeleteOutcome, DeleteRedo, DeletedItem};
use crate::rename::paths::directory_name;

const RECYCLE_BIN: &str = "Recycle Bin";
const HOLDING_FOLDER: &str = "BertBrowser's holding folder";

#[derive(Clone)]
pub struct DeleteRecord {
    outcome: DeleteOutcome,
    is_applied: bool,
    description: String,
}

impl DeleteRecord {
    /// `description` is what to call it when it was not a delete to the user: a drag out of the
    /// app is carried out as one.
    pub fn for_outcome(outcome: DeleteOutcome, description: Option<&str>) -> Option<DeleteRecord> {
        if !outcome.can_undo() {
            return None;
        }

        // Only what can come back is worth recording: an item the bin erased rather than holding
        // would fail every undo, and a redo has nothing to redo for it.
        let recoverable = DeleteOutcome {
            deleted: outcome
                .deleted
                .iter()
                .filter(|d| d.is_recoverable())
                .cloned()
                .collect(),
            ..outcome
        };
        let description = match description {
            Some(text) => text.to_string(),
            None => {
                let paths: Vec<String> = recoverable
                    .deleted
                    .iter()
                    .map(|d| d.source_path.clone())
                    .collect();
                format!("Delete {}", UndoText::named(&paths))
            }
        };
        Some(DeleteRecord {
            outcome: recoverable,
            is_applied: true,
            description,
        })
    }

    pub fn outcome(&self) -> &DeleteOutcome {
        &self.outcome
    }

    fn where_it_went(item: &DeletedItem) -> &'static str {
        if item.recycled_path.is_some() {
            RECYCLE_BIN
        } else {
            HOLDING_FOLDER
        }
    }

    fn undo(&self, host: &dyn UndoHost) -> StepResult {
        let first = host.deletes().undo(&self.outcome, &CancelToken::none());
        let elevated = host.elevate_delete_undo(&self.outcome, first);
        let (result, note) = (elevated.outcome, elevated.note);

        let next = DeleteRecord {
            outcome: DeleteOutcome {
                deleted: result.reverted.clone(),
                failed: Vec::new(),
                staging_directories: Vec::new(),
                permanent: self.outcome.permanent,
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
            UndoRefresh::of(
                self.outcome
                    .deleted
                    .iter()
                    .map(|d| directory_name(&d.source_path)),
            ),
        )
    }

    fn redo(&self, host: &dyn UndoHost) -> StepResult {
        let plan = DeleteRedo::plan_for(&self.outcome.deleted);
        let first = host.deletes().execute(&plan, &CancelToken::none(), None);
        let elevated = host.elevate_delete(&plan, first);
        let (outcome, note) = (elevated.outcome, elevated.note);

        // Whatever the executor could not hold is not something a later undo could bring back.
        let next = DeleteRecord {
            outcome: DeleteOutcome {
                deleted: outcome
                    .deleted
                    .iter()
                    .filter(|d| d.is_recoverable())
                    .cloned()
                    .collect(),
                ..outcome.clone()
            },
            is_applied: true,
            description: self.description.clone(),
        };

        let mut refresh = UndoRefresh::of(plan.deletions.iter().map(|d| Some(d.parent_path())));
        refresh.vacated = outcome.deleted.clone();
        let report = StepReport::with_note(
            next.item_count(),
            outcome.failed.iter().map(|f| f.message.clone()).collect(),
            &note,
        );
        StepResult::new(Box::new(next), report, refresh)
    }
}

impl UndoableRecord for DeleteRecord {
    fn kind(&self) -> UndoKind {
        UndoKind::Delete
    }

    fn is_applied(&self) -> bool {
        self.is_applied
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn item_count(&self) -> usize {
        self.outcome.deleted.len()
    }

    fn details(&self) -> Vec<UndoItemDetail> {
        self.outcome
            .deleted
            .iter()
            .map(|d| UndoItemDetail::new(&d.source_path, Self::where_it_went(d), d.is_directory))
            .collect()
    }

    fn held(&self) -> Vec<HeldEntry> {
        if !self.is_applied {
            return Vec::new();
        }
        self.outcome
            .deleted
            .iter()
            .filter_map(|d| {
                d.staged_path
                    .as_ref()
                    .map(|staged| HeldEntry::new(staged, &d.source_path, d.is_directory))
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
            DeleteExecutor::commit_staging(&self.outcome);
        }
    }
}
