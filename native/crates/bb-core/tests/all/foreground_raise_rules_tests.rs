// Port of tests/BertBrowser.Core.Tests/ForegroundRaiseRulesTests.cs.
use bb_core::rules::{ForegroundRaiseRules, RaiseAction, UserNotificationState};

const OURS: bool = true;
const THEIRS: bool = false;
const FILLS: bool = true;
const WINDOWED: bool = false;

#[test]
fn decide_withholds_the_raise_only_for_something_full_screen() {
    use UserNotificationState::*;
    for (state, ours, fills, expected) in [
        // A hand-off arriving while something is full screen must not take the screen.
        (Busy, THEIRS, WINDOWED, RaiseAction::Flash),
        (RunningD3DFullScreen, THEIRS, WINDOWED, RaiseAction::Flash),
        (PresentationMode, THEIRS, WINDOWED, RaiseAction::Flash),
        // A Store app only counts when it is actually full screen.
        (App, THEIRS, FILLS, RaiseAction::Flash),
        (App, THEIRS, WINDOWED, RaiseAction::Raise),
        // The ordinary case, and the two states that are about notifications rather than the screen.
        (AcceptsNotifications, THEIRS, WINDOWED, RaiseAction::Raise),
        (QuietTime, THEIRS, WINDOWED, RaiseAction::Raise),
        (NotPresent, THEIRS, WINDOWED, RaiseAction::Raise),
        // A shell that would not answer must not cost the user the folder they asked for.
        (Unknown, THEIRS, WINDOWED, RaiseAction::Raise),
        (Unknown, THEIRS, FILLS, RaiseAction::Raise),
        // Geometry alone is not full screen.
        (AcceptsNotifications, THEIRS, FILLS, RaiseAction::Raise),
    ] {
        assert_eq!(
            expected,
            ForegroundRaiseRules::decide(state, ours, fills),
            "{state:?} {ours} {fills}"
        );
    }
}

#[test]
fn decide_raises_when_we_are_already_the_foreground() {
    use UserNotificationState::*;
    for state in [Busy, RunningD3DFullScreen, PresentationMode, App] {
        assert_eq!(
            RaiseAction::Raise,
            ForegroundRaiseRules::decide(state, OURS, FILLS),
            "{state:?}"
        );
    }
}
