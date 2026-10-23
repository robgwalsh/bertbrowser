// Port of tests/BertBrowser.Core.Tests/ChangeLogRulesTests.cs.
use bb_core::FileTime;
use bb_core::changes::{
    ChangeKind, ChangeLogPolicy, ChangeLogRules, ChangeRange, ChangeTimelineAvailability,
};
use bb_core::mft::usn_reason::{CLOSE, DATA_EXTEND, FILE_CREATE, FILE_DELETE, RENAME_NEW_NAME};

#[test]
fn classify_follows_the_indexers_precedence() {
    for (reason, had_old_name, expected) in [
        (CLOSE | FILE_CREATE, false, ChangeKind::Created),
        (
            CLOSE | FILE_CREATE | DATA_EXTEND,
            false,
            ChangeKind::Created,
        ),
        (CLOSE | DATA_EXTEND, false, ChangeKind::Modified),
        (CLOSE, false, ChangeKind::Modified),
        (CLOSE | FILE_DELETE, false, ChangeKind::Deleted),
        // A temp file created and deleted inside one close: it is gone, and that is the fact.
        (
            CLOSE | FILE_CREATE | FILE_DELETE,
            false,
            ChangeKind::Deleted,
        ),
        (CLOSE | RENAME_NEW_NAME, true, ChangeKind::Renamed),
        // An unpaired new name is a move-in from somewhere the map could not resolve.
        (CLOSE | RENAME_NEW_NAME, false, ChangeKind::Created),
        (
            CLOSE | RENAME_NEW_NAME | FILE_DELETE,
            true,
            ChangeKind::Deleted,
        ),
    ] {
        assert_eq!(
            expected,
            ChangeLogRules::classify(reason, had_old_name),
            "reason {reason:#x}"
        );
    }
}

#[test]
fn is_excluded_covers_the_data_directory_and_nothing_beside() {
    for (path_key, expected) in [
        (r"C:\USERS\ROB\.BERTBROWSER\BERTBROWSER.DB-WAL", true),
        (r"C:\USERS\ROB\.BERTBROWSER\THEMES\MINE.JSON", true),
        (r"C:\USERS\ROB\.BERTBROWSER", true),
        (r"C:\USERS\ROB\.BERTBROWSERX\FILE.TXT", false),
        (r"C:\USERS\ROB\NOTES.TXT", false),
    ] {
        assert_eq!(
            expected,
            ChangeLogRules::is_excluded(path_key, r"C:\USERS\ROB\.BERTBROWSER"),
            "{path_key}"
        );
    }
}

fn now() -> FileTime {
    FileTime::from_ymd_hms(2026, 9, 2, 12, 0, 0)
}

#[test]
fn since_utc_presets_subtract_from_now() {
    let now = now();
    assert_eq!(
        Some(now.add_minutes(-15)),
        ChangeLogRules::since_utc(ChangeRange::Last15Minutes, now, None)
    );
    assert_eq!(
        Some(now.add_hours(-1)),
        ChangeLogRules::since_utc(ChangeRange::LastHour, now, None)
    );
    assert_eq!(
        Some(now.add_hours(-6)),
        ChangeLogRules::since_utc(ChangeRange::Last6Hours, now, None)
    );
    assert_eq!(
        Some(now.add_hours(-24)),
        ChangeLogRules::since_utc(ChangeRange::Last24Hours, now, None)
    );
}

#[test]
fn since_utc_since_mark_needs_a_mark() {
    let now = now();
    let mark = now.add_minutes(-3);
    assert_eq!(
        Some(mark),
        ChangeLogRules::since_utc(ChangeRange::SinceMark, now, Some(mark))
    );
    assert_eq!(
        None,
        ChangeLogRules::since_utc(ChangeRange::SinceMark, now, None)
    );
}

