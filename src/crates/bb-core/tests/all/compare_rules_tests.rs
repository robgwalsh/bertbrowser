// Port of tests/BertBrowser.Core.Tests/CompareRulesTests.cs
//
// The judgements a comparison makes about what to show and what a sync may touch. Pure, and held
// still here because every one of them is read from a colour on a row rather than from a number.
use bb_core::compare::{CompareRowState, CompareRules, CompareSide, CompareVerdict};

// --- The per-side projection ---

/// The mirror is the whole point of having a row state at all: one verdict, two opposite
/// readings, so neither list ever tells the user the other list's story.
#[test]
fn newer_on_one_side_reads_as_older_on_the_other() {
    assert_eq!(
        CompareRowState::Newer,
        CompareRules::row_state(CompareVerdict::LeftNewer, CompareSide::Left)
    );
    assert_eq!(
        CompareRowState::Older,
        CompareRules::row_state(CompareVerdict::LeftNewer, CompareSide::Right)
    );
    assert_eq!(
        CompareRowState::Newer,
        CompareRules::row_state(CompareVerdict::RightNewer, CompareSide::Right)
    );
    assert_eq!(
        CompareRowState::Older,
        CompareRules::row_state(CompareVerdict::RightNewer, CompareSide::Left)
    );
}

/// There is no row on the other side to colour, so there is nothing to say there.
#[test]
fn only_on_one_side_says_nothing_on_the_other() {
    assert_eq!(
        CompareRowState::OnlyHere,
        CompareRules::row_state(CompareVerdict::LeftOnly, CompareSide::Left)
    );
    assert_eq!(
        CompareRowState::None,
        CompareRules::row_state(CompareVerdict::LeftOnly, CompareSide::Right)
    );
    assert_eq!(
        CompareRowState::OnlyHere,
        CompareRules::row_state(CompareVerdict::RightOnly, CompareSide::Right)
    );
    assert_eq!(
        CompareRowState::None,
        CompareRules::row_state(CompareVerdict::RightOnly, CompareSide::Left)
    );
}

#[test]
fn symmetric_verdicts_read_the_same_on_both_sides() {
    for (verdict, expected) in [
        (CompareVerdict::Same, CompareRowState::Same),
        (CompareVerdict::Differs, CompareRowState::Differs),
        (CompareVerdict::Unknown, CompareRowState::Unknown),
    ] {
        assert_eq!(
            expected,
            CompareRules::row_state(verdict, CompareSide::Left),
            "{verdict:?}"
        );
        assert_eq!(
            expected,
            CompareRules::row_state(verdict, CompareSide::Right),
            "{verdict:?}"
        );
    }
}

// --- What the filter keeps ---

/// "Show only differences" hiding an uncomparable row would be the one way a row nothing is known
/// about disappears from the screen the user is about to sync from.
#[test]
fn an_unknown_survives_the_differences_filter() {
    assert!(CompareRules::is_difference(CompareVerdict::Unknown));
    assert!(!CompareRules::is_difference(CompareVerdict::Same));
}

// --- What a sync would do ---

/// You cannot sync what you could not compare, in either direction.
#[test]
fn an_unknown_is_never_acted_on() {
    assert!(!CompareRules::would_copy(CompareVerdict::Unknown));
    assert!(!CompareRules::would_delete(CompareVerdict::Unknown));
}

#[test]
fn only_the_right_only_side_is_ever_deleted() {
    assert!(CompareRules::would_delete(CompareVerdict::RightOnly));

    for verdict in CompareVerdict::ALL {
        if verdict == CompareVerdict::RightOnly {
            continue;
        }
        assert!(!CompareRules::would_delete(verdict), "{verdict:?}");
    }
}

#[test]
fn every_unsettled_pair_is_copied() {
    for verdict in [
        CompareVerdict::LeftOnly,
        CompareVerdict::LeftNewer,
        CompareVerdict::RightNewer,
        CompareVerdict::Differs,
    ] {
        assert!(CompareRules::would_copy(verdict), "{verdict:?}");
    }
}

#[test]
fn a_match_is_never_copied() {
    assert!(!CompareRules::would_copy(CompareVerdict::Same));
    assert!(!CompareRules::would_copy(CompareVerdict::RightOnly));
}

/// The one copy a user would not expect to have agreed to, flagged so the planner can leave it
/// unticked.
#[test]
fn overwriting_the_newer_file_is_singled_out() {
    assert!(CompareRules::overwrites_newer(CompareVerdict::RightNewer));
    assert!(!CompareRules::overwrites_newer(CompareVerdict::LeftNewer));
    assert!(!CompareRules::overwrites_newer(CompareVerdict::LeftOnly));
}

// --- The fold ---

/// A child only ever reports that something beneath is unsettled, never which side it is missing
/// from. Without that, one left-only file would rename its whole parent folder "left only" and a
/// sync would try to copy a folder that is already there.
#[test]
fn a_child_never_tells_its_folder_which_side_is_missing() {
    assert_eq!(
        CompareVerdict::Differs,
        CompareRules::roll_up(CompareVerdict::Same, CompareVerdict::LeftOnly)
    );

    assert_eq!(
        CompareVerdict::Differs,
        CompareRules::roll_up(CompareVerdict::Same, CompareVerdict::RightNewer)
    );
}

#[test]
fn a_matching_child_leaves_the_folder_alone() {
    assert_eq!(
        CompareVerdict::Same,
        CompareRules::roll_up(CompareVerdict::Same, CompareVerdict::Same)
    );
    assert_eq!(
        CompareVerdict::Differs,
        CompareRules::roll_up(CompareVerdict::Differs, CompareVerdict::Same)
    );
}

/// The folder is absent on one side, so everything under it is too; nothing a child says can
/// downgrade that to a mere difference.
#[test]
fn an_absent_folder_stays_absent() {
    assert_eq!(
        CompareVerdict::LeftOnly,
        CompareRules::roll_up(CompareVerdict::LeftOnly, CompareVerdict::LeftOnly)
    );
}

/// Unknown outranks everything, because the only verdict that must never be arrived at by not
/// knowing is `Same`.
#[test]
fn an_unknown_child_outranks_everything() {
    for folder in CompareVerdict::ALL {
        assert_eq!(
            CompareVerdict::Unknown,
            CompareRules::roll_up(folder, CompareVerdict::Unknown),
            "{folder:?}"
        );
    }
}

/// The fold runs over a map in whatever order it iterates, so the order two children arrive in
/// must not change the answer.
#[test]
fn the_fold_does_not_depend_on_the_order_children_arrive_in() {
    for a in CompareVerdict::ALL {
        for b in CompareVerdict::ALL {
            assert_eq!(
                CompareRules::roll_up(CompareRules::roll_up(CompareVerdict::Same, a), b),
                CompareRules::roll_up(CompareRules::roll_up(CompareVerdict::Same, b), a),
                "{a:?} {b:?}"
            );
        }
    }
}
