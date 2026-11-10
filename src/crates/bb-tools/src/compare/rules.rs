//! What a verdict means to the rest of the app: what a row shows, what the filter keeps, and what
//! a sync would do about it. Pure, so every one of these answers is pinned by a test rather than
//! spread across view models. Spec: Core/Services/Compare/CompareRules.cs.

use super::models::{CompareRowState, CompareSide, CompareVerdict};

pub struct CompareRules;

impl CompareRules {
    /// How one side renders a verdict. The mirror is the point: a left-newer pair is "newer" on
    /// the left and "older" on the right, and a left-only pair has nothing at all to say on the
    /// right, because there is no row there to colour.
    pub fn row_state(verdict: CompareVerdict, side: CompareSide) -> CompareRowState {
        use CompareVerdict as V;
        match verdict {
            V::Unknown => CompareRowState::Unknown,
            V::Same => CompareRowState::Same,
            V::Differs => CompareRowState::Differs,
            V::LeftOnly => Self::only_on(side, CompareSide::Left),
            V::RightOnly => Self::only_on(side, CompareSide::Right),
            V::LeftNewer => Self::newer_on(side, CompareSide::Left),
            V::RightNewer => Self::newer_on(side, CompareSide::Right),
        }
    }

    fn only_on(side: CompareSide, holder: CompareSide) -> CompareRowState {
        if side == holder {
            CompareRowState::OnlyHere
        } else {
            CompareRowState::None
        }
    }

    fn newer_on(side: CompareSide, newer: CompareSide) -> CompareRowState {
        if side == newer {
            CompareRowState::Newer
        } else {
            CompareRowState::Older
        }
    }

    /// What "show only differences" keeps. **`Unknown` counts as a difference.** It is the absence
    /// of a match, not a match, and hiding it would be the one way a row that could not be
    /// compared disappears from the screen the user is about to sync from.
    pub fn is_difference(verdict: CompareVerdict) -> bool {
        verdict != CompareVerdict::Same
    }

    /// Verdicts a left-to-right sync answers by writing to the right. `RightNewer` is included
    /// because "make right match left" is not finished while the right side still holds a
    /// different file, but the planner leaves those actions unticked by default (see
    /// [`overwrites_newer`](Self::overwrites_newer)): overwriting the newer of two files is the
    /// one copy a user would not expect to have agreed to.
    pub fn would_copy(verdict: CompareVerdict) -> bool {
        matches!(
            verdict,
            CompareVerdict::LeftOnly
                | CompareVerdict::LeftNewer
                | CompareVerdict::RightNewer
                | CompareVerdict::Differs
        )
    }

    /// A copy that would replace a file the right side updated more recently.
    pub fn overwrites_newer(verdict: CompareVerdict) -> bool {
        verdict == CompareVerdict::RightNewer
    }

    /// Verdicts a left-to-right sync answers by removing from the right.
    pub fn would_delete(verdict: CompareVerdict) -> bool {
        verdict == CompareVerdict::RightOnly
    }

    /// Folds one child's verdict into the folder above it.
    ///
    /// A child never tells a folder *which* side it is missing from, only that something beneath
    /// it is not settled. So a child contributes one of three signals: unknown, different, or
    /// nothing. That is what stops a folder that exists on both sides being called "left only"
    /// because one file inside it is.
    ///
    /// The ranking is `Same < Differs < LeftOnly/RightOnly < Unknown`. `Unknown` outranking
    /// everything is deliberate: "differs" invites a copy, which is additive and safe, while
    /// "same" invites a delete. The only verdict that must never be arrived at by not knowing is
    /// `Same`.
    pub fn roll_up(folder: CompareVerdict, child: CompareVerdict) -> CompareVerdict {
        let signal = match child {
            CompareVerdict::Unknown => CompareVerdict::Unknown,
            CompareVerdict::Same => CompareVerdict::Same,
            _ => CompareVerdict::Differs,
        };

        if Self::rank(signal) > Self::rank(folder) {
            signal
        } else {
            folder
        }
    }

    fn rank(verdict: CompareVerdict) -> u8 {
        match verdict {
            CompareVerdict::Same => 0,
            CompareVerdict::Differs | CompareVerdict::LeftNewer | CompareVerdict::RightNewer => 1,
            CompareVerdict::LeftOnly | CompareVerdict::RightOnly => 2,
            CompareVerdict::Unknown => 3,
        }
    }

    /// One side's phrasing of a verdict, for a tooltip, a status column and the summary.
    pub fn describe(verdict: CompareVerdict, side: CompareSide) -> &'static str {
        match Self::row_state(verdict, side) {
            CompareRowState::Unknown => "Could not be compared",
            CompareRowState::Same => "Same",
            CompareRowState::OnlyHere => {
                if side == CompareSide::Left {
                    "Only on the left"
                } else {
                    "Only on the right"
                }
            }
            CompareRowState::Newer => "Newer",
            CompareRowState::Older => "Older",
            CompareRowState::Differs => "Differs",
            CompareRowState::None => "",
        }
    }
}