#[test]
fn since_utc_never_reaches_past_the_retention() {
    // With retention at an hour, "last 24 hours" must not surface rows the writer has simply not
    // got round to pruning yet: the user was promised an hour.
    let now = now();
    let policy = ChangeLogPolicy::from_hours(1).unwrap();

    assert_eq!(
        Some(now.add_hours(-1)),
        ChangeLogRules::since_utc_within(ChangeRange::Last24Hours, now, None, policy)
    );
    assert_eq!(
        Some(now.add_minutes(-15)),
        ChangeLogRules::since_utc_within(ChangeRange::Last15Minutes, now, None, policy)
    );
    assert_eq!(
        Some(now.add_hours(-1)),
        ChangeLogRules::since_utc_within(
            ChangeRange::SinceMark,
            now,
            Some(now.add_hours(-5)),
            policy
        )
    );
    assert_eq!(
        None,
        ChangeLogRules::since_utc_within(ChangeRange::SinceMark, now, None, policy)
    );
}

#[test]
fn availability_says_which_thing_is_missing() {
    use ChangeTimelineAvailability::*;
    for (
        recording_on,
        scoped,
        any_indexed,
        scope_indexed,
        is_building,
        indexer_running,
        expected,
    ) in [
        // Recording off wins over everything: there is nothing to show however healthy the index is.
        (false, false, true, true, false, true, RecordingOff),
        (true, false, true, true, false, true, Ready),
        (true, true, true, true, false, true, Ready),
        // The helper declined or died: nothing is being recorded.
        (true, false, false, false, false, false, IndexerUnavailable),
        // Still building: recording starts when the build completes.
        (true, false, false, false, true, true, Building),
        (true, true, true, false, true, true, Building),
        // A scoped folder on a drive the helper does not cover (exFAT, a share).
        (true, true, true, false, false, true, ScopeNotIndexed),
        // Nothing indexed and nothing building: the helper is up but found no NTFS volume.
        (true, false, false, false, false, true, IndexerUnavailable),
    ] {
        assert_eq!(
            expected,
            ChangeLogRules::availability(
                recording_on,
                scoped,
                any_indexed,
                scope_indexed,
                is_building,
                indexer_running
            ),
            "{recording_on} {scoped} {any_indexed} {scope_indexed} {is_building} {indexer_running}"
        );
    }
}

#[test]
fn scope_for_leaves_containers_for_their_folder() {
    let is_zip = |p: &str| p.to_ascii_lowercase().ends_with(".zip");
    for (path, expected) in [
        (None, None),
        (Some(""), None),
        (Some(r"C:\Work\Proj"), Some(r"C:\Work\Proj")),
        // Inside a container: the entry has no real path, so the question becomes "what changed
        // around the archive" rather than an empty answer that reads as "nothing".
        (Some(r"C:\Work\a.zip"), Some(r"C:\Work")),
        (Some(r"C:\Work\a.zip\src\lib"), Some(r"C:\Work")),
    ] {
        assert_eq!(
            expected.map(str::to_string),
            ChangeLogRules::scope_for(path, is_zip),
            "{path:?}"
        );
    }
}

#[test]
fn empty_message_names_the_range_and_the_scope() {
    assert_eq!(
        "Nothing changed here in the last hour.",
        ChangeLogRules::empty_message(ChangeRange::LastHour, true)
    );
    assert_eq!(
        "Nothing changed on this PC in the last 15 minutes.",
        ChangeLogRules::empty_message(ChangeRange::Last15Minutes, false)
    );
    assert_eq!(
        "Nothing changed on this PC since the mark.",
        ChangeLogRules::empty_message(ChangeRange::SinceMark, false)
    );
}

#[test]
fn policy_round_trips_through_hours() {
    for (hours, enabled, back_out) in [
        (0, false, 0),
        (1, true, 1),
        (24, true, 24),
        (168, true, 168),
    ] {
        let policy = ChangeLogPolicy::from_hours(hours).unwrap();
        assert_eq!(enabled, policy.enabled);
        assert_eq!(back_out, policy.to_hours());
        assert!(ChangeLogPolicy::is_acceptable_hours(hours));
    }
}

#[test]
fn policy_refuses_hours_off_the_menu() {
    for hours in [-1, 2, 25, 1000] {
        assert!(!ChangeLogPolicy::is_acceptable_hours(hours));
        assert!(ChangeLogPolicy::from_hours(hours).is_none());
    }
}

#[test]
fn policy_default_is_off() {
    assert!(!ChangeLogPolicy::default().enabled);
    let off = ChangeLogPolicy::OFF;
    assert!(!off.enabled);
    assert_eq!(24, ChangeLogPolicy::DEFAULT_RETENTION_HOURS);
}
