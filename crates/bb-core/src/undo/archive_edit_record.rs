//! An edit to an archive's contents in the undo history. Both directions are a swap of two whole
//! containers beside each other, so each state holds exactly one file: the original while the edit
//! is in effect, the edited one while it is undone.
//! Spec: Core/Services/UndoHistory/ArchiveEditRecord.cs.

use bb_paths::file_name;

use super::host::UndoHost;
use super::models::{HeldEntry, StepReport, StepResult, UndoItemDetail, UndoKind, UndoRefresh};
use super::record::UndoableRecord;
use crate::archives::{ArchiveEditExecutor, ArchiveEditOutcome, ArchiveEditUndo};
use crate::rename::paths::directory_name;

#[derive(Clone)]
pub struct ArchiveEditRecord {
    applied: Option<ArchiveEditOutcome>,
    undone: Option<ArchiveEditUndo>,
    description: String,
}

impl ArchiveEditRecord {
    pub fn for_outcome(outcome: ArchiveEditOutcome) -> Option<ArchiveEditRecord> {
        if !outcome.can_undo() {
            return None;
        }
        let description = format!("Edit {}", file_name(&outcome.archive_file));
        Some(ArchiveEditRecord {
            applied: Some(outcome),
            undone: None,
            description,
        })
    }

    fn archive_file(&self) -> &str {
        match (&self.applied, &self.undone) {
            (Some(applied), _) => &applied.archive_file,
            (None, Some(undone)) => &undone.archive_file,
            (None, None) => "",
        }
    }
}

impl UndoableRecord for ArchiveEditRecord {
    fn kind(&self) -> UndoKind {
        UndoKind::ArchiveEdit
    }

    fn is_applied(&self) -> bool {
        self.applied.is_some()
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn item_count(&self) -> usize {
        1
    }

    fn details(&self) -> Vec<UndoItemDetail> {
        let archive = self.archive_file();
        vec![UndoItemDetail::new(archive, archive, false)]
    }

    fn held(&self) -> Vec<HeldEntry> {
        let held = self
            .applied
            .as_ref()
            .and_then(|a| a.staged_original.as_deref())
            .or_else(|| {
                self.undone
                    .as_ref()
                    .and_then(|u| u.staged_edited.as_deref())
            });
        match held {
            Some(path) => vec![HeldEntry::new(path, self.archive_file(), false)],
            None => Vec::new(),
        }
    }

    fn step(&self, host: &dyn UndoHost) -> StepResult {
        let archive = self.archive_file().to_string();
        let mut refresh = UndoRefresh::of([directory_name(&archive)]);
        refresh.containers = vec![archive.clone()];

        if let Some(applied) = &self.applied {
            let undo = host.archive_edits().undo(applied);
            return match undo.failure.clone() {
                Some(failure) => StepResult::new(
                    Box::new(self.clone()),
                    StepReport::new(0, vec![failure]),
                    refresh,
                ),
                None => StepResult::new(
                    Box::new(ArchiveEditRecord {
                        applied: None,
                        undone: Some(undo),
                        description: self.description.clone(),
                    }),
                    StepReport::new(1, Vec::new()),
                    refresh,
                ),
            };
        }

        let undone = self
            .undone
            .as_ref()
            .expect("an archive-edit record is either applied or undone");
        let redo = host.archive_edits().redo(undone);
        match redo.failure.clone() {
            Some(refused) => StepResult::new(
                Box::new(self.clone()),
                StepReport::new(0, vec![refused]),
                refresh,
            ),
            None => StepResult::new(
                Box::new(ArchiveEditRecord {
                    applied: Some(redo),
                    undone: None,
                    description: self.description.clone(),
                }),
                StepReport::new(1, Vec::new()),
                refresh,
            ),
        }
    }

    fn retire(&self) {
        if let Some(applied) = &self.applied {
            ArchiveEditExecutor::commit_staging(applied);
        } else if let Some(undone) = &self.undone {
            ArchiveEditExecutor::commit_undo_staging(undone);
        }
    }
}
