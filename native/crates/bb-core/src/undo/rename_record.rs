//! A rename in the undo history. A rename is its own inverse, so undo and redo are the same step:
//! run the outcome's reversal, and keep what that run completed as the next record.
//! Spec: Core/Services/UndoHistory/RenameRecord.cs.
//!
//! It holds nothing (its temporary names are gone by the time it finishes), so it costs the budget
//! nothing and retiring it does nothing.

use bb_paths::file_name;

use super::host::UndoHost;
use super::models::{HeldEntry, StepReport, StepResult, UndoItemDetail, UndoKind, UndoRefresh};
use super::record::UndoableRecord;
use super::text::UndoText;
use crate::rename::paths::directory_name;
use crate::rename::{RenameExecutor, RenameOutcome};

#[derive(Clone)]
pub struct RenameRecord {
    /// The last run, in the direction it went. Undone, that is the reversal, so its "source" is
    /// where the item was renamed *to*.
    last_run: RenameOutcome,
    is_applied: bool,
    description: String,
}

impl RenameRecord {
    pub fn for_outcome(outcome: RenameOutcome) -> Option<RenameRecord> {
        if !outcome.can_undo() {
            return None;
        }

        let description = if outcome.completed.len() == 1 {
            format!(
                "Rename '{}' to '{}'",
                file_name(&outcome.completed[0].source_path),
                file_name(&outcome.completed[0].final_path)
            )
        } else {
            format!("Rename {}", UndoText::items(outcome.completed.len()))
        };
        Some(RenameRecord {
            last_run: outcome,
            is_applied: true,
            description,
        })
    }
}

impl UndoableRecord for RenameRecord {
    fn kind(&self) -> UndoKind {
        UndoKind::Rename
    }

    fn is_applied(&self) -> bool {
        self.is_applied
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn item_count(&self) -> usize {
        self.last_run.completed.len()
    }

    /// Always old name to new name, whichever way the last run went.
    fn details(&self) -> Vec<UndoItemDetail> {
        self.last_run
            .completed
            .iter()
            .map(|c| {
                if self.is_applied {
                    UndoItemDetail::new(&c.source_path, &c.final_path, c.is_directory)
                } else {
                    UndoItemDetail::new(&c.final_path, &c.source_path, c.is_directory)
                }
            })
            .collect()
    }

    fn held(&self) -> Vec<HeldEntry> {
        Vec::new()
    }

    fn step(&self, host: &dyn UndoHost) -> StepResult {
        let plan = RenameExecutor::undo_plan(&self.last_run);
        let first = host.renames().execute(&plan);
        let elevated = host.elevate_rename(&plan, first);
        let (result, note) = (elevated.outcome, elevated.note);

        let mut refresh =
            UndoRefresh::of(plan.renames.iter().map(|r| directory_name(&r.source_path)));
        refresh.renamed = result.completed.clone();
        let report = StepReport::with_note(
            result.completed.len(),
            result.failed.iter().map(|f| f.message.clone()).collect(),
            &note,
        );
        StepResult::new(
            Box::new(RenameRecord {
                last_run: result,
                is_applied: !self.is_applied,
                description: self.description.clone(),
            }),
            report,
            refresh,
        )
    }

    fn retire(&self) {}
}
