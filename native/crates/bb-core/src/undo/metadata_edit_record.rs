//! A metadata edit in the undo history. Each file has its other version held beside it (the
//! original while the edit is in effect, the edited one while it is undone) and a step swaps the
//! two, so undo and redo are the same move in opposite directions.
//! Spec: Core/Services/UndoHistory/MetadataEditRecord.cs.
//!
//! A file that would not swap drops out of the record, as with every other kind, and its held
//! version is *left where it is* rather than erased: the failure names the path, and a stray file
//! beside a picture is a far smaller problem than the only copy of its original gone.

use bb_paths::file_name;

use super::host::UndoHost;
use super::models::{HeldEntry, StepReport, StepResult, UndoItemDetail, UndoKind, UndoRefresh};
use super::record::UndoableRecord;
use crate::metadata::{HeldVersion, MetadataEditExecutor, MetadataEditOutcome};
use crate::rename::paths::directory_name;

#[derive(Clone)]
pub struct MetadataEditRecord {
    held: Vec<HeldVersion>,
    is_applied: bool,
    description: String,
}

impl MetadataEditRecord {
    pub fn for_outcome(outcome: MetadataEditOutcome) -> Option<MetadataEditRecord> {
        if !outcome.can_undo() {
            return None;
        }
        let description = Self::describe(&outcome.edited);
        Some(MetadataEditRecord {
            held: outcome.edited,
            is_applied: true,
            description,
        })
    }

    fn describe(edited: &[HeldVersion]) -> String {
        if edited.len() == 1 {
            format!("Edit metadata of {}", file_name(&edited[0].path))
        } else {
            format!("Edit metadata of {} files", edited.len())
        }
    }
}

impl UndoableRecord for MetadataEditRecord {
    fn kind(&self) -> UndoKind {
        UndoKind::MetadataEdit
    }

    fn is_applied(&self) -> bool {
        self.is_applied
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn item_count(&self) -> usize {
        self.held.len()
    }

    fn details(&self) -> Vec<UndoItemDetail> {
        self.held
            .iter()
            .map(|h| UndoItemDetail::new(&h.path, &h.path, false))
            .collect()
    }

    fn held(&self) -> Vec<HeldEntry> {
        self.held
            .iter()
            .map(|h| HeldEntry::new(&h.held_path, &h.path, false))
            .collect()
    }

    fn step(&self, host: &dyn UndoHost) -> StepResult {
        let result = if self.is_applied {
            host.metadata_edits().undo(&self.held)
        } else {
            host.metadata_edits().redo(&self.held)
        };

        let refresh = UndoRefresh::of(self.held.iter().map(|h| directory_name(&h.path)));
        let report = StepReport::new(
            result.swapped.len(),
            result.failed.iter().map(|f| f.message.clone()).collect(),
        );

        if result.swapped.is_empty() {
            StepResult::new(Box::new(self.clone()), report, refresh)
        } else {
            StepResult::new(
                Box::new(MetadataEditRecord {
                    held: result.swapped,
                    is_applied: !self.is_applied,
                    description: self.description.clone(),
                }),
                report,
                refresh,
            )
        }
    }

    fn retire(&self) {
        MetadataEditExecutor::commit_staging(&self.held);
    }
}
