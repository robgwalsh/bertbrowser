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
use crate::fs::{EntryStamp, StagedReplace};
use crate::rename::paths::directory_name;

// TODO(merge): replaced by crate::metadata::HeldVersion
/// One file whose other version is held beside it. Spec: Core/Services/Metadata/MetadataModels.cs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldVersion {
    pub path: String,
    pub held_path: String,
    pub in_place: EntryStamp,
}

// TODO(merge): replaced by crate::metadata::FailedMetadataEdit
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedMetadataEdit {
    pub path: String,
    pub message: String,
    pub access_denied: bool,
}

// TODO(merge): replaced by crate::metadata::MetadataEditOutcome
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct MetadataEditOutcome {
    pub edited: Vec<HeldVersion>,
    pub failed: Vec<FailedMetadataEdit>,
    pub cancelled: bool,
    /// Files the edit was offered to and which already were as it asked: not changed, not failed,
    /// and not in the history.
    pub unchanged: Vec<String>,
}

impl MetadataEditOutcome {
    pub fn can_undo(&self) -> bool {
        !self.edited.is_empty()
    }
}

// TODO(merge): replaced by crate::metadata::MetadataSwapResult
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct MetadataSwapResult {
    pub swapped: Vec<HeldVersion>,
    pub failed: Vec<FailedMetadataEdit>,
}

// TODO(merge): replaced by crate::metadata::MetadataEditExecutor (`undo`, `redo`)
/// The metadata executor as the record drives it.
pub trait MetadataEdits {
    /// Puts the originals back, holding each edited file so the edit can be redone.
    fn undo(&self, edited: &[HeldVersion]) -> MetadataSwapResult;

    /// Takes undone edits back in, holding each original again.
    fn redo(&self, restored: &[HeldVersion]) -> MetadataSwapResult;
}

// TODO(merge): replaced by crate::metadata::MetadataEditExecutor::commit_staging
/// The markers the metadata executor names its held versions with.
const REPLACED_MARKER: &str = ".bertbrowser-replaced-";
const EDITED_MARKER: &str = ".bertbrowser-edited-";

/// Erases held versions once the step that needs them can no longer be taken: when the undo
/// history releases the entry, or the session ends. Named the way the executor names them, or it
/// is not ours to erase.
pub fn commit_staging(held: &[HeldVersion]) {
    for version in held {
        if !StagedReplace::is_marked(&version.held_path, REPLACED_MARKER)
            && !StagedReplace::is_marked(&version.held_path, EDITED_MARKER)
        {
            continue;
        }
        let _ = std::fs::remove_file(&version.held_path);
    }
}

// TODO(merge): replaced by crate::metadata::MetadataEditExecutor
/// Stands in until the metadata port lands: every swap fails, nothing is touched.
pub struct UnportedMetadataEdits;

impl MetadataEdits for UnportedMetadataEdits {
    fn undo(&self, edited: &[HeldVersion]) -> MetadataSwapResult {
        Self::refuse(edited)
    }

    fn redo(&self, restored: &[HeldVersion]) -> MetadataSwapResult {
        Self::refuse(restored)
    }
}

impl UnportedMetadataEdits {
    fn refuse(held: &[HeldVersion]) -> MetadataSwapResult {
        MetadataSwapResult {
            swapped: Vec::new(),
            failed: held
                .iter()
                .map(|h| FailedMetadataEdit {
                    path: h.path.clone(),
                    message: format!(
                        "{}: metadata editing is not available in this build.",
                        file_name(&h.path)
                    ),
                    access_denied: false,
                })
                .collect(),
        }
    }
}

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
        commit_staging(&self.held);
    }
}
