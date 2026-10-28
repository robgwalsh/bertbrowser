// Port of tests/BertBrowser.Core.Tests/DropInContractTests.cs. The truth table for an incoming
// drop. The dangerous half is what gets reported back: an external source acts on it, so
// reporting Move for something the user meant as a copy deletes another application's files on
// our say-so.
use bb_core::transfer::{DropEffect, DropInContract, DropOrigin, TransferVerb};

const BOTH: DropEffect = DropEffect(DropEffect::COPY.0 | DropEffect::MOVE.0);

// --- In-app: Ctrl copies, everything else moves, and nothing is ever reported ---

#[test]
fn in_app_ctrl_copies_everything_else_moves() {
    for (control, shift, expected) in [
        (false, false, TransferVerb::Move),
        (false, true, TransferVerb::Move), // Shift is the list's range modifier, not a verb
        (true, false, TransferVerb::Copy),
        (true, true, TransferVerb::Copy),
    ] {
        let decision = DropInContract::decide(DropOrigin::InApp, control, shift, BOTH);

        assert_eq!(expected, decision.verb, "control={control} shift={shift}");
    }
}

/// The one that protects our own files. Our drop already happened through the executor, so
/// reporting anything but None would have `DragOutContract` read our own drop as a foreign move
/// and delete the items we just placed. Make this report the verb and it goes red.
#[test]
fn in_app_reports_nothing_whatever_it_did() {
    for (control, shift) in [(false, false), (true, false), (false, true)] {
        let decision = DropInContract::decide(DropOrigin::InApp, control, shift, BOTH);

        assert_eq!(
            DropEffect::NONE,
            decision.report,
            "control={control} shift={shift}"
        );
    }
}

// --- External: copying is the default, and a move must be asked for ---

/// The source is a window we know nothing about, and a move makes *it* delete its files. So "put
/// this here" is read as a copy unless the user says otherwise. Flip this default and dragging a
/// file out of a mail client would remove the attachment.
#[test]
fn external_defaults_to_copy_not_move() {
    let decision = DropInContract::decide(DropOrigin::External, false, false, BOTH);

    assert_eq!(TransferVerb::Copy, decision.verb);
    assert_eq!(DropEffect::COPY, decision.report);
}

#[test]
fn external_shift_asks_for_a_move() {
    let decision = DropInContract::decide(DropOrigin::External, false, true, BOTH);

    assert_eq!(TransferVerb::Move, decision.verb);
    assert_eq!(DropEffect::MOVE, decision.report);
}

/// Ctrl means copy on both sides, so holding it can never become a move by accident, not even
/// with Shift also down.
#[test]
fn external_ctrl_forces_copy_even_with_shift() {
    let decision = DropInContract::decide(DropOrigin::External, true, true, BOTH);

    assert_eq!(TransferVerb::Copy, decision.verb);
}

/// A source that will not permit a move must never be told one: it may act on the report.
#[test]
fn external_never_reports_a_verb_the_source_did_not_offer() {
    let decision = DropInContract::decide(DropOrigin::External, false, true, DropEffect::COPY);

    assert_eq!(TransferVerb::Copy, decision.verb);
    assert_eq!(DropEffect::COPY, decision.report);
}

/// A move-only source still works, rather than the drop silently doing nothing.
#[test]
fn external_falls_back_to_the_verb_the_source_does_offer() {
    let decision = DropInContract::decide(DropOrigin::External, false, false, DropEffect::MOVE);

    assert_eq!(TransferVerb::Move, decision.verb);
    assert_eq!(DropEffect::MOVE, decision.report);
}

/// The auto-scroll bit rides along on real drags and is not a verb; a raw comparison against Copy
/// would miss a value carrying it.
#[test]
fn the_scroll_bit_is_not_mistaken_for_a_verb() {
    let decision = DropInContract::decide(
        DropOrigin::External,
        false,
        false,
        BOTH | DropEffect::SCROLL,
    );

    assert_eq!(TransferVerb::Copy, decision.verb);
}

// --- What is worth accepting at all ---

#[test]
fn a_link_only_source_is_refused_this_app_makes_no_shortcuts() {
    assert!(!DropInContract::can_accept(DropEffect::LINK));
    assert!(!DropInContract::can_accept(DropEffect::NONE));
    assert!(DropInContract::can_accept(DropEffect::COPY));
    assert!(DropInContract::can_accept(DropEffect::MOVE));
}
