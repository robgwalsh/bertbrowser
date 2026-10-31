// Port of tests/BertBrowser.Core.Tests/RateFormatterTests.cs.
use std::time::Duration;

use bb_core::format::{remaining, speed};

#[test]
fn speed_reads_as_a_throughput() {
    for (rate, expected) in [
        (None, ""),
        (Some(0.0), ""),
        (Some(-1.0), ""),
        (Some(512.0), "512 B/s"),
        (Some(1024.0), "1 KB/s"),
        (Some(117_440_512.0), "112 MB/s"),
    ] {
        assert_eq!(expected, speed(rate), "{rate:?}");
    }
}

#[test]
fn speed_is_blank_rather_than_zero_when_nothing_is_known_yet() {
    // A "0 B/s" on screen reads as a stalled transfer; a blank reads as "not yet".
    assert_eq!("", speed(None));
}

#[test]
fn remaining_is_blank_when_there_is_no_estimate() {
    assert_eq!("", remaining(None));
}

#[test]
fn remaining_reads_as_a_time_left() {
    for (seconds, expected) in [
        (0, "less than a minute left"),
        (59, "less than a minute left"),
        (90, "about 2 minutes left"),
        (60, "about 1 minute left"),
        (360, "about 6 minutes left"),
        (3600, "about 1 hour left"),
        (4800, "about 1 hour 20 minutes left"),
        (7200, "about 2 hours left"),
    ] {
        assert_eq!(
            expected,
            remaining(Some(Duration::from_secs(seconds))),
            "{seconds}"
        );
    }
}

#[test]
fn remaining_never_says_sixty_minutes() {
    // Rounding after splitting hours from minutes turns 1:59:50 into "1 hour 60 minutes".
    let text = remaining(Some(Duration::from_secs(3600 + 59 * 60 + 50)));
    assert!(!text.contains("60 minutes"));
    assert_eq!("about 2 hours left", text);
}
