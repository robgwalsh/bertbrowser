//! Deriving the second, elevated pass from the first one's failures, and folding the two results
//! back into a single outcome. Spec: Core/Services/Elevation/ElevatedRetry.cs.
//!
//! This is the whole of the feature's decision-making, and it is pure so that it can be held still
//! by a test. Everything else (the prompt, the pipe, the helper) is plumbing around it.
//!
//! **A merged outcome must be indistinguishable from one the executor produced alone.** That is
//! what lets the undo history, the staging commit, the tab refresh and the tab fan-out carry on
//! knowing nothing about elevation. It is also why the merge subtracts: an item that was denied and
//! then fixed must appear exactly once, in the completed list, and an item denied twice exactly
//! once, in the failures.

use std::collections::{HashMap, HashSet};
use std::fmt;

use super::rules::ElevationRules;
use crate::delete::{DeleteOutcome, DeletePlan, DeleteUndoResult, PlannedDelete};
use crate::newitem::{NewItemOutcome, NewItemPlan};
use crate::rename::{PlannedRename, RenameOutcome, RenamePlan};
use crate::transfer::{
    PlannedTransfer, Resolutions, TransferOutcome, TransferPlan, TransferUndoResult,
};

/// A transfer to run again with a token, and the failures it stands in for. `covers` holds the
/// canonical keys of the items in the first pass's failure list that this retry answers. Carried
/// rather than recomputed, because a merge otherwise has to guess which original failure a result
/// belongs to, and for a rename the two are not even the same path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferRetry {
    pub plan: TransferPlan,
    pub resolutions: Resolutions,
    pub covers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteRetry {
    pub plan: DeletePlan,
    pub covers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameRetry {
    pub plan: RenamePlan,
    pub covers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewItemRetry {
    pub plan: NewItemPlan,
}

/// The part of an undo to run again with a token, and the failures it stands in for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferUndoRetry {
    pub outcome: TransferOutcome,
    pub covers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteUndoRetry {
    pub outcome: DeleteOutcome,
    pub covers: Vec<String>,
}

/// The two passes disagree about the shape of the operation: an `ArgumentException` in the C#.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeError {
    pub what: &'static str,
}

impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "The two passes disagree about the {} of the operation.",
            self.what
        )
    }
}

impl std::error::Error for MergeError {}

pub struct ElevatedRetry;

impl ElevatedRetry {
    // --- deriving the retry ---

    /// The transfer to run again with a token, or None when there is nothing a token could fix.
    /// A cancelled run is never retryable: a consent prompt in front of somebody who has just
    /// pressed Cancel is the wrong answer whatever else is true.
    pub fn retry_for_transfer(
        plan: &TransferPlan,
        outcome: &TransferOutcome,
        resolutions: Option<&Resolutions>,
    ) -> Option<TransferRetry> {
        if outcome.cancelled {
            return None;
        }

        let covers = denied(&outcome.failed, |f| &f.source_path, |f| f.access_denied);
        if covers.is_empty() {
            return None;
        }

        let transfers: Vec<PlannedTransfer> = plan
            .transfers
            .iter()
            .filter(|t| {
                ElevationRules::key_of(&t.source_path).is_some_and(|key| covers.contains(&key))
            })
            .cloned()
            .collect();
        if transfers.is_empty() {
            return None;
        }

        let mut kept = Resolutions::new();
        if let Some(resolutions) = resolutions {
            // The chosen resolutions travel with the retry. Without them a Replace silently becomes
            // the default KeepBoth and the operation quietly changes meaning half way through.
            for transfer in &transfers {
                if let Some(key) = ElevationRules::key_of(&transfer.source_path)
                    && let Some(resolution) = resolutions.get(&key)
                {
                    kept.insert(key, *resolution);
                }
            }
        }

        // Rejected: empty on purpose. The planner's refusals are not permission problems, they were
        // reported once already, and carrying them would double them in the merged outcome.
        Some(TransferRetry {
            plan: TransferPlan::new(
                plan.verb,
                &plan.destination_directory,
                transfers,
                Vec::new(),
            ),
            resolutions: kept,
            covers: covers.into_iter().collect(),
        })
    }

