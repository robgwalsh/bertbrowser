//! Settling a comparison's verdict by what the bytes actually say. Spec:
//! Core/Services/Compare/ContentSettlement.cs.
//!
//! A folder comparison judges on timestamps, so it can say two files are probably different but
//! never that they are the same. Reading both is the only thing that can, and this is the rule
//! for what that reading is allowed to change.
//!
//! **One direction only, and only on proof.** Identical bytes are stronger evidence than any
//! timestamp, so they may raise a verdict to `Same`, including from `Unknown`: an unknown that has
//! been read is no longer unknown. Everything else changes nothing. A comparison that could not
//! read a side is `Unreadable` and is never evidence of anything, since `Same` is what authorises
//! a delete and every doubt resolves away from it. `Differs` is handled for completeness and will
//! not move in practice: at a leaf it means equal timestamps and *unequal sizes*.
//!
//! **This produces a new `CompareResult` rather than an overlay on the view.** `SyncPlanner` reads
//! its verdicts straight off the result, so a settlement kept beside the rows would repaint one
//! green while the sync went on copying it.

use std::borrow::Cow;
use std::collections::HashMap;

use super::file_content_comparer::ContentVerdict;
use super::folder_comparer::{CompareResult, FolderComparer};
use super::models::CompareVerdict;

pub struct ContentSettlement;

impl ContentSettlement {
    /// What a verdict becomes once the bytes have been read.
    pub fn settle(current: CompareVerdict, content: ContentVerdict) -> CompareVerdict {
        if content == ContentVerdict::Identical {
            CompareVerdict::Same
        } else {
            current
        }
    }

    /// A copy of `result` with those keys settled and every folder above them re-folded; the
    /// result itself, untouched, when there is nothing to settle. Keys the comparison never saw
    /// are ignored rather than invented.
    ///
    /// The roll-ups are rebuilt from scratch, not patched. `CompareRules::roll_up` only ever
    /// raises rank, so it cannot walk a folder back down from `Differs` to `Same` when its last
    /// differing child is settled, and that folder turning green is the visible point of the whole
    /// thing. Every folder is seeded from what it is on its own, then the same fold the original
    /// comparison used puts the children back.
    pub fn apply<'a>(
        result: &'a CompareResult,
        settled: &HashMap<String, ContentVerdict>,
    ) -> Cow<'a, CompareResult> {
        if settled.is_empty() {
            return Cow::Borrowed(result);
        }

        let mut verdicts: HashMap<String, CompareVerdict> =
            HashMap::with_capacity(result.by_relative_key.len());

        for (key, verdict) in &result.by_relative_key {
            let value = if result.is_folder_on_both_sides(key) {
                // Seeded neutral; its children decide it, exactly as in the original fold.
                CompareVerdict::Same
            } else {
                match settled.get(key) {
                    Some(content) => Self::settle(*verdict, *content),
                    None => *verdict,
                }
            };
            verdicts.insert(key.clone(), value);
        }

        FolderComparer::roll_up_folders(&mut verdicts);

        // Counted exactly as FolderComparer::compare counts: raw over every key, not through
        // CompareResult::count, the ancestor-aware count the banner uses. Mixing the two would
        // quietly change what these three numbers mean the moment anything was settled.
        Cow::Owned(FolderComparer::finish(
            verdicts,
            result.left.clone(),
            result.right.clone(),
        ))
    }
}
