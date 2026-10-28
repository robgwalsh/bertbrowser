//! A change of dates in the undo history. It holds nothing on disk (the old dates ride in the
//! record), so there is nothing to retire and nothing counted against the history's budget.
//! Spec: Core/Services/UndoHistory/TimestampRecord.cs.

use bb_paths::file_name;

use super::host::UndoHost;
use super::models::{HeldEntry, StepReport, StepResult, UndoItemDetail, UndoKind, UndoRefresh};
use super::record::UndoableRecord;
use crate::rename::paths::directory_name;
use crate::timestamps::{StampedEntry, TimestampOutcome};

#[derive(Clone)]
pub struct TimestampRecord {
    stamped: Vec<StampedEntry>,
    is_applied: bool,
    description: String,
}

impl TimestampRecord {
    pub fn for_outcome(outcome: TimestampOutcome) -> Option<TimestampRecord> {
        if !outcome.can_undo() {
            return None;
        }
        let description = if outcome.stamped.len() == 1 {
            format!("Change the date of {}", file_name(&outcome.stamped[0].path))
        } else {
            format!("Change the dates of {} items", outcome.stamped.len())
        };
        Some(TimestampRecord {
            stamped: outcome.stamped,
            is_applied: true,
            description,
        })
    }
}

impl UndoableRecord for TimestampRecord {
    fn kind(&self) -> UndoKind {
        UndoKind::Timestamps
    }

    fn is_applied(&self) -> bool {
        self.is_applied
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn item_count(&self) -> usize {
        self.stamped.len()
    }

    fn details(&self) -> Vec<UndoItemDetail> {
        self.stamped
            .iter()
            .map(|s| UndoItemDetail::new(&s.path, &s.path, s.is_directory))
            .collect()
    }

    fn held(&self) -> Vec<HeldEntry> {
        Vec::new()
    }

    fn step(&self, host: &dyn UndoHost) -> StepResult {
        let result = host.timestamps().revert(&self.stamped);

        let refresh = UndoRefresh::of(self.stamped.iter().map(|s| directory_name(&s.path)));
        let report = StepReport::new(
            result.stamped.len(),
            result.failed.iter().map(|f| f.message.clone()).collect(),
        );

        if result.stamped.is_empty() {
            StepResult::new(Box::new(self.clone()), report, refresh)
        } else {
            StepResult::new(
                Box::new(TimestampRecord {
                    stamped: result.stamped,
                    is_applied: !self.is_applied,
                    description: self.description.clone(),
                }),
                report,
                refresh,
            )
        }
    }

    fn retire(&self) {}
}