    pub fn retry_for_delete(plan: &DeletePlan, outcome: &DeleteOutcome) -> Option<DeleteRetry> {
        let covers = denied(&outcome.failed, |f| &f.source_path, |f| f.access_denied);
        if covers.is_empty() {
            return None;
        }

        let deletions: Vec<PlannedDelete> = plan
            .deletions
            .iter()
            .filter(|d| {
                ElevationRules::key_of(&d.source_path).is_some_and(|key| covers.contains(&key))
            })
            .cloned()
            .collect();
        if deletions.is_empty() {
            return None;
        }

        // The mode travels; the per-item disposition is re-derived against live disk by the
        // executor anyway, so an item that lost its Recycle Bin between the passes is still handled
        // correctly.
        Some(DeleteRetry {
            plan: DeletePlan {
                mode: plan.mode,
                deletions,
                rejected: Vec::new(),
            },
            covers: covers.into_iter().collect(),
        })
    }

    pub fn retry_for_rename(plan: &RenamePlan, outcome: &RenameOutcome) -> Option<RenameRetry> {
        let denied: Vec<_> = outcome.failed.iter().filter(|f| f.access_denied).collect();
        if denied.is_empty() {
            return None;
        }

        // Where each denied item actually is. Usually its original path; a failed rename that could
        // not be put back leaves it parked under a staging name, and renaming from the path it no
        // longer occupies would fail and be reported as success.
        let mut actual: HashMap<String, String> = HashMap::new();
        let mut covers = Vec::new();
        for failure in denied {
            let Some(key) = ElevationRules::key_of(&failure.source_path) else {
                continue;
            };
            let from = failure
                .stranded_path
                .as_deref()
                .unwrap_or(&failure.source_path);
            if ElevationRules::is_refused_for_elevation(from) {
                continue;
            }
            actual.insert(key.clone(), from.to_string());
            covers.push(key);
        }
        if covers.is_empty() {
            return None;
        }

        let mut renames = Vec::new();
        for item in &plan.renames {
            let Some(key) = ElevationRules::key_of(&item.source_path) else {
                continue;
            };
            let Some(from) = actual.get(&key) else {
                continue;
            };
            renames.push(PlannedRename::new(
                from,
                &item.target_path,
                item.is_directory,
            ));
        }
        if renames.is_empty() {
            return None;
        }

        Some(RenameRetry {
            plan: RenamePlan::new(renames, Vec::new()),
            covers,
        })
    }

    /// There is only ever one item, which is why `NewItemPlan` carries one rejection rather than a
    /// list, so the retry is either the whole plan again or nothing.
    pub fn retry_for_new_item(
        plan: &NewItemPlan,
        outcome: &NewItemOutcome,
    ) -> Option<NewItemRetry> {
        if !outcome.failed.as_ref().is_some_and(|f| f.access_denied) {
            return None;
        }
        if !plan.has_work() {
            return None;
        }

        // Deliberately no is_refused_for_elevation here. That rule guards a protected folder from
        // being moved, renamed or destroyed; creating a file inside one is the ordinary thing a
        // file browser does, and it is what Explorer does with the same prompt behind it.
        Some(NewItemRetry { plan: plan.clone() })
    }

    // --- putting things back ---

    /// The part of an undo to run again with a token, or None when there is nothing a token could
    /// fix. An undo takes an *outcome*, not a plan, so this narrows the outcome rather than a plan:
    /// the completions whose sources came back denied, and nothing else.
    ///
    /// **`staging_directories` is deliberately emptied.** The unelevated half still holds items in
    /// its own staging folders, and the elevated pass's cleanup (which only removes an empty folder,
    /// but still) has no business running over them. `pruned_directories` is emptied for the same
    /// reason: the unelevated undo has already recreated those folders, and the elevated pass
    /// repeating it would be a second attempt at work that is done.
    pub fn undo_retry_for_transfer(
        outcome: &TransferOutcome,
        result: &TransferUndoResult,
    ) -> Option<TransferUndoRetry> {
        let covers = denied(&result.failed, |f| &f.source_path, |f| f.access_denied);
        if covers.is_empty() {
            return None;
        }

        let completed: Vec<_> = outcome
            .completed
            .iter()
            .filter(|c| {
                ElevationRules::key_of(&c.source_path).is_some_and(|key| covers.contains(&key))
            })
            .cloned()
            .collect();
        if completed.is_empty() {
            return None;
        }

        Some(TransferUndoRetry {
            outcome: TransferOutcome {
                completed,
                skipped: Vec::new(),
                failed: Vec::new(),
                staging_directories: Vec::new(),
                pruned_directories: Vec::new(),
                ..outcome.clone()
            },
            covers: covers.into_iter().collect(),
        })
    }

