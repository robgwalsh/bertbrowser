// Port of tests/BertBrowser.Core.Tests/RightDropMenuRulesTests.cs. What the menu at the end of a
// right-drag offers. Entries are greyed rather than dropped, so the menu never rearranges itself
// under the cursor between one drag and the next: a menu whose items move is a menu you misclick.
use bb_core::transfer::{RightDropContext, RightDropEntry, RightDropMenuRules, RightDropVerb};

fn ordinary() -> RightDropContext {
    RightDropContext {
        destination_is_archive: false,
        sources_are_virtual: false,
        move_has_work: true,
        copy_has_work: true,
        item_count: 1,
    }
}

fn enabled(entries: &[RightDropEntry], verb: RightDropVerb) -> bool {
    entries.iter().find(|e| e.verb == verb).unwrap().enabled
}

fn label(entries: &[RightDropEntry], verb: RightDropVerb) -> String {
    entries
        .iter()
        .find(|e| e.verb == verb)
        .unwrap()
        .label
        .clone()
}

#[test]
fn an_ordinary_drop_offers_move_then_copy_then_shortcut() {
    let entries = RightDropMenuRules::build(ordinary());

    assert_eq!(
        vec![
            RightDropVerb::Move,
            RightDropVerb::Copy,
            RightDropVerb::Shortcut
        ],
        entries.iter().map(|e| e.verb).collect::<Vec<_>>()
    );
    assert!(entries.iter().all(|e| e.enabled));
}

/// Adding to a container rewrites it, and that is always additive: there is no move to offer, and
/// a shortcut is a file pointing at a path, which an entry inside an archive has not got.
#[test]
fn into_an_archive_only_copy_is_offered() {
    let entries = RightDropMenuRules::build(RightDropContext {
        destination_is_archive: true,
        ..ordinary()
    });

    assert!(!enabled(&entries, RightDropVerb::Move));
    assert!(enabled(&entries, RightDropVerb::Copy));
    assert!(!enabled(&entries, RightDropVerb::Shortcut));
}

/// The counterpart: dragging *out* of a container. The paths are virtual, so nothing a shortcut
/// could point at exists, but the copy that extracts them is real work.
#[test]
fn virtual_sources_cannot_be_shortcut_to() {
    let entries = RightDropMenuRules::build(RightDropContext {
        sources_are_virtual: true,
        ..ordinary()
    });

    assert!(!enabled(&entries, RightDropVerb::Shortcut));
    assert!(enabled(&entries, RightDropVerb::Copy));
}

/// Dropping items into the folder they already live in: there is nothing to move, but copying
/// still produces "name (2)" and a shortcut is still a real thing to make.
#[test]
fn a_move_with_nothing_to_do_is_greyed_but_the_others_stand() {
    let entries = RightDropMenuRules::build(RightDropContext {
        move_has_work: false,
        ..ordinary()
    });

    assert!(!enabled(&entries, RightDropVerb::Move));
    assert!(enabled(&entries, RightDropVerb::Copy));
    assert!(enabled(&entries, RightDropVerb::Shortcut));
}

#[test]
fn a_copy_with_nothing_to_do_is_greyed() {
    let entries = RightDropMenuRules::build(RightDropContext {
        copy_has_work: false,
        ..ordinary()
    });

    assert!(!enabled(&entries, RightDropVerb::Copy));
    assert!(enabled(&entries, RightDropVerb::Move));
}

// --- Wording ---

#[test]
fn one_item_gets_a_singular_shortcut_label() {
    let entries = RightDropMenuRules::build(RightDropContext {
        item_count: 1,
        ..ordinary()
    });

    assert_eq!("Move here", label(&entries, RightDropVerb::Move));
    assert_eq!("Copy here", label(&entries, RightDropVerb::Copy));
    assert_eq!(
        "Create shortcut here",
        label(&entries, RightDropVerb::Shortcut)
    );
}

#[test]
fn several_items_get_a_plural_shortcut_label() {
    let entries = RightDropMenuRules::build(RightDropContext {
        item_count: 3,
        ..ordinary()
    });

    assert_eq!(
        "Create shortcuts here",
        label(&entries, RightDropVerb::Shortcut)
    );
}
