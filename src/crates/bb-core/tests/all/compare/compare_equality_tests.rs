// Port of tests/BertBrowser.Core.Tests/CompareEqualityTests.cs
//
// When two files count as the same file. Every assertion here guards the same asymmetry: calling
// two different files "the same" is what authorises a sync to delete the other side, while calling
// two identical files different only offers a harmless copy. So every doubt resolves away from
// `Same`.
use bb_core::FileTime;
use bb_core::compare::{CompareEntry, CompareEquality, CompareTolerance, CompareVerdict};
use bb_core::time::TICKS_PER_SECOND;
use bb_paths::file_name;

fn noon() -> FileTime {
    FileTime::from_ymd_hms(2026, 3, 14, 12, 0, 0)
}

fn add_seconds(t: FileTime, seconds: f64) -> FileTime {
    t.add_ticks((seconds * TICKS_PER_SECOND as f64).round() as i64)
}

fn file(size: i64, modified: FileTime) -> CompareEntry {
    file_keyed(size, modified, "A.TXT")
}

fn file_keyed(size: i64, modified: FileTime, key: &str) -> CompareEntry {
    CompareEntry::new(key, file_name(key), false, size, modified)
}

fn folder() -> CompareEntry {
    CompareEntry::new("SUB", "SUB", true, 0, noon())
}

// --- Timestamp tolerance ---

#[test]
fn within_fat_granularity_is_the_same_instant() {
    for seconds in [0.0, 1.0, 1.999] {
        let tolerance = CompareTolerance::STRICT;

        assert_eq!(
            0,
            CompareEquality::compare_times(add_seconds(noon(), seconds), noon(), tolerance),
            "{seconds}"
        );
        assert_eq!(
            0,
            CompareEquality::compare_times(noon(), add_seconds(noon(), seconds), tolerance),
            "{seconds}"
        );
    }
}

#[test]
fn past_fat_granularity_is_measurably_newer() {
    let tolerance = CompareTolerance::STRICT;

    assert_eq!(
        1,
        CompareEquality::compare_times(add_seconds(noon(), 2.001), noon(), tolerance)
    );
    assert_eq!(
        -1,
        CompareEquality::compare_times(noon(), add_seconds(noon(), 2.001), tolerance)
    );
}

#[test]
fn a_whole_hour_shift_is_forgiven_only_when_loose() {
    for seconds in [3600.0, 7200.0, -3600.0] {
        let shifted = add_seconds(noon(), seconds);

        assert_eq!(
            0,
            CompareEquality::compare_times(shifted, noon(), CompareTolerance::LOOSE),
            "{seconds}"
        );
        assert_ne!(
            0,
            CompareEquality::compare_times(shifted, noon(), CompareTolerance::STRICT),
            "{seconds}"
        );
    }
}

/// The test that stops the daylight-saving rule quietly becoming an hour of slack. A file edited
/// fifty-five minutes ago is genuinely newer, and no filesystem rounds a timestamp by that much.
/// Widen the forgiven shifts into a band and this goes red.
#[test]
fn nearly_a_whole_hour_is_not_forgiven_even_when_loose() {
    // 55 minutes; five seconds short of the hour; ten seconds past it; half an hour.
    for seconds in [3300.0, 3595.0, 3610.0, 1800.0] {
        assert_eq!(
            1,
            CompareEquality::compare_times(
                add_seconds(noon(), seconds),
                noon(),
                CompareTolerance::LOOSE
            ),
            "{seconds}"
        );
    }
}

#[test]
fn a_fat_volume_on_either_side_loosens_the_tolerance() {
    for format in ["FAT32", "exFAT", "fat"] {
        assert!(
            CompareTolerance::for_formats(Some(format), Some("NTFS")).allow_whole_hour_shift,
            "{format}"
        );
        assert!(
            CompareTolerance::for_formats(Some("NTFS"), Some(format)).allow_whole_hour_shift,
            "{format}"
        );
    }
}

/// A volume that would not name its format (a UNC path, a mapped drive) must fail closed. An
/// unforgiven hour shows as "newer" and offers a copy; a wrongly forgiven one offers a delete.
#[test]
fn an_unrecognised_volume_stays_strict() {
    for (left, right) in [
        (None, None),
        (Some("NTFS"), Some("NTFS")),
        (Some("ReFS"), None),
    ] {
        assert_eq!(
            CompareTolerance::STRICT,
            CompareTolerance::for_formats(left, right),
            "{left:?} {right:?}"
        );
    }
}

// --- Verdicts ---

#[test]
fn one_side_missing_is_only_on_the_other() {
    let file = file(10, noon());

    assert_eq!(
        CompareVerdict::LeftOnly,
        CompareEquality::verdict(Some(&file), None, CompareTolerance::STRICT)
    );
    assert_eq!(
        CompareVerdict::RightOnly,
        CompareEquality::verdict(None, Some(&file), CompareTolerance::STRICT)
    );
}

#[test]
fn same_size_same_instant_is_same() {
    assert_eq!(
        CompareVerdict::Same,
        CompareEquality::verdict(
            Some(&file(10, noon())),
            Some(&file(10, add_seconds(noon(), 1.0))),
            CompareTolerance::STRICT
        )
    );
}

/// The size is never allowed to make two files equal on its own. A file rewritten to the same
/// length is the everyday case, and it is why the timestamp is compared at all.
#[test]
fn same_size_different_instant_is_newer_not_same() {
    assert_eq!(
        CompareVerdict::LeftNewer,
        CompareEquality::verdict(
            Some(&file(10, noon().add_minutes(5))),
            Some(&file(10, noon())),
            CompareTolerance::STRICT
        )
    );
}

#[test]
fn different_sizes_are_never_same_whatever_the_timestamps() {
    assert_eq!(
        CompareVerdict::Differs,
        CompareEquality::verdict(
            Some(&file(10, noon())),
            Some(&file(20, noon())),
            CompareTolerance::STRICT
        )
    );

    assert_eq!(
        CompareVerdict::RightNewer,
        CompareEquality::verdict(
            Some(&file(10, noon())),
            Some(&file(20, noon().add_hours(3))),
            CompareTolerance::STRICT
        )
    );
}

/// The index's name-only build path writes the unknown timestamp for every row on the volume.
/// Trusting the sizes there would call a whole drive "the same", and then offer to empty the
/// other one.
#[test]
fn a_missing_timestamp_is_unknown_not_same() {
    assert_eq!(
        CompareVerdict::Unknown,
        CompareEquality::verdict(
            Some(&file(10, FileTime::UNKNOWN)),
            Some(&file(10, noon())),
            CompareTolerance::STRICT
        )
    );

    assert_eq!(
        CompareVerdict::Unknown,
        CompareEquality::verdict(
            Some(&file(10, noon())),
            Some(&file(10, FileTime::UNKNOWN)),
            CompareTolerance::STRICT
        )
    );
}

#[test]
fn a_file_against_a_folder_of_the_same_name_differs() {
    assert_eq!(
        CompareVerdict::Differs,
        CompareEquality::verdict(
            Some(&file_keyed(10, noon(), "SUB")),
            Some(&folder()),
            CompareTolerance::STRICT
        )
    );
}

/// Two folders are the neutral seed the roll-up folds children into, and also the right answer
/// for two empty folders, which is the case that reaches this unfolded.
#[test]
fn two_folders_are_same_before_their_contents_are_folded_in() {
    assert_eq!(
        CompareVerdict::Same,
        CompareEquality::verdict(Some(&folder()), Some(&folder()), CompareTolerance::STRICT)
    );
}