    pub fn undo_retry_for_delete(
        outcome: &DeleteOutcome,
        result: &DeleteUndoResult,
    ) -> Option<DeleteUndoRetry> {
        let covers = denied(&result.failed, |f| &f.source_path, |f| f.access_denied);
        if covers.is_empty() {
            return None;
        }

        let deleted: Vec<_> = outcome
            .deleted
            .iter()
            .filter(|d| {
                ElevationRules::key_of(&d.source_path).is_some_and(|key| covers.contains(&key))
            })
            .cloned()
            .collect();
        if deleted.is_empty() {
            return None;
        }

        Some(DeleteUndoRetry {
            outcome: DeleteOutcome {
                deleted,
                failed: Vec::new(),
                staging_directories: Vec::new(),
                permanent: outcome.permanent,
            },
            covers: covers.into_iter().collect(),
        })
    }

    /// **The elevated half's `reverted` is worked out here**, since the pipe reports a count and
    /// never a list: everything the retry was handed, less whatever it reported failing. A redo
    /// built from it then acts on the items that really did go back.
    pub fn merge_transfer_undo(
        first: &TransferUndoResult,
        retry: &TransferUndoRetry,
        second: &TransferUndoResult,
    ) -> TransferUndoResult {
        let mut reverted = first.reverted.clone();
        if !second.reverted.is_empty() {
            reverted.extend(second.reverted.iter().cloned());
        } else {
            let failed = keys(&second.failed, |f| &f.source_path);
            reverted.extend(
                survivors(&retry.outcome.completed, |c| &c.source_path, &failed)
                    .into_iter()
                    .cloned(),
            );
        }
        TransferUndoResult {
            restored: first.restored + second.restored,
            failed: survivors(&first.failed, |f| &f.source_path, &retry.covers)
                .into_iter()
                .cloned()
                .chain(second.failed.iter().cloned())
                .collect(),
            reverted,
        }
    }

    pub fn merge_delete_undo(
        first: &DeleteUndoResult,
        retry: &DeleteUndoRetry,
        second: &DeleteUndoResult,
    ) -> DeleteUndoResult {
        let mut reverted = first.reverted.clone();
        if !second.reverted.is_empty() {
            reverted.extend(second.reverted.iter().cloned());
        } else {
            let failed = keys(&second.failed, |f| &f.source_path);
            reverted.extend(
                survivors(&retry.outcome.deleted, |d| &d.source_path, &failed)
                    .into_iter()
                    .cloned(),
            );
        }
        DeleteUndoResult {
            restored: first.restored + second.restored,
            failed: survivors(&first.failed, |f| &f.source_path, &retry.covers)
                .into_iter()
                .cloned()
                .chain(second.failed.iter().cloned())
                .collect(),
            reverted,
        }
    }

    // --- folding the two results into one ---

    pub fn merge_transfer(
        first: &TransferOutcome,
        retry: &TransferRetry,
        second: &TransferOutcome,
    ) -> Result<TransferOutcome, MergeError> {
        require_same_shape(first.verb == second.verb, "verb")?;
        require_same_shape(
            ElevationRules::key_of(&first.destination_directory)
                == ElevationRules::key_of(&second.destination_directory),
            "destination",
        )?;

        Ok(TransferOutcome {
            verb: first.verb,
            destination_directory: first.destination_directory.clone(),
            completed: first
                .completed
                .iter()
                .chain(second.completed.iter())
                .cloned()
                .collect(),
            skipped: first
                .skipped
                .iter()
                .chain(second.skipped.iter())
                .cloned()
                .collect(),
            failed: survivors(&first.failed, |f| &f.source_path, &retry.covers)
                .into_iter()
                .cloned()
                .chain(second.failed.iter().cloned())
                .collect(),
            staging_directories: first
                .staging_directories
                .iter()
                .chain(second.staging_directories.iter())
                .cloned()
                .collect(),
            cancelled: first.cancelled || second.cancelled,
            // Carried across for the reason staging_directories is: a record dropped here is a
            // folder the undo can never put back, and every item that belonged in it would then
            // fail on the "its original folder is gone" guard. The elevated half never prunes
            // (its plan carries no list), so in practice this is always the first half's.
            pruned_directories: first
                .pruned_directories
                .iter()
                .chain(second.pruned_directories.iter())
                .cloned()
                .collect(),
            can_undo_copy: first.can_undo_copy || second.can_undo_copy,
        })
    }

