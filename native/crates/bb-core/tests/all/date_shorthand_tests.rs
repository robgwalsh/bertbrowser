// Port of tests/BertBrowser.Core.Tests/DateShorthandTests.cs. The clock is injected throughout:
// "today" is otherwise a test that fails once a day at midnight.
use bb_core::FileTime;
use bb_core::search::DateShorthand;

/// A Wednesday, so the week boundaries are not accidentally right. Local wall-clock ticks.
fn now() -> FileTime {
    FileTime::from_ymd_hms(2026, 8, 26, 15, 30, 0)
}

fn resolve(text: &str) -> (FileTime, FileTime) {
    DateShorthand::try_resolve(text, now()).unwrap_or_else(|| panic!("{text}"))
}

/// The local wall-clock instant a bound is expected to fall on, in UTC.
fn local(y: i64, m: u32, d: u32) -> FileTime {
    FileTime::from_ymd(y, m, d).local_to_utc()
}

#[test]
fn today() {
    let (lo, hi) = resolve("today");
    assert_eq!(local(2026, 8, 26), lo);
    assert_eq!(local(2026, 8, 27), hi);
}

#[test]
fn yesterday() {
    let (lo, hi) = resolve("yesterday");
    assert_eq!(local(2026, 8, 25), lo);
    assert_eq!(local(2026, 8, 26), hi);
}

#[test]
fn this_week_starts_monday() {
    // 26 Aug 2026 is a Wednesday; the ISO week began on Monday the 24th.
    let (lo, hi) = resolve("thisweek");
    assert_eq!(local(2026, 8, 24), lo);
    assert_eq!(local(2026, 8, 31), hi);
}

#[test]
fn last_month() {
    let (lo, hi) = resolve("lastmonth");
    assert_eq!(local(2026, 7, 1), lo);
    assert_eq!(local(2026, 8, 1), hi);
}

#[test]
fn last_n_days_ends_at_the_end_of_today() {
    // Something modified an hour ago is inside "last7days".
    let (lo, hi) = resolve("last7days");
    assert_eq!(local(2026, 8, 27), hi);
    assert_eq!(local(2026, 8, 20), lo);
}

#[test]
fn calendar_spans() {
    for (text, ly, lm, ld, hy, hm, hd) in [
        ("2026", 2026, 1, 1, 2027, 1, 1),
        ("2026-08", 2026, 8, 1, 2026, 9, 1),
        ("2026-08-26", 2026, 8, 26, 2026, 8, 27),
    ] {
        let (lo, hi) = resolve(text);
        assert_eq!(local(ly, lm, ld), lo, "{text}");
        assert_eq!(local(hy, hm, hd), hi, "{text}");
    }
}

#[test]
fn refuses_what_is_not_a_date() {
    for text in ["", "banana", "2026-13-01", "last", "lastfortnights", "0000"] {
        assert!(
            DateShorthand::try_resolve(text, now()).is_none(),
            "{text:?}"
        );
    }
}

/// Shorthands mean the user's day, not UTC's.
#[test]
fn resolves_against_local_midnight_not_utc_midnight() {
    let (lo, _) = resolve("today");
    let civil = lo.to_local().civil();
    assert_eq!((2026, 8, 26), (civil.year, civil.month, civil.day));
    assert_eq!((0, 0, 0), (civil.hour, civil.minute, civil.second));
}
