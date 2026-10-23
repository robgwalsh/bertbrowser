//! Resolves the date literals a `dm:` filter accepts ("today", "thisweek", "2026-08", "2026-08-29")
//! into a half-open UTC range. **The clock is a parameter, never read here**, so the tests can pin
//! "today". **Shorthands resolve in local time, and the range comes back in UTC**: the Modified
//! column shows local time, so "today" has to mean the user's day, but the index stores UTC.
//! Spec: Core/Services/Search/DateShorthand.cs.

use crate::FileTime;
use crate::time::{civil_from_days, days_from_civil};

pub struct DateShorthand;

/// A local calendar day as days since 1970-01-01.
type Day = i64;

impl DateShorthand {
    /// Resolves `text` against `now_local` (wall-clock ticks) into the half-open local range
    /// `[lo, hi)`, converted to UTC. None when the text is not a date.
    pub fn try_resolve(text: &str, now_local: FileTime) -> Option<(FileTime, FileTime)> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let today = now_local.day_number();
        let (lo, hi) = resolve_local(text, today)?;
        Some((
            local_midnight(lo).local_to_utc(),
            local_midnight(hi).local_to_utc(),
        ))
    }
}

fn local_midnight(day: Day) -> FileTime {
    let (y, m, d) = civil_from_days(day);
    FileTime::from_ymd(y, m, d)
}

fn resolve_local(text: &str, today: Day) -> Option<(Day, Day)> {
    let upper = text.to_ascii_uppercase();
    let (ty, tm, _) = civil_from_days(today);
    match upper.as_str() {
        "TODAY" => return Some((today, today + 1)),
        "YESTERDAY" => return Some((today - 1, today)),
        "THISWEEK" => {
            let lo = start_of_week(today);
            return Some((lo, lo + 7));
        }
        "LASTWEEK" => {
            let hi = start_of_week(today);
            return Some((hi - 7, hi));
        }
        "THISMONTH" => {
            let lo = days_from_civil(ty, tm, 1);
            return Some((lo, add_months(lo, 1)));
        }
        "LASTMONTH" => {
            let hi = days_from_civil(ty, tm, 1);
            return Some((add_months(hi, -1), hi));
        }
        "THISYEAR" => {
            let lo = days_from_civil(ty, 1, 1);
            return Some((lo, days_from_civil(ty + 1, 1, 1)));
        }
        "LASTYEAR" => {
            let hi = days_from_civil(ty, 1, 1);
            return Some((days_from_civil(ty - 1, 1, 1), hi));
        }
        _ => {}
    }

    // "last7days" / "last30days" / "last12months": a window ending at the end of today, so an item
    // modified an hour ago is inside "last7days".
    if let Some(window) = parse_last_window(&upper, today) {
        return Some(window);
    }

    // "2026-08-29", "2026-08", "2026": a calendar span, most specific first.
    let parts: Vec<&str> = text.split('-').collect();
    match parts.as_slice() {
        [y, m, d] if y.len() == 4 && m.len() == 2 && d.len() == 2 => {
            let (y, m, d) = (digits(y)?, digits(m)? as u32, digits(d)? as u32);
            if !(1..=12).contains(&m)
                || d == 0
                || d > days_in_month(y, m)
                || !(1..=9999).contains(&y)
            {
                return None;
            }
            let lo = days_from_civil(y, m, d);
            Some((lo, lo + 1))
        }
        [y, m] if y.len() == 4 && m.len() == 2 => {
            let (y, m) = (digits(y)?, digits(m)? as u32);
            if !(1..=12).contains(&m) || !(1..=9999).contains(&y) {
                return None;
            }
            let lo = days_from_civil(y, m, 1);
            Some((lo, add_months(lo, 1)))
        }
        [y] if y.len() == 4 => {
            let y = digits(y)?;
            if !(1601..=9999).contains(&y) {
                return None;
            }
            Some((days_from_civil(y, 1, 1), days_from_civil(y + 1, 1, 1)))
        }
        _ => None,
    }
}

fn digits(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// "LAST<n>DAYS" / "LAST<n>WEEKS" / "LAST<n>MONTHS" / "LAST<n>YEARS".
fn parse_last_window(upper: &str, today: Day) -> Option<(Day, Day)> {
    let rest = upper.strip_prefix("LAST")?;
    let count = rest.bytes().take_while(u8::is_ascii_digit).count();
    if count == 0 {
        return None;
    }
    let n: i64 = rest[..count].parse().ok()?;
    if n <= 0 {
        return None;
    }
    // The window ends at the end of today; "last1days" is therefore today itself.
    let hi = today + 1;
    let lo = match &rest[count..] {
        "DAY" | "DAYS" => hi - n,
        "WEEK" | "WEEKS" => hi - 7 * n,
        "MONTH" | "MONTHS" => add_months(hi, -n),
        "YEAR" | "YEARS" => add_years(hi, -n),
        _ => return None,
    };
    Some((lo, hi))
}

/// Weeks start Monday (ISO 8601) rather than following the current culture, so a query means the
/// same thing on every machine. 1970-01-01 was a Thursday.
fn start_of_week(day: Day) -> Day {
    let monday_based = (day + 3).rem_euclid(7);
    day - monday_based
}

/// `DateTime.AddMonths`: the day of month is clamped to the target month's length.
fn add_months(day: Day, months: i64) -> Day {
    let (y, m, d) = civil_from_days(day);
    let total = y * 12 + (i64::from(m) - 1) + months;
    let ny = total.div_euclid(12);
    let nm = (total.rem_euclid(12) + 1) as u32;
    days_from_civil(ny, nm, d.min(days_in_month(ny, nm)))
}

fn add_years(day: Day, years: i64) -> Day {
    add_months(day, years * 12)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}
