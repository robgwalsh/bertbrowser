//! Pairing two flat subtree listings and judging every pair. Spec:
//! Core/Services/Compare/FolderComparer.cs.
//!
//! Shaped after `FileListDiff`, which does the same job for one folder across time: pure, keyed
//! on a canonical path, and returning a description of the difference rather than acting on it.
//! Everything that decides what a difference *means* is in `CompareEquality` and `CompareRules`;
//! this only pairs and folds. A folder's verdict is the roll-up of everything beneath it, which
//! is what lets a whole tree be synced without opening it. The fold needs no recursion and no
//! particular input order: each entry's verdict is merged into each of its ancestors, and
//! `CompareRules::roll_up` is order-independent.

use std::collections::HashMap;

use super::equality::{CompareEquality, CompareTolerance};
use super::models::{CompareEntry, CompareKeys, CompareSide, CompareVerdict};
use super::rules::CompareRules;

/// What comparing two trees found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareResult {
    /// A verdict for every path either side holds, folders included.
    pub by_relative_key: HashMap<String, CompareVerdict>,
    /// The left listing, keyed the same way.
    pub left: HashMap<String, CompareEntry>,
    /// The right listing.
    pub right: HashMap<String, CompareEntry>,
    /// Entries that match.
    pub same_count: usize,
    /// Entries that do not, excluding the ones nothing is known about.
    pub difference_count: usize,
    /// Entries that could not be compared. Counted separately because they are neither a match
    /// nor something a sync may act on.
    pub unknown_count: usize,
}

impl CompareResult {
    pub fn none() -> CompareResult {
        CompareResult {
            by_relative_key: HashMap::new(),
            left: HashMap::new(),
            right: HashMap::new(),
            same_count: 0,
            difference_count: 0,
            unknown_count: 0,
        }
    }

    pub fn any_difference(&self) -> bool {
        self.difference_count > 0 || self.unknown_count > 0
    }

    /// The verdict for a path, or `Unknown` when the comparison never saw it: a file created since
    /// the scan has not been compared, and saying so is the only honest answer.
    pub fn verdict_for(&self, relative_key: &str) -> CompareVerdict {
        self.by_relative_key
            .get(relative_key)
            .copied()
            .unwrap_or(CompareVerdict::Unknown)
    }

    /// The relative path in the casing its side recorded, rebuilt from the ancestors' names.
    ///
    /// The one place a display path is assembled, so a dialog and a status column cannot disagree
    /// about how a path is spelled. A key whose folder row is missing (the index can hold a file
    /// whose parent was swept) falls back to that segment of the uppercased key: ugly, but still
    /// pointing at the right file. Asked for a handful of entries, never for all of them, which
    /// is why no side stores one per row.
    pub fn display_path(&self, relative_key: &str, side: CompareSide) -> String {
        let names = match side {
            CompareSide::Left => &self.left,
            CompareSide::Right => &self.right,
        };

        let mut segments: Vec<&str> = Vec::new();
        let mut key = relative_key;
        while !key.is_empty() {
            let cut = key.rfind(CompareKeys::SEPARATOR);
            segments.push(match names.get(key) {
                Some(entry) => entry.name.as_str(),
                None => &key[cut.map_or(0, |c| c + 1)..],
            });
            match cut {
                Some(c) => key = &key[..c],
                None => break,
            }
        }

        segments.reverse();
        segments.join("\\")
    }

    /// Whether this path is answered by something else rather than on its own.
    ///
    /// Two things answer for a path. A folder both sides have carries only the roll-up of what is
    /// inside it, so counting it as well as its contents would report one changed file as two
    /// differences. And everything under a folder one side is missing entirely is missing for the
    /// same single reason, so a new folder of two hundred files is one thing to say, not two
    /// hundred and one. Both are also exactly what a sync does about them (one action, or none),
    /// which is what keeps the summary and the preview from disagreeing.
    pub fn is_answered_by_an_ancestor(&self, relative_key: &str) -> bool {
        if self.is_folder_on_both_sides(relative_key) {
            return true;
        }

        CompareKeys::ancestors(relative_key).any(|ancestor| {
            matches!(
                self.verdict_for(ancestor),
                CompareVerdict::LeftOnly | CompareVerdict::RightOnly
            )
        })
    }

