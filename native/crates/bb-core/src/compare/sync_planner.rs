//! Turning a comparison into what a one-way sync would do, and then into the plans the existing
//! executors already know how to carry out. Pure: it decides, and writes nothing. Spec:
//! Core/Services/Compare/SyncPlanner.cs.

use std::collections::{HashMap, HashSet};

use bb_paths::{PathKey, fold_upper, parent_key};

use super::models::{CompareEntry, CompareKeys, CompareSide, CompareVerdict};
use super::rules::CompareRules;
use super::service::FolderCompareOutcome;
use super::sync_models::{SyncAction, SyncActionKind, SyncPlans, SyncPreview};
use super::transfer_stubs::{
    ConflictResolution, DeleteMode, DeletePlan, DeletePlanning, DeleteSource, TransferPlan,
    TransferPlanning, TransferVerb,
};

pub struct SyncPlanner;

impl SyncPlanner {
    /// What making the right side match the left would take. `remove_right_only` is whether to
    /// offer deleting what only the right side has: off is the safe default and the one the
    /// dialog opens with, since everything else a sync does is additive or reversible in place,
    /// and this is the half that takes something away.
    ///
    /// **A folder is acted on whole or not at all.** A folder the right side lacks becomes one
    /// action covering everything inside it, and nothing under it gets an action of its own,
    /// which is what lets the executor recurse into it as a single copy and removes the question
    /// of who creates the intermediate folders. An entry no verdict could be reached for produces
    /// no action at all, and is counted instead.
    pub fn preview(compare: &FolderCompareOutcome, remove_right_only: bool) -> SyncPreview {
        if !compare.can_sync() {
            return SyncPreview::empty(&compare.left_path, &compare.right_path);
        }

        let result = &compare.result;
        let mut keys: Vec<&String> = result.by_relative_key.keys().collect();
        keys.sort();
        let left_bytes = Self::subtree_bytes(&result.left);
        let right_bytes = Self::subtree_bytes(&result.right);

        let mut actions: Vec<SyncAction> = Vec::new();
        let mut covered: Vec<&str> = Vec::new(); // keys already acted on, whole
        let mut unknown = 0;

        for key in keys {
            // A folder both sides have is opened, never acted on. Its verdict is the roll-up of
            // what is inside it, so treating that "differs" as something to write would replace a
            // whole tree to sync one file in it, and treating an "unknown" rolled up from one
            // descendant as an uncomparable entry would count the same doubt once per folder
            // above it. The same predicate the banner counts through, so what it offers to do
            // and what it said it found cannot disagree.
            if result.is_folder_on_both_sides(key) {
                continue;
            }

            let verdict = result.verdict_for(key);
            if verdict == CompareVerdict::Unknown {
                unknown += 1;
                continue;
            }

            // One action settles everything beneath the path it names, whichever kind it is: a
            // copy and an overwrite each write the whole of it, and a delete removes the whole of
            // it. Sorted ordinally, an ancestor is a strict prefix and so has already been seen.
            // The awkward case this covers is a file arriving where the right side keeps a folder:
            // the overwrite stages that folder away entire, so offering to delete what is inside
            // it as well would be an action against a path that will not be there.
            if covered
                .iter()
                .any(|acted| CompareKeys::is_at_or_under(key, acted))
            {
                continue;
            }

            if CompareRules::would_copy(verdict) {
                let display = result.display_path(key, CompareSide::Left);
                let entry = &result.left[key];
                let kind = if result.right.contains_key(key) {
                    SyncActionKind::Overwrite
                } else {
                    SyncActionKind::Copy
                };

                actions.push(SyncAction {
                    relative_key: key.clone(),
                    source_path: combine(&compare.left_path, &display),
                    target_path: combine(&compare.right_path, &display),
                    relative_display: display,
                    kind,
                    is_directory: entry.is_directory,
                    bytes: Self::weigh(entry, key, &left_bytes),
                    verdict,
                    // Overwriting a file the right side updated more recently is the one write a
                    // user would not expect to have agreed to by asking for a sync, so it is shown
                    // and left for them to tick.
                    ticked: !CompareRules::overwrites_newer(verdict),
                });

                covered.push(key);
            } else if remove_right_only && CompareRules::would_delete(verdict) {
                let display = result.display_path(key, CompareSide::Right);
                let entry = &result.right[key];

                actions.push(SyncAction {
                    relative_key: key.clone(),
                    source_path: String::new(),
                    target_path: combine(&compare.right_path, &display),
                    relative_display: display,
                    kind: SyncActionKind::Delete,
                    is_directory: entry.is_directory,
                    bytes: Self::weigh(entry, key, &right_bytes),
                    verdict,
                    ticked: true,
                });

                covered.push(key);
            }
        }

        SyncPreview {
            left_path: compare.left_path.clone(),
            right_path: compare.right_path.clone(),
            actions,
            unknown_count: unknown,
        }
    }