    pub fn merge_delete(
        first: &DeleteOutcome,
        retry: &DeleteRetry,
        second: &DeleteOutcome,
    ) -> Result<DeleteOutcome, MergeError> {
        require_same_shape(first.permanent == second.permanent, "delete mode")?;

        Ok(DeleteOutcome {
            permanent: first.permanent,
            deleted: first
                .deleted
                .iter()
                .chain(second.deleted.iter())
                .cloned()
                .collect(),
            failed: survivors(&first.failed, |f| &f.source_path, &retry.covers)
                .into_iter()
                .cloned()
                .chain(second.failed.iter().cloned())
                .collect(),
            staging_directories: first
                .staging_directories
                .iter()
                .chain(second.staging_directories.iter())
                .cloned()
                .collect(),
        })
    }

    pub fn merge_rename(
        first: &RenameOutcome,
        retry: &RenameRetry,
        second: &RenameOutcome,
    ) -> RenameOutcome {
        // The retry renamed from wherever the item actually was, which for a stranded item is not
        // the path the user knows it by. The completed records are rewritten back onto the original
        // sources, so undo (which is this same execution with every path swapped) puts things back
        // where they came from rather than into a staging name.
        let mut origins: HashMap<String, String> = HashMap::new();
        for item in &first.failed {
            if let Some(stranded) = &item.stranded_path
                && let Some(key) = ElevationRules::key_of(stranded)
            {
                origins.insert(key, item.source_path.clone());
            }
        }

        let completed = second.completed.iter().map(|c| {
            match ElevationRules::key_of(&c.source_path).and_then(|key| origins.get(&key)) {
                Some(origin) => crate::rename::CompletedRename {
                    source_path: origin.clone(),
                    ..c.clone()
                },
                None => c.clone(),
            }
        });

        RenameOutcome {
            completed: first.completed.iter().cloned().chain(completed).collect(),
            failed: survivors(&first.failed, |f| &f.source_path, &retry.covers)
                .into_iter()
                .cloned()
                .chain(second.failed.iter().cloned())
                .collect(),
        }
    }

    /// One item, so there is nothing to fold: the second attempt is the answer, whichever way it
    /// went. Kept as a merge for symmetry with the other three, so a caller written from one of
    /// them reads correctly here.
    pub fn merge_new_item(
        _first: &NewItemOutcome,
        _retry: &NewItemRetry,
        second: &NewItemOutcome,
    ) -> NewItemOutcome {
        second.clone()
    }
}

// --- shared ---

/// Canonical keys of the failures a token could plausibly fix.
fn denied<T>(
    failures: &[T],
    path: impl Fn(&T) -> &str,
    access_denied: impl Fn(&T) -> bool,
) -> HashSet<String> {
    let mut keys = HashSet::new();
    for failure in failures {
        if !access_denied(failure) {
            continue;
        }
        if ElevationRules::is_refused_for_elevation(path(failure)) {
            continue;
        }
        if let Some(key) = ElevationRules::key_of(path(failure)) {
            keys.insert(key);
        }
    }
    keys
}

/// The first pass's failures that the retry did not answer for.
fn survivors<'a, T>(failed: &'a [T], path: impl Fn(&T) -> &str, covers: &[String]) -> Vec<&'a T> {
    let covered: HashSet<&str> = covers.iter().map(String::as_str).collect();
    failed
        .iter()
        .filter(|f| match ElevationRules::key_of(path(f)) {
            Some(key) => !covered.contains(key.as_str()),
            None => true,
        })
        .collect()
}

fn keys<T>(items: &[T], path: impl Fn(&T) -> &str) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| ElevationRules::key_of(path(i)))
        .collect()
}

fn require_same_shape(same: bool, what: &'static str) -> Result<(), MergeError> {
    if same {
        Ok(())
    } else {
        Err(MergeError { what })
    }
}
