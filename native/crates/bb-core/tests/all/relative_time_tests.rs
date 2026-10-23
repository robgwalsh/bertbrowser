// Port of tests/BertBrowser.Core.Tests/RelativeTimeTests.cs.
use bb_core::FileTime;
use bb_core::format::{day, relative_time};
use bb_core::time::format_short_local;

fn now() -> FileTime {
    FileTime::from_ymd_hms(2026, 9, 2, 12, 0, 0)
}

#[test]
fn format_reads_as_an_age() {
    for (seconds_ago, expected) in [
        (0, "just now"),
        (59, "just now"),
        (60, "1 min ago"),
        (59 * 60 + 59, "59 min ago"),
        (3600, "1 h ago"),
        (23 * 3600 + 3599, "23 h ago"),
    ] {
        assert_eq!(
            expected,
            relative_time(now().add_secs(-seconds_ago), now()),
            "{seconds_ago}"
        );
    }
}

#[test]
fn format_clamps_the_future_to_just_now() {
    // A record stamped by a clock a few seconds ahead of ours must not read as "-3 s ago".
    assert_eq!("just now", relative_time(now().add_secs(5), now()));
}

#[test]
fn format_falls_back_to_a_date_after_a_day() {
    let utc = now().add_days(-2);
    assert_eq!(format_short_local(utc), relative_time(utc, now()));
}

fn local_now() -> FileTime {
    FileTime::from_ymd_hms(2026, 9, 25, 16, 0, 0)
}

#[test]
fn day_says_never_for_no_timestamp() {
    assert_eq!("Never", day(None, local_now(), "Never"));
}

#[test]
fn day_names_today_and_yesterday() {
    assert_eq!(
        "Today 09:05",
        day(
            Some(FileTime::from_ymd_hms(2026, 9, 25, 9, 5, 0)),
            local_now(),
            "Never"
        )
    );
    assert_eq!(
        "Yesterday 23:59",
        day(
            Some(FileTime::from_ymd_hms(2026, 9, 24, 23, 59, 0)),
            local_now(),
            "Never"
        )
    );
}

#[test]
fn day_drops_the_year_only_within_this_year() {
    assert_eq!(
        "3 Feb 08:15",
        day(
            Some(FileTime::from_ymd_hms(2026, 2, 3, 8, 15, 0)),
            local_now(),
            "Never"
        )
    );
    assert_eq!(
        "3 Feb 2025",
        day(
            Some(FileTime::from_ymd_hms(2025, 2, 3, 8, 15, 0)),
            local_now(),
            "Never"
        )
    );
}
