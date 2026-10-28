//! An edit to an archive's contents in the undo history. Both directions are a swap of two whole
//! containers beside each other, so each state holds exactly one file: the original while the edit
//! is in effect, the edited one while it is undone.
//! Spec: Core/Services/UndoHistory/ArchiveEditRecord.cs.

use bb_paths::file_name;

use super::host::UndoHost;
use super::models::{HeldEntry, StepReport, StepResult, UndoItemDetail, UndoKind, UndoRefresh};
use super::record::UndoableRecord;
use crate::fs::EntryStamp;
use crate::rename::paths::directory_name;

// TODO(merge): replaced by crate::archives::ArchiveEditOutcome
/// What an archive edit did, and what it left behind so it can be undone. `staged_original` is
/// where the container that was replaced is being held, erased by `commit_staging` exactly one
/// operation later. Spec: Core/Services/Archives/ArchiveEditModels.cs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEditOutcome {
    pub archive_file: String,
    pub staged_original: Option<String>,
    pub entries_written: usize,
    pub failure: Option<String>,
    pub cancelled: bool,
    /// The original container as it was before the swap: what a redo checks is still in place
    /// before it takes the edit back in.
    pub original: Option<EntryStamp>,
    /// The edited container as the swap left it: what an undo checks is still in place before it
    /// sets it aside. A container changed by anything else since is refused, not lost.
    pub edited: Option<EntryStamp>,
}

impl ArchiveEditOutcome {
    pub fn can_undo(&self) -> bool {
        self.staged_original.is_some() && self.failure.is_none() && !self.cancelled
    }

    pub fn nothing(archive_file: &str) -> ArchiveEditOutcome {
        ArchiveEditOutcome {
            archive_file: archive_file.to_string(),
            staged_original: None,
            entries_written: 0,
            failure: None,
            cancelled: false,
            original: None,
            edited: None,
        }
    }
}

// TODO(merge): replaced by crate::archives::ArchiveEditUndo
/// An archive edit that was undone: the original is back in place, and the edited container is
/// held beside it so the edit can be redone. `staged_edited` is None when the undo did not
/// happen, and `failure` says why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEditUndo {
    pub archive_file: String,
    pub staged_edited: Option<String>,
    pub original: Option<EntryStamp>,
    pub edited: Option<EntryStamp>,
    pub failure: Option<String>,
}

impl ArchiveEditUndo {
    pub fn can_redo(&self) -> bool {
        self.staged_edited.is_some() && self.failure.is_none()
    }
}

// TODO(merge): replaced by crate::archives::ArchiveEditExecutor (`undo`, `redo`)
/// The archive-edit executor as the record drives it.
pub trait ArchiveEdits {
    fn undo(&self, applied: &ArchiveEditOutcome) -> ArchiveEditUndo;

    fn redo(&self, undone: &ArchiveEditUndo) -> ArchiveEditOutcome;
}

// TODO(merge): replaced by crate::archives::ArchiveEditExecutor::commit_staging
/// The markers the archive executor names its held containers with.
const REPLACED_MARKER: &str = ".bertbrowser-replaced-";
const EDITED_MARKER: &str = ".bertbrowser-edited-";

/// Erases a staged original once the edit can no longer be undone. Named the way the executor
/// names them, or it is not ours to erase: the guard every recursive delete in this codebase
/// carries, applied to a single file.
pub fn commit_applied(outcome: &ArchiveEditOutcome) {
    if let Some(staged) = &outcome.staged_original
        && file_name(staged).contains(REPLACED_MARKER)
    {
        let _ = std::fs::remove_file(staged);
    }
}

/// Erases a held edited container once the undone edit can no longer be redone.
pub fn commit_undone(undo: &ArchiveEditUndo) {
    if let Some(edited) = &undo.staged_edited
        && file_name(edited).contains(EDITED_MARKER)
    {
        let _ = std::fs::remove_file(edited);
    }
}

// TODO(merge): replaced by crate::archives::ArchiveEditExecutor
/// Stands in until the archive port lands: every step is refused, nothing is touched.
pub struct UnportedArchiveEdits;

impl ArchiveEdits for UnportedArchiveEdits {
    fn undo(&self, applied: &ArchiveEditOutcome) -> ArchiveEditUndo {
        ArchiveEditUndo {
            archive_file: applied.archive_file.clone(),
            staged_edited: None,
            original: None,
            edited: None,
            failure: Some("Archive editing is not available in this build.".to_string()),
        }
    }

    fn redo(&self, undone: &ArchiveEditUndo) -> ArchiveEditOutcome {
        ArchiveEditOutcome {
            failure: Some("Archive editing is not available in this build.".to_string()),
            ..ArchiveEditOutcome::nothing(&undone.archive_file)
        }
    }
}

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
            commit_applied(applied);
        } else if let Some(undone) = &self.undone {
            commit_undone(undone);
        }
    }
}
