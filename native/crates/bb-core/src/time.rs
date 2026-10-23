//! Timestamps as Windows FILETIME ticks: 100 ns units since 1601-01-01 UTC. The unit the MFT, the USN
//! journal and every Win32 call already speak, so nothing is converted on the hot paths, and it is
//! what the database stores. Zero means **unknown** and renders blank, never as a date.

use std::ops::{Add, Sub};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::Globalization::{
    DATE_SHORTDATE, GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS,
};
use windows_sys::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows_sys::Win32::System::Time::{
    FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
    TzSpecificLocalTimeToSystemTime,
};

pub const TICKS_PER_SECOND: i64 = 10_000_000;
pub const TICKS_PER_MINUTE: i64 = 60 * TICKS_PER_SECOND;
pub const TICKS_PER_HOUR: i64 = 60 * TICKS_PER_MINUTE;
pub const TICKS_PER_DAY: i64 = 24 * TICKS_PER_HOUR;
/// 1970-01-01 in FILETIME ticks.
const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct FileTime(pub i64);

/// A broken-down civil time, UTC or local depending on where it came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// Sub-second ticks (100 ns).
    pub ticks: u32,
}

impl FileTime {
    pub const UNKNOWN: FileTime = FileTime(0);

    pub fn now() -> FileTime {
        let mut ft = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        // SAFETY: the out pointer is valid for the call.
        unsafe { GetSystemTimePreciseAsFileTime(&mut ft) };
        FileTime::from_parts(ft.dwLowDateTime, ft.dwHighDateTime)
    }

    pub fn from_parts(low: u32, high: u32) -> FileTime {
        FileTime(((high as i64) << 32) | low as i64)
    }

    pub fn to_parts(self) -> (u32, u32) {
        (self.0 as u32, (self.0 >> 32) as u32)
    }

    pub fn is_unknown(self) -> bool {
        self.0 == 0
    }

    /// A civil UTC date and time.
    pub fn from_ymd_hms(
        year: i64,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> FileTime {
        let days = days_from_civil(year, month, day);
        let secs =
            days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second);
        FileTime(secs * TICKS_PER_SECOND + UNIX_EPOCH_TICKS)
    }

    pub fn from_ymd(year: i64, month: u32, day: u32) -> FileTime {
        Self::from_ymd_hms(year, month, day, 0, 0, 0)
    }

    pub fn civil(self) -> Civil {
        let since_unix = self.0 - UNIX_EPOCH_TICKS;
        let secs = since_unix.div_euclid(TICKS_PER_SECOND);
        let ticks = since_unix.rem_euclid(TICKS_PER_SECOND) as u32;
        let days = secs.div_euclid(86_400);
        let sod = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Civil {
            year,
            month,
            day,
            hour: (sod / 3_600) as u32,
            minute: ((sod % 3_600) / 60) as u32,
            second: (sod % 60) as u32,
            ticks,
        }
    }

    /// The date part alone, as the number of whole days since 1970-01-01.
    pub fn day_number(self) -> i64 {
        (self.0 - UNIX_EPOCH_TICKS).div_euclid(TICKS_PER_DAY)
    }

    pub fn add_ticks(self, ticks: i64) -> FileTime {
        FileTime(self.0 + ticks)
    }
    pub fn add_secs(self, seconds: i64) -> FileTime {
        self.add_ticks(seconds * TICKS_PER_SECOND)
    }
    pub fn add_minutes(self, minutes: i64) -> FileTime {
        self.add_ticks(minutes * TICKS_PER_MINUTE)
    }
    pub fn add_hours(self, hours: i64) -> FileTime {
        self.add_ticks(hours * TICKS_PER_HOUR)
    }
    pub fn add_days(self, days: i64) -> FileTime {
        self.add_ticks(days * TICKS_PER_DAY)
    }

    /// Ticks from `earlier` to `self`; negative when `earlier` is later.
    pub fn ticks_since(self, earlier: FileTime) -> i64 {
        self.0 - earlier.0
    }

    pub fn from_unix_millis(millis: i64) -> FileTime {
        FileTime(millis * (TICKS_PER_SECOND / 1000) + UNIX_EPOCH_TICKS)
    }

    pub fn to_unix_millis(self) -> i64 {
        (self.0 - UNIX_EPOCH_TICKS).div_euclid(TICKS_PER_SECOND / 1000)
    }

    pub fn from_system_time(t: SystemTime) -> FileTime {
        match t.duration_since(UNIX_EPOCH) {
            Ok(d) => FileTime(duration_ticks(d) + UNIX_EPOCH_TICKS),
            Err(e) => FileTime(UNIX_EPOCH_TICKS - duration_ticks(e.duration())),
        }
    }

