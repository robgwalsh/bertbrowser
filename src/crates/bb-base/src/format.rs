//! Numbers and times as words: byte sizes, throughput, time remaining, relative ages.
//! Spec: Core/Services/ByteSizeFormatter.cs, RateFormatter.cs, RelativeTime.cs.

use std::time::Duration;

use crate::FileTime;
use crate::time::{MONTH_ABBREVIATIONS, TICKS_PER_DAY, TICKS_PER_HOUR, TICKS_PER_MINUTE};

const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

/// Formats bytes the same way for files and directories, e.g. "1.4 MB". GB and larger get three
/// decimals so big folders are distinguishable; KB and MB stay at one.
pub fn byte_size(bytes: i64) -> String {
    if bytes < 0 {
        return "\u{2014}".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    if unit >= 3 {
        return format!("{value:.3} {}", UNITS[unit]);
    }
    let one_decimal = format!("{value:.1}");
    let trimmed = one_decimal.strip_suffix(".0").unwrap_or(&one_decimal);
    format!("{trimmed} {}", UNITS[unit])
}

/// e.g. "112 MB/s". Empty when there is no figure yet: a blank is honest where a zero would read as
/// a stall.
pub fn speed(bytes_per_second: Option<f64>) -> String {
    match bytes_per_second {
        Some(rate) if rate > 0.0 => format!("{}/s", byte_size(rate.round() as i64)),
        _ => String::new(),
    }
}

/// e.g. "about 6 minutes left". Deliberately vague: a remaining time is an estimate off a smoothed
/// rate. Rounded to minutes once, then split, so 1 h 59 m 50 s never comes out as "1 hour 60 minutes".
pub fn remaining(left: Option<Duration>) -> String {
    let Some(left) = left else {
        return String::new();
    };
    if left < Duration::from_secs(60) {
        return "less than a minute left".to_string();
    }
    let total_minutes = (left.as_secs_f64() / 60.0).round() as i64;
    if total_minutes < 60 {
        return format!("about {} left", plural(total_minutes, "minute"));
    }
    let hours = total_minutes / 60;
    let trailing = total_minutes % 60;
    if trailing == 0 {
        format!("about {} left", plural(hours, "hour"))
    } else {
        format!(
            "about {} {} left",
            plural(hours, "hour"),
            plural(trailing, "minute")
        )
    }
}

fn plural(count: i64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

/// "3 min ago" for a timeline that is read while it is still happening. Past a day an age stops
/// being useful and the local short date takes over. A record stamped by a clock slightly ahead of
/// ours is not from the future.
pub fn relative_time(utc: FileTime, now_utc: FileTime) -> String {
    let age = now_utc.ticks_since(utc);
    if age < TICKS_PER_MINUTE {
        return "just now".to_string();
    }
    if age < TICKS_PER_HOUR {
        return format!("{} min ago", age / TICKS_PER_MINUTE);
    }
    if age < TICKS_PER_DAY {
        return format!("{} h ago", age / TICKS_PER_HOUR);
    }
    crate::time::format_short_local(utc)
}

/// A timestamp by the day it fell on, for a list read at leisure: "Today 14:05", "Yesterday 09:30",
/// then the date. None reads as `missing` ("Never" for a saved search that has not been used). Both
/// arguments are local wall-clock ticks; the caller converts, so this stays pure.
pub fn day(local: Option<FileTime>, now_local: FileTime, missing: &str) -> String {
    let Some(when) = local else {
        return missing.to_string();
    };
    let days = now_local.day_number() - when.day_number();
    let c = when.civil();
    match days {
        0 => format!("Today {}", when.hhmm()),
        1 => format!("Yesterday {}", when.hhmm()),
        _ if c.year == now_local.civil().year => {
            format!(
                "{} {} {}",
                c.day,
                MONTH_ABBREVIATIONS[(c.month - 1) as usize],
                when.hhmm()
            )
        }
        _ => format!(
            "{} {} {}",
            c.day,
            MONTH_ABBREVIATIONS[(c.month - 1) as usize],
            c.year
        ),
    }
}

/// A count with thousands separators, as `N0` printed it: 1,200.
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}
