//! A folder sync in the undo history: copies and removals at once, reversed and redone together by
//! `SyncRunner`. Never retried elevated, as a sync never was.
//! Spec: Core/Services/UndoHistory/SyncRecord.cs.

use bb_paths::file_name;

use super::host::UndoHost;
use super::models::{HeldEntry, StepReport, StepResult, UndoItemDetail, UndoKind, UndoRefresh};
use super::record::UndoableRecord;
use super::text::UndoText;
use crate::cancel::CancelToken;
use crate::compare::{SyncOutcome, SyncRunner};
use crate::rename::paths::{directory_exists, directory_name};
use crate::transfer::{PlannedTransfer, TransferExecutor, TransferPlan, TransferVerb};

const RECYCLE_BIN: &str = "Recycle Bin";
const HOLDING_FOLDER: &str = "BertBrowser's holding folder";

#[derive(Clone)]
pub struct SyncRecord {
    outcome: SyncOutcome,
    is_applied: bool,
    description: String,
}

impl SyncRecord {
    pub fn for_outcome(outcome: SyncOutcome) -> Option<SyncRecord> {
        if !outcome.can_undo() {
            return None;
        }

        let into = match outcome.copies.first() {
            Some(copy) => format!(
                " into {}",
                UndoText::folder_name(&copy.destination_directory)
            ),
            None => String::new(),
        };
        let description = format!("Sync {}{into}", UndoText::items(Self::count(&outcome)));
        Some(SyncRecord {
            outcome,
            is_applied: true,
            description,
        })
    }

    fn count(outcome: &SyncOutcome) -> usize {
        outcome
            .copies
            .iter()
            .map(|c| c.completed.len())
            .sum::<usize>()
            + outcome.removals.deleted.len()
    }

    fn refresh_for(outcome: &SyncOutcome) -> UndoRefresh {
        let mut refresh = UndoRefresh::of(
            outcome
                .copies
                .iter()
                .flat_map(|c| c.completed.iter().map(|t| directory_name(&t.final_path)))
                .chain(
                    outcome
                        .copies
                        .iter()
                        .map(|c| Some(c.destination_directory.clone())),
                )
                .chain(
                    outcome
                        .removals
                        .deleted
                        .iter()
                        .map(|d| directory_name(&d.source_path)),
                ),
        );
        refresh.rescan_compare = true;
        refresh
    }

    fn undo(&self, host: &dyn UndoHost) -> StepResult {
        let undo = host.sync().undo(&self.outcome, &CancelToken::none());
        let report = StepReport::new(Self::count(&undo.reverted), undo.failed);
        StepResult::new(
            Box::new(SyncRecord {
                outcome: undo.reverted,
                is_applied: false,
                description: self.description.clone(),
            }),
            report,
            Self::refresh_for(&self.outcome),
        )
    }

    fn redo(&self, host: &dyn UndoHost) -> StepResult {
        let whole = TransferPlan::new(
            TransferVerb::Copy,
            self.outcome
                .copies
                .first()
                .map(|c| c.destination_directory.as_str())
                .unwrap_or(""),
            self.outcome
                .copies
                .iter()
                .flat_map(|c| &c.completed)
                .map(|c| PlannedTransfer::new(&c.source_path, c.is_directory, &c.final_path, false))
                .collect(),
            Vec::new(),
        );

        let mut progress = host.begin_progress(&whole, &format!("Redoing — {}…", self.description));
        let mut redone =
            host.sync()
                .redo(&self.outcome, &CancelToken::none(), progress.as_deref_mut());
        drop(progress);
        host.end_progress();

        redone.copies = redone
            .copies
            .into_iter()
            .map(TransferExecutor::stamp_copies)
            .collect();
        let failures: Vec<String> = redone
            .copies
            .iter()
            .flat_map(|c| {
                c.failed
                    .iter()
                    .map(|f| f.message.clone())
                    .chain(c.skipped.iter().map(|s| {
                        format!(
                            "{}: its old place is taken now, so it was left where it is.",
                            file_name(s)
                        )
                    }))
                    .collect::<Vec<_>>()
            })
            .chain(redone.removals.failed.iter().map(|f| f.message.clone()))
            .collect();

        let report = StepReport::new(Self::count(&redone), failures);
        let refresh = Self::refresh_for(&redone);
        StepResult::new(
            Box::new(SyncRecord {
                outcome: redone,
                is_applied: true,
                description: self.description.clone(),
            }),
            report,
            refresh,
        )
    }
}

impl UndoableRecord for SyncRecord {
    fn kind(&self) -> UndoKind {
        UndoKind::Sync
    }

    fn is_applied(&self) -> bool {
        self.is_applied
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn item_count(&self) -> usize {
        Self::count(&self.outcome)
    }

    fn details(&self) -> Vec<UndoItemDetail> {
        self.outcome
            .copies
            .iter()
            .flat_map(|c| &c.completed)
            .map(|c| UndoItemDetail::new(&c.source_path, &c.final_path, c.is_directory))
            .chain(self.outcome.removals.deleted.iter().map(|d| {
                let to = if d.recycled_path.is_some() {
                    RECYCLE_BIN
                } else {
                    HOLDING_FOLDER
                };
                UndoItemDetail::new(&d.source_path, to, d.is_directory)
            }))
            .collect()
    }

    fn held(&self) -> Vec<HeldEntry> {
        if !self.is_applied {
            return Vec::new();
        }
        self.outcome
            .copies
            .iter()
            .flat_map(|c| &c.completed)
            .filter_map(|c| {
                c.displaced_stage_path
                    .as_ref()
                    .map(|staged| HeldEntry::new(staged, &c.final_path, directory_exists(staged)))
            })
            .chain(self.outcome.removals.deleted.iter().filter_map(|d| {
                d.staged_path
                    .as_ref()
                    .map(|staged| HeldEntry::new(staged, &d.source_path, d.is_directory))
            }))
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
            SyncRunner::retire(&self.outcome);
        }
    }
}