    /// Whether a key is a folder both sides hold: the rows whose verdict is a roll-up rather than
    /// their own.
    pub(crate) fn is_folder_on_both_sides(&self, key: &str) -> bool {
        self.left.get(key).is_some_and(|l| l.is_directory)
            && self.right.get(key).is_some_and(|r| r.is_directory)
    }

    /// How many entries carry this verdict in their own right.
    pub fn count(&self, verdict: CompareVerdict) -> usize {
        self.by_relative_key
            .iter()
            .filter(|(key, value)| **value == verdict && !self.is_answered_by_an_ancestor(key))
            .count()
    }
}

pub struct FolderComparer;

impl FolderComparer {
    pub fn compare(
        left: &[CompareEntry],
        right: &[CompareEntry],
        tolerance: CompareTolerance,
    ) -> CompareResult {
        let left_by_key = Self::index(left);
        let right_by_key = Self::index(right);

        let mut verdicts: HashMap<String, CompareVerdict> =
            HashMap::with_capacity(left_by_key.len() + right_by_key.len());

        for (key, entry) in &left_by_key {
            verdicts.insert(
                key.clone(),
                CompareEquality::verdict(Some(entry), right_by_key.get(key), tolerance),
            );
        }

        for (key, entry) in &right_by_key {
            if left_by_key.contains_key(key) {
                continue; // already judged from the left
            }
            verdicts.insert(
                key.clone(),
                CompareEquality::verdict(None, Some(entry), tolerance),
            );
        }

        Self::roll_up_folders(&mut verdicts);

        Self::finish(verdicts, left_by_key, right_by_key)
    }

    /// Counts and packages a settled verdict map. Raw over every key, not through
    /// [`CompareResult::count`], which is the ancestor-aware count the banner uses.
    pub(crate) fn finish(
        verdicts: HashMap<String, CompareVerdict>,
        left: HashMap<String, CompareEntry>,
        right: HashMap<String, CompareEntry>,
    ) -> CompareResult {
        let mut same = 0;
        let mut unknown = 0;
        for verdict in verdicts.values() {
            match verdict {
                CompareVerdict::Same => same += 1,
                CompareVerdict::Unknown => unknown += 1,
                _ => {}
            }
        }

        let total = verdicts.len();
        CompareResult {
            by_relative_key: verdicts,
            left,
            right,
            same_count: same,
            difference_count: total - same - unknown,
            unknown_count: unknown,
        }
    }

    /// Merges every entry's verdict into each of its ancestors. Reads the verdicts settled by
    /// pairing and writes only to ancestor keys, so no entry is ever folded into itself and the
    /// pass does not depend on the order the map happens to iterate in.
    pub(crate) fn roll_up_folders(verdicts: &mut HashMap<String, CompareVerdict>) {
        let direct: Vec<(String, CompareVerdict)> =
            verdicts.iter().map(|(k, v)| (k.clone(), *v)).collect();
        for (key, verdict) in &direct {
            for ancestor in CompareKeys::ancestors(key) {
                // A listing that omits a folder row but holds its contents still gets a verdict
                // for the folder, seeded neutral so its children are what decide it.
                let folder = verdicts
                    .get(ancestor)
                    .copied()
                    .unwrap_or(CompareVerdict::Same);
                verdicts.insert(
                    ancestor.to_string(),
                    CompareRules::roll_up(folder, *verdict),
                );
            }
        }
    }

    fn index(entries: &[CompareEntry]) -> HashMap<String, CompareEntry> {
        let mut by_key = HashMap::with_capacity(entries.len());
        for entry in entries {
            by_key.insert(entry.relative_key.clone(), entry.clone());
        }
        by_key
    }
}