    pub fn to_system_time(self) -> SystemTime {
        let since_unix = self.0 - UNIX_EPOCH_TICKS;
        if since_unix >= 0 {
            UNIX_EPOCH + ticks_duration(since_unix)
        } else {
            UNIX_EPOCH - ticks_duration(-since_unix)
        }
    }

    /// The same instant as local wall-clock ticks, for display. Uses the zone's rules for that
    /// instant, as `DateTime.ToLocalTime` did.
    /// Local wall-clock ticks back to the UTC instant they name, through the zone rules for that
    /// time, as `DateTime.ToUniversalTime` did for a local value.
    pub fn local_to_utc(self) -> FileTime {
        let local = self.system_time_struct();
        let mut utc = local;
        // SAFETY: valid pointers; a null time zone means the current one.
        let ok = unsafe { TzSpecificLocalTimeToSystemTime(std::ptr::null(), &local, &mut utc) };
        if ok == 0 {
            return self;
        }
        let mut ft = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        // SAFETY: valid pointers.
        if unsafe { SystemTimeToFileTime(&utc, &mut ft) } == 0 {
            return self;
        }
        FileTime::from_parts(ft.dwLowDateTime, ft.dwHighDateTime)
    }

    pub fn to_local(self) -> FileTime {
        let utc = self.system_time_struct();
        let mut local = utc;
        // SAFETY: valid pointers; a null time zone means the current one.
        let ok = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) };
        if ok == 0 {
            return self;
        }
        let mut ft = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        // SAFETY: valid pointers.
        if unsafe { SystemTimeToFileTime(&local, &mut ft) } == 0 {
            return self;
        }
        FileTime::from_parts(ft.dwLowDateTime, ft.dwHighDateTime)
    }

    fn system_time_struct(self) -> SYSTEMTIME {
        let (low, high) = self.to_parts();
        let ft = FILETIME {
            dwLowDateTime: low,
            dwHighDateTime: high,
        };
        let mut st = SYSTEMTIME {
            wYear: 0,
            wMonth: 0,
            wDayOfWeek: 0,
            wDay: 0,
            wHour: 0,
            wMinute: 0,
            wSecond: 0,
            wMilliseconds: 0,
        };
        // SAFETY: valid pointers.
        unsafe { FileTimeToSystemTime(&ft, &mut st) };
        st
    }

    /// `HH:mm` of this value read as wall-clock ticks.
    pub fn hhmm(self) -> String {
        let c = self.civil();
        format!("{:02}:{:02}", c.hour, c.minute)
    }
}

impl Add<Duration> for FileTime {
    type Output = FileTime;
    fn add(self, rhs: Duration) -> FileTime {
        FileTime(self.0 + duration_ticks(rhs))
    }
}

impl Sub<Duration> for FileTime {
    type Output = FileTime;
    fn sub(self, rhs: Duration) -> FileTime {
        FileTime(self.0 - duration_ticks(rhs))
    }
}

pub fn duration_ticks(d: Duration) -> i64 {
    (d.as_nanos() / 100) as i64
}

pub fn ticks_duration(ticks: i64) -> Duration {
    Duration::from_nanos((ticks.max(0) as u64) * 100)
}

/// Days since 1970-01-01 for a proleptic Gregorian civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse of [`days_from_civil`].
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub const MONTH_ABBREVIATIONS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The user's short date and short time for a UTC instant, as `DateTime.ToString("g")` gave
/// after `ToLocalTime()`: the locale's own order and separators.
pub fn format_short_local(utc: FileTime) -> String {
    let st = utc.to_local().system_time_struct();
    let date = locale_format(|buf, len| unsafe {
        GetDateFormatEx(
            std::ptr::null(),
            DATE_SHORTDATE,
            &st,
            std::ptr::null(),
            buf,
            len,
            std::ptr::null(),
        )
    });
    let time = locale_format(|buf, len| unsafe {
        GetTimeFormatEx(
            std::ptr::null(),
            TIME_NOSECONDS,
            &st,
            std::ptr::null(),
            buf,
            len,
        )
    });
    format!("{date} {time}")
}

fn locale_format(call: impl Fn(*mut u16, i32) -> i32) -> String {
    let needed = call(std::ptr::null_mut(), 0);
    if needed <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; needed as usize];
    let written = call(buf.as_mut_ptr(), needed);
    if written <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(written as usize).saturating_sub(1)])
}
