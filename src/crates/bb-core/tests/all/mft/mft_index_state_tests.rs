// Port of tests/BertBrowser.Core.Tests/MftIndexStateTests.cs: the state both the in-process
// service and the out-of-process client answer from. These are the semantics search routing and the
// status bar depend on, and they must not differ by which implementation happens to be wired up.
use bb_core::mft::MftIndexState;

#[test]
fn nothing_is_indexed_or_building_initially() {
    let state = MftIndexState::new();

    assert!(!state.any_indexed());
    assert!(!state.is_building());
    assert!(!state.is_indexed(r"C:\WINDOWS"));
    assert_eq!("", state.format_status());
}

#[test]
fn completed_root_matches_itself_and_its_descendants() {
    let state = MftIndexState::new();
    state.mark_complete(r"C:\");

    assert!(state.any_indexed());
    assert!(state.is_indexed(r"C:\"));
    assert!(state.is_indexed(r"C:\WINDOWS"));
    assert!(state.is_indexed(r"C:\WINDOWS\SYSTEM32\DRIVERS"));
}

#[test]
fn other_volumes_are_not_covered_by_a_completed_root() {
    let state = MftIndexState::new();
    state.mark_complete(r"C:\");

    assert!(!state.is_indexed(r"D:\"));
    assert!(!state.is_indexed(r"D:\DATA"));
    assert!(!state.is_indexed(r"\\SERVER\SHARE"));
}

/// The reason this goes through `PathKey::is_under` rather than a prefix test. Mutate it to a
/// prefix comparison and this is the theory that goes red.
#[test]
fn a_sibling_sharing_the_prefix_is_not_covered() {
    for key in [r"C:\FOOBAR", r"C:\FOO2", r"C:\FOO.BAK"] {
        let state = MftIndexState::new();
        state.mark_complete(r"C:\FOO");

        assert!(state.is_indexed(r"C:\FOO"), "{key}");
        assert!(state.is_indexed(r"C:\FOO\INNER"), "{key}");
        assert!(!state.is_indexed(key), "{key}");
    }
}

#[test]
fn building_is_tracked_per_drive_and_cleared() {
    let state = MftIndexState::new();

    state.mark_building("C");
    assert!(state.is_building());

    state.mark_building("D");
    assert!(state.is_building());

    state.clear_building("C");
    assert!(state.is_building());

    state.clear_building("D");
    assert!(!state.is_building());
}

#[test]
fn clearing_a_drive_that_is_not_building_is_harmless() {
    let state = MftIndexState::new();

    state.clear_building("C");

    assert!(!state.is_building());
}

#[test]
fn status_names_one_building_drive() {
    let state = MftIndexState::new();
    state.mark_building("C");

    assert_eq!("Indexing C:…", state.format_status());
}

/// Ordered, so the line does not shuffle as threads finish in whatever order.
#[test]
fn status_names_several_building_drives_in_order() {
    let state = MftIndexState::new();
    state.mark_building("D");
    state.mark_building("C");
    state.mark_building("E");

    assert_eq!("Indexing C:, D:, E:…", state.format_status());
}

#[test]
fn status_says_how_far_along_a_drive_is() {
    let state = MftIndexState::new();
    state.mark_building("C");
    state.set_progress("C", Some(42));

    assert_eq!(Some(42), state.progress_of("C"));
    assert_eq!("Indexing C: 42%…", state.format_status());
}

/// Each drive speaks for itself, so one that has not reported yet does not borrow another's number.
#[test]
fn status_gives_each_drive_its_own_number_or_none() {
    let state = MftIndexState::new();
    state.mark_building("C");
    state.mark_building("D");
    state.mark_building("E");
    state.set_progress("C", Some(42));
    state.set_progress("E", Some(7));

    assert_eq!("Indexing C: 42%, D:, E: 7%…", state.format_status());
}

/// What a build falling back to the names-only path does: it can no longer say.
#[test]
fn progress_can_be_taken_back() {
    let state = MftIndexState::new();
    state.mark_building("C");
    state.set_progress("C", Some(12));

    state.set_progress("C", None);

    assert_eq!(None, state.progress_of("C"));
    assert_eq!("Indexing C:…", state.format_status());
}

/// A message arriving after `Idle` must not put a finished drive back on the status line for ever.
#[test]
fn progress_for_a_drive_not_building_does_not_start_it_building() {
    let state = MftIndexState::new();

    state.set_progress("C", Some(42));

    assert!(!state.is_building());
    assert_eq!(None, state.progress_of("C"));
    assert_eq!("", state.format_status());
}

/// Building replays on every session; it must not wipe a number that is still true.
#[test]
fn being_told_a_drive_is_building_again_keeps_its_progress() {
    let state = MftIndexState::new();
    state.mark_building("C");
    state.set_progress("C", Some(42));

    state.mark_building("C");

    assert_eq!(Some(42), state.progress_of("C"));
}

#[test]
fn finishing_a_drive_forgets_its_progress() {
    let state = MftIndexState::new();
    state.mark_building("C");
    state.set_progress("C", Some(99));

    state.clear_building("C");
    state.mark_building("C");

    assert_eq!(None, state.progress_of("C"));
}

#[test]
fn status_is_empty_once_every_drive_has_finished() {
    let state = MftIndexState::new();
    state.mark_building("C");
    state.mark_complete(r"C:\");
    state.clear_building("C");

    assert_eq!("", state.format_status());
    assert!(state.any_indexed());
}

#[test]
fn completed_roots_round_trip_for_a_client_resending_its_state() {
    let state = MftIndexState::new();
    state.mark_complete(r"C:\");
    state.mark_complete(r"D:\");

    let mut roots = state.completed_roots();
    roots.sort();
    assert_eq!(vec![r"C:\".to_string(), r"D:\".to_string()], roots);
}

/// What a client does when its indexer dies: forget everything, claim nothing.
#[test]
fn clear_forgets_completion_and_building() {
    let state = MftIndexState::new();
    state.mark_complete(r"C:\");
    state.mark_building("D");

    state.clear();

    assert!(!state.any_indexed());
    assert!(!state.is_building());
    assert!(!state.is_indexed(r"C:\WINDOWS"));
    assert_eq!("", state.format_status());
}

#[test]
fn marking_the_same_drive_or_root_twice_is_idempotent() {
    let state = MftIndexState::new();

    state.mark_building("C");
    state.mark_building("C");
    state.mark_complete(r"C:\");
    state.mark_complete(r"C:\");

    assert_eq!(1, state.completed_roots().len());
    state.clear_building("C");
    assert!(!state.is_building());
}
