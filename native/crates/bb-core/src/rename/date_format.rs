//! `DateTime.ToString(format, CultureInfo.InvariantCulture)` for the `{modified:…}` and
//! `{taken:…}` tokens: the .NET custom format specifiers (`yyyy`, `MM`, `dd`, `HH`, `mm`, `ss`,
//! `fff`, `tt`, quoted literals, `%`, `\`) and the single-letter standard formats, in the invariant
//! culture's spellings. A template written for the C# app keeps producing the same names.
//!
//! Nothing here consults a culture: the names of months and days are English and the separators
//! are `/` and `:`, which is what the invariant culture always gave. The value is wall-clock ticks
//! with no zone, as `DateTimeKind.Unspecified` was; `K` is therefore empty and `z` is this
//! machine's offset for that instant.

use std::fmt;

use crate::FileTime;
use crate::time::MONTH_ABBREVIATIONS;

/// `FormatException`: the format string is not one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateFormatError;

impl fmt::Display for DateFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Input string was not in a correct format.")
    }
}

impl std::error::Error for DateFormatError {}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Formats `t` (wall-clock ticks) by a .NET format string.
pub fn format_date(t: FileTime, format: &str) -> Result<String, DateFormatError> {
    let mut chars = format.chars();
    let (Some(first), None) = (chars.next(), chars.next()) else {
        return custom(t, format);
    };
    // One character is a standard format, never a custom one: "%d" is how a lone custom
    // specifier is written.
    let expanded = match first {
        'd' => "MM/dd/yyyy",
        'D' => "dddd, dd MMMM yyyy",
        'f' => "dddd, dd MMMM yyyy HH:mm",
        'F' => "dddd, dd MMMM yyyy HH:mm:ss",
        'g' => "MM/dd/yyyy HH:mm",
        'G' => "MM/dd/yyyy HH:mm:ss",
        'm' | 'M' => "MMMM dd",
        'o' | 'O' => "yyyy-MM-ddTHH:mm:ss.fffffffK",
        'r' | 'R' => "ddd, dd MMM yyyy HH':'mm':'ss 'GMT'",
        's' => "yyyy-MM-ddTHH:mm:ss",
        't' => "HH:mm",
        'T' => "HH:mm:ss",
        'u' => "yyyy-MM-dd HH:mm:ss'Z'",
        'U' => return custom(t.local_to_utc(), "dddd, dd MMMM yyyy HH:mm:ss"),
        'y' | 'Y' => "yyyy MMMM",
        _ => return Err(DateFormatError),
    };
    custom(t, expanded)
}

fn custom(t: FileTime, format: &str) -> Result<String, DateFormatError> {
    let c = t.civil();
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        let run = chars[i..].iter().take_while(|&&x| x == ch).count();
        match ch {
            'd' => {
                match run {
                    1 | 2 => push_padded(&mut out, c.day, run),
                    3 => out.push_str(&DAYS[weekday(t)][..3]),
                    _ => out.push_str(DAYS[weekday(t)]),
                }
                i += run;
            }
            'M' => {
                match run {
                    1 | 2 => push_padded(&mut out, c.month, run),
                    3 => out.push_str(MONTH_ABBREVIATIONS[c.month as usize - 1]),
                    _ => out.push_str(MONTHS[c.month as usize - 1]),
                }
                i += run;
            }
            'y' => {
                let year = c.year.rem_euclid(10_000) as u32;
                match run {
                    1 => out.push_str(&(year % 100).to_string()),
                    2 => push_padded(&mut out, year % 100, 2),
                    n => push_padded(&mut out, year, n),
                }
                i += run;
            }
            'h' => {
                let hour = match c.hour % 12 {
                    0 => 12,
                    h => h,
                };
                push_padded(&mut out, hour, run.min(2));
                i += run;
            }
            'H' => {
                push_padded(&mut out, c.hour, run.min(2));
                i += run;
            }
            'm' => {
                push_padded(&mut out, c.minute, run.min(2));
                i += run;
            }
            's' => {
                push_padded(&mut out, c.second, run.min(2));
                i += run;
            }
            'f' | 'F' => {
                if run > 7 {
                    return Err(DateFormatError);
                }
                let digits = format!("{:07}", c.ticks);
                let mut piece = digits[..run].to_string();
                if ch == 'F' {
                    piece = piece.trim_end_matches('0').to_string();
                }
                out.push_str(&piece);
                i += run;
            }
            't' => {
                let designator = if c.hour < 12 { "AM" } else { "PM" };
                out.push_str(if run == 1 {
                    &designator[..1]
                } else {
                    designator
                });
                i += run;
            }
            'z' => {
                let offset_minutes = (t.0 - t.local_to_utc().0) / crate::time::TICKS_PER_MINUTE;
                let sign = if offset_minutes < 0 { '-' } else { '+' };
                let hours = (offset_minutes.abs() / 60) as u32;
                let minutes = (offset_minutes.abs() % 60) as u32;
                out.push(sign);
                match run {
                    1 => out.push_str(&hours.to_string()),
                    2 => push_padded(&mut out, hours, 2),
                    _ => {
                        push_padded(&mut out, hours, 2);
                        out.push(':');
                        push_padded(&mut out, minutes, 2);
                    }
                }
                i += run;
            }
            // An unspecified kind has no zone to write.
            'K' => i += 1,
            'g' => {
                out.push_str("A.D.");
                i += run;
            }
            ':' => {
                out.push(':');
                i += 1;
            }
            '/' => {
                out.push('/');
                i += 1;
            }
            '\'' | '"' => {
                let Some(end) = chars[i + 1..].iter().position(|&x| x == ch) else {
                    return Err(DateFormatError);
                };
                out.extend(&chars[i + 1..i + 1 + end]);
                i += end + 2;
            }
            '%' => {
                // One custom specifier on its own; "%%" and a trailing "%" are not formats.
                match chars.get(i + 1) {
                    Some(&next) if next != '%' => {
                        out.push_str(&custom(t, &next.to_string())?);
                        i += 2;
                    }
                    _ => return Err(DateFormatError),
                }
            }
            '\\' => {
                let Some(&next) = chars.get(i + 1) else {
                    return Err(DateFormatError);
                };
                out.push(next);
                i += 2;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    Ok(out)
}

fn push_padded(out: &mut String, value: u32, width: usize) {
    out.push_str(&format!("{value:0width$}"));
}

/// 0 for Sunday, as `DayOfWeek` counted; 1970-01-01 was a Thursday.
fn weekday(t: FileTime) -> usize {
    (t.day_number() + 4).rem_euclid(7) as usize
}