    /// Re-ticks a preview, so a dialog can hand back what the user changed without the planner
    /// having to know what a checkbox is.
    pub fn with_ticks(preview: &SyncPreview, ticked_keys: &HashSet<String>) -> SyncPreview {
        SyncPreview {
            actions: preview
                .actions
                .iter()
                .map(|a| SyncAction {
                    ticked: ticked_keys.contains(&a.relative_key),
                    ..a.clone()
                })
                .collect(),
            ..preview.clone()
        }
    }

    /// The ticked actions, as one transfer plan per destination folder plus one delete plan.
    ///
    /// Every copy is given [`ConflictResolution::Overwrite`], including the ones the comparison
    /// found a free name for. A sync says "make this side match", and a file that appeared on the
    /// right between the scan and the run is a name to take over rather than one to sidestep:
    /// `KeepBoth` would leave a silent "x (2)" behind and call the sync done. Nothing is lost by
    /// it: the displaced entry goes to staging and the whole run is undoable. Copies are grouped
    /// by destination folder and ordered parent-first.
    pub fn to_plans(
        preview: &SyncPreview,
        transfers: &dyn TransferPlanning,
        deletes: &dyn DeletePlanning,
        mode: DeleteMode,
    ) -> SyncPlans {
        let mut refused: Vec<String> = Vec::new();
        let mut resolutions: HashMap<String, ConflictResolution> = HashMap::new();
        let mut plans: Vec<TransferPlan> = Vec::new();

        // Grouped case-insensitively, keeping the first spelling seen, as GroupBy with an
        // ignore-case comparer did.
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        let mut group_index: HashMap<String, usize> = HashMap::new();
        for action in preview.ticked() {
            if action.kind == SyncActionKind::Delete {
                continue;
            }
            let directory = parent_key(&action.target_path).unwrap_or_default();
            let folded = fold_upper(&directory);
            let index = *group_index.entry(folded).or_insert_with(|| {
                groups.push((directory, Vec::new()));
                groups.len() - 1
            });
            groups[index].1.push(action.source_path.clone());
        }
        groups.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then_with(|| a.0.cmp(&b.0)));

        for (destination, sources) in &groups {
            let plan = transfers.plan(sources, destination, TransferVerb::Copy);

            refused.extend(plan.problems().map(|p| p.message.clone()));

            if !plan.has_work() {
                continue;
            }

            for transfer in &plan.transfers {
                let key = PathKey::canonicalize(&transfer.source_path)
                    .unwrap_or_else(|_| fold_upper(&transfer.source_path));
                resolutions.insert(key, ConflictResolution::Overwrite);
            }

            plans.push(plan);
        }

        let removals: Vec<DeleteSource> = preview
            .ticked()
            .into_iter()
            .filter(|a| a.kind == SyncActionKind::Delete)
            .map(|a| DeleteSource {
                path: a.target_path.clone(),
                is_directory: a.is_directory,
            })
            .collect();
        let delete_plan = if removals.is_empty() {
            DeletePlan::empty(mode)
        } else {
            deletes.plan(&removals, mode)
        };

        refused.extend(delete_plan.problems().map(|p| p.message.clone()));

        SyncPlans {
            copies: plans,
            resolutions,
            removals: delete_plan,
            refused,
        }
    }

    /// What each folder's contents weigh, folded up the same way a verdict is. The listings are
    /// already in hand, so this is exact rather than an estimate, and it is the reason a sync can
    /// show a byte total for a folder at all: `dir_size_cache` holds a number only for volumes the
    /// elevated pass has measured, which is precisely not the case for the backup drive on the
    /// other side of the comparison.
    fn subtree_bytes(side: &HashMap<String, CompareEntry>) -> HashMap<String, i64> {
        let mut totals: HashMap<String, i64> = HashMap::new();
        for (key, entry) in side {
            if entry.is_directory {
                continue;
            }
            for ancestor in CompareKeys::ancestors(key) {
                *totals.entry(ancestor.to_string()).or_insert(0) += entry.size_bytes;
            }
        }
        totals
    }

    /// A file weighs what it says; a folder weighs its contents, or nothing when it has none.
    /// Never None in practice (a file with no measured length has no verdict either) but the type
    /// says so rather than the comment alone.
    fn weigh(entry: &CompareEntry, key: &str, subtree_bytes: &HashMap<String, i64>) -> Option<i64> {
        Some(if entry.is_directory {
            subtree_bytes.get(key).copied().unwrap_or(0)
        } else {
            entry.size_bytes
        })
    }
}

/// `Path.Combine(root, relative)` for a relative path that is never rooted.
fn combine(root: &str, relative: &str) -> String {
    if root.ends_with('\\') || root.ends_with('/') {
        format!("{root}{relative}")
    } else {
        format!("{root}\\{relative}")
    }
}
