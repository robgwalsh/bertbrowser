// Port of tests/BertBrowser.Core.Tests/DragOutContractTests.cs. The drag-out rule decides whether
// a finished drag means "delete the user's originals", on the word of an external application.
// That makes it the most dangerous branch in the feature and the one worth pinning down
// completely, so it is a pure function, and this is its truth table. Mutate any row of
// `DragOutContract::decide` and one of these goes red.
use bb_core::transfer::{DragOutAction, DragOutContract, DropEffect};

fn decide(
    returned: DropEffect,
    logical: Option<DropEffect>,
    performed: Option<DropEffect>,
) -> DragOutAction {
    DragOutContract::decide(false, returned, logical, performed)
}

// --- Our own drops ---

/// The trap this guard exists for: the drop pipeline handles an in-app drop and the executor has
/// already relocated the items, but `DoDragDrop` still reports Move. Acting on that would delete
/// what we just placed at the destination's source.
#[test]
fn our_own_pipeline_handling_the_drop_means_there_is_nothing_left_to_do() {
    for returned in [DropEffect::MOVE, DropEffect::COPY, DropEffect::NONE] {
        assert_eq!(
            DragOutAction::Nothing,
            DragOutContract::decide(
                true,
                returned,
                Some(DropEffect::MOVE),
                Some(DropEffect::MOVE)
            ),
            "{returned:?}"
        );
    }
}

// --- Nothing happened ---

#[test]
fn a_cancelled_or_refused_drag_does_nothing() {
    assert_eq!(DragOutAction::Nothing, decide(DropEffect::NONE, None, None));
}

#[test]
fn a_cancelled_drag_stays_nothing_even_if_a_target_left_a_performed_effect_behind() {
    assert_eq!(
        DragOutAction::Nothing,
        decide(DropEffect::NONE, None, Some(DropEffect::MOVE))
    );
}

// --- Copy and link never remove anything ---

#[test]
fn a_copy_leaves_the_originals_and_only_refreshes() {
    assert_eq!(
        DragOutAction::RefreshOnly,
        decide(DropEffect::COPY, None, None)
    );
}

#[test]
fn a_link_is_not_a_move() {
    assert_eq!(
        DragOutAction::RefreshOnly,
        decide(DropEffect::LINK, None, None)
    );
}

// --- The optimized move: the target already did it ---

/// Explorer moving within a volume relocates the files itself and reports None, which means
/// "already done": deleting here would destroy files that are now at the destination.
#[test]
fn an_optimized_move_reported_as_none_removes_nothing() {
    assert_eq!(
        DragOutAction::RefreshOnly,
        decide(DropEffect::MOVE, Some(DropEffect::NONE), None)
    );
}

#[test]
fn an_optimized_move_reported_only_in_the_legacy_format_removes_nothing() {
    assert_eq!(
        DragOutAction::RefreshOnly,
        decide(DropEffect::MOVE, None, Some(DropEffect::NONE))
    );
}

// --- The non-optimized move: ours to finish ---

#[test]
fn a_non_optimized_move_removes_the_sources() {
    assert_eq!(
        DragOutAction::RemoveSources,
        decide(DropEffect::MOVE, Some(DropEffect::MOVE), None)
    );
}

#[test]
fn a_non_optimized_move_reported_only_in_the_legacy_format_removes_the_sources() {
    assert_eq!(
        DragOutAction::RemoveSources,
        decide(DropEffect::MOVE, None, Some(DropEffect::MOVE))
    );
}

/// Plenty of targets ignore the performed-effect protocol entirely. Falling back to the return
/// value is the documented reading and is what makes a real move out of the app work at all; the
/// safety net is that the removal goes through the reversible delete, not an erase.
#[test]
fn a_move_with_no_report_at_all_falls_back_to_the_returned_effect() {
    assert_eq!(
        DragOutAction::RemoveSources,
        decide(DropEffect::MOVE, None, None)
    );
}

// --- Disagreement between the two formats ---

/// LOGICALPERFORMEDDROPEFFECT exists precisely because targets report Move in the older format
/// even for an optimized move. When they disagree, the newer one is the truthful one.
#[test]
fn when_the_two_reports_disagree_the_logical_one_wins_and_can_spare_the_sources() {
    assert_eq!(
        DragOutAction::RefreshOnly,
        decide(
            DropEffect::MOVE,
            Some(DropEffect::NONE),
            Some(DropEffect::MOVE)
        )
    );
}

#[test]
fn when_the_two_reports_disagree_the_logical_one_wins_and_can_call_for_removal() {
    assert_eq!(
        DragOutAction::RemoveSources,
        decide(
            DropEffect::MOVE,
            Some(DropEffect::MOVE),
            Some(DropEffect::NONE)
        )
    );
}

/// A target that answers "I copied" to a Move drag is contradicting itself. Nothing but an
/// explicit Move earns a deletion.
#[test]
fn a_target_that_contradicts_itself_does_not_earn_a_deletion() {
    assert_eq!(
        DragOutAction::RefreshOnly,
        decide(DropEffect::MOVE, Some(DropEffect::COPY), None)
    );
}

// --- Bit hygiene ---

/// The shell ORs in DROPEFFECT_SCROLL during auto-scroll. A raw equality check against Move would
/// miss a value carrying it, and a raw check against None would misread it.
#[test]
fn the_scroll_bit_is_not_mistaken_for_a_verb() {
    assert_eq!(
        DragOutAction::Nothing,
        decide(DropEffect::SCROLL, None, None)
    );
}

#[test]
fn the_scroll_bit_does_not_hide_a_move() {
    assert_eq!(
        DragOutAction::RemoveSources,
        decide(
            DropEffect::MOVE | DropEffect::SCROLL,
            Some(DropEffect::MOVE),
            None
        )
    );
}

#[test]
fn the_scroll_bit_does_not_hide_a_copy() {
    assert_eq!(
        DragOutAction::RefreshOnly,
        decide(DropEffect::COPY | DropEffect::SCROLL, None, None)
    );
}

/// A target reporting Move alongside the scroll bit is still reporting Move.
#[test]
fn the_scroll_bit_is_ignored_in_the_reported_effect_too() {
    assert_eq!(
        DragOutAction::RemoveSources,
        decide(
            DropEffect::MOVE,
            Some(DropEffect::MOVE | DropEffect::SCROLL),
            None
        )
    );
}

/// Copy+Move together is what the source offers, not what a target answers; if one comes back
/// that way the Move bit is present and the sources are ours to remove.
#[test]
fn a_copy_move_combination_is_treated_as_a_move() {
    assert_eq!(
        DragOutAction::RemoveSources,
        decide(DropEffect::COPY | DropEffect::MOVE, None, None)
    );
}
