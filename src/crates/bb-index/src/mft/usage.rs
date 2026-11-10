//! What the index helper costs: its memory, CPU and disk traffic, measured by the helper itself and
//! pushed down the pipe as `Usage`.
//!
//! **The helper measures itself because nobody else can.** It runs with an administrator token and
//! the app does not; no-read-up denies a medium-integrity process `PROCESS_VM_READ` on it, which
//! `GetProcessMemoryInfo` wants, and the app usually attached to a helper it did not start and so has
//! no id for it anyway. What crosses the pipe is raw counters, never rates or words: the app turns two
//! samples into a rate with the helper's own uptime as the clock (so no timer on either side has to
//! agree with the other), and words the rows in one place for the title-bar tip and Settings alike.

use std::sync::Arc;

use crate::ipc::IndexProtocol;
use bb_base::format;

/// One reading of the helper's counters. Every figure is cumulative or instantaneous, never a rate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessUsageSample {
    pub pid: u64,
    pub working_set: u64,
    pub private_bytes: u64,
    pub peak_working_set: u64,
    /// Kernel plus user time, in 100 ns units.
    pub cpu_100ns: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub handles: u64,
    /// Milliseconds since the process started: the clock rates are measured against.
    pub uptime_ms: u64,
    /// Logical processors, so a CPU percentage means what Task Manager means by one.
    pub cpus: u64,
}

impl ProcessUsageSample {
    /// The `Usage` argument for this sample.
    pub fn to_argument(&self) -> String {
        IndexProtocol::format_usage(&[
            ("pid", self.pid),
            ("ws", self.working_set),
            ("priv", self.private_bytes),
            ("peak", self.peak_working_set),
            ("cpu", self.cpu_100ns),
            ("rd", self.read_bytes),
            ("wr", self.write_bytes),
            ("h", self.handles),
            ("up", self.uptime_ms),
            ("n", self.cpus),
        ])
    }

    /// Reads a `Usage` argument back. Keys it does not know are skipped (a later helper may send
    /// more); one it needs but is missing is zero, which rows word as nothing rather than a figure.
    pub fn from_argument(argument: &str) -> Option<ProcessUsageSample> {
        let pairs = IndexProtocol::parse_usage(argument)?;
        let mut sample = ProcessUsageSample::default();
        for (key, value) in pairs {
            match key {
                "pid" => sample.pid = value,
                "ws" => sample.working_set = value,
                "priv" => sample.private_bytes = value,
                "peak" => sample.peak_working_set = value,
                "cpu" => sample.cpu_100ns = value,
                "rd" => sample.read_bytes = value,
                "wr" => sample.write_bytes = value,
                "h" => sample.handles = value,
                "up" => sample.uptime_ms = value,
                "n" => sample.cpus = value,
                _ => {}
            }
        }
        Some(sample)
    }
}

/// Reads the counters of the process it runs in.
pub trait ProcessUsageProbe: Send + Sync {
    /// None when the counters could not be read; the helper then sends nothing that tick.
    fn sample(&self) -> Option<ProcessUsageSample>;
}

/// One row as both the tip and the Settings page show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageRow {
    /// Stable, for the harness and for updating a row in place.
    pub id: &'static str,
    pub label: &'static str,
    pub value: String,
    /// A second figure beside `value`, empty for most rows. Disk traffic splits in two (the rate,
    /// the running total) so a host can give the rate a column of fixed width
    /// (`RATE_WIDTH_TEMPLATE`) and the total does not shift every time the rate's digits do.
    pub detail: String,
}

/// The widest rate the rows write: divided until under 1024, one decimal at every unit.
/// What a host measures to size the rate's column, so the column never has to grow.
pub const RATE_WIDTH_TEMPLATE: &str = "1023.9 MB/s";

/// The latest sample and the rates since the one before it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndexerUsage {
    pub sample: ProcessUsageSample,
    /// Percent of the whole machine, None until there are two samples of one process to compare.
    pub cpu_percent: Option<f64>,
    pub read_per_second: Option<f64>,
    pub write_per_second: Option<f64>,
}

impl IndexerUsage {
    /// The usage `next` shows, given the sample before it. A first sample, a different process
    /// (the helper was restarted) or a clock that did not move has no rates: nothing honest to
    /// divide by.
    pub fn advance(
        previous: Option<&ProcessUsageSample>,
        next: ProcessUsageSample,
    ) -> IndexerUsage {
        let mut usage = IndexerUsage {
            sample: next,
            cpu_percent: None,
            read_per_second: None,
            write_per_second: None,
        };
        let Some(previous) = previous else {
            return usage;
        };
        if previous.pid != next.pid || next.uptime_ms <= previous.uptime_ms {
            return usage;
        }
        let seconds = (next.uptime_ms - previous.uptime_ms) as f64 / 1000.0;
        let cpus = next.cpus.max(1) as f64;
        let cpu_seconds = next.cpu_100ns.saturating_sub(previous.cpu_100ns) as f64 / 10_000_000.0;
        usage.cpu_percent = Some((cpu_seconds / (seconds * cpus) * 100.0).clamp(0.0, 100.0));
        usage.read_per_second =
            Some(next.read_bytes.saturating_sub(previous.read_bytes) as f64 / seconds);
        usage.write_per_second =
            Some(next.write_bytes.saturating_sub(previous.write_bytes) as f64 / seconds);
        usage
    }

    /// Every row, in the order they are shown.
    pub fn rows(&self) -> Vec<UsageRow> {
        let s = &self.sample;
        let row = |id, label, value| UsageRow {
            id,
            label,
            value,
            detail: String::new(),
        };
        let traffic = |id, label, rate, total| UsageRow {
            id,
            label,
            value: rate_text(rate),
            detail: format!("{} total", bytes(total)),
        };
        vec![
            row("memory", "Memory", bytes(s.working_set)),
            row("private", "Private memory", bytes(s.private_bytes)),
            row("peak", "Peak memory", bytes(s.peak_working_set)),
            row("cpu", "CPU", percent(self.cpu_percent)),
            traffic("read", "Disk read", self.read_per_second, s.read_bytes),
            traffic("write", "Disk write", self.write_per_second, s.write_bytes),
            row("handles", "Handles", format::thousands(s.handles as i64)),
            row("uptime", "Running for", uptime(s.uptime_ms)),
            row("pid", "Process ID", s.pid.to_string()),
        ]
    }
}

fn bytes(n: u64) -> String {
    format::byte_size(i64::try_from(n).unwrap_or(i64::MAX))
}

/// "0.4%", "<0.1%" for a busy-but-tiny figure, an em dash until there is a second sample.
fn percent(value: Option<f64>) -> String {
    match value {
        None => "\u{2014}".to_string(),
        Some(0.0) => "0%".to_string(),
        Some(p) if p < 0.1 => "<0.1%".to_string(),
        Some(p) if p < 10.0 => format!("{p:.1}%"),
        Some(p) => format!("{p:.0}%"),
    }
}

/// "1.2 MB/s", an em dash until there is a second sample, and "0 B/s" for an idle second (here a
/// zero is the answer, not a blank). One decimal at every unit, unlike `format::byte_size`'s three
/// for GB: a rate is read at a glance, and a fixed precision is what keeps it inside the column
/// `RATE_WIDTH_TEMPLATE` sizes.
fn rate_text(rate: Option<f64>) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let Some(mut value) = rate else {
        return "\u{2014}".to_string();
    };
    if value < 1.0 {
        return "0 B/s".to_string();
    }
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{} B/s", value.round() as u64);
    }
    // Rounding to one decimal can reach 1024.0, which belongs to the next unit.
    if (value * 10.0).round() >= 10240.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let one_decimal = format!("{value:.1}");
    let trimmed = one_decimal.strip_suffix(".0").unwrap_or(&one_decimal);
    format!("{trimmed} {}/s", UNITS[unit])
}

/// "45 s", "12 min", "3 h 12 min", "2 d 4 h".
fn uptime(ms: u64) -> String {
    let seconds = ms / 1000;
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60);
    if seconds < 60 {
        format!("{seconds} s")
    } else if seconds < 3600 {
        format!("{minutes} min")
    } else if days == 0 {
        format!("{} h {minutes} min", seconds / 3600)
    } else {
        format!("{days} d {hours} h")
    }
}

/// The helper's own counters, read from inside it: nothing here opens another process.
#[derive(Default)]
pub struct CurrentProcessUsage;

impl CurrentProcessUsage {
    pub fn shared() -> Arc<dyn ProcessUsageProbe> {
        Arc::new(CurrentProcessUsage)
    }
}

impl ProcessUsageProbe for CurrentProcessUsage {
    fn sample(&self) -> Option<ProcessUsageSample> {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
        };
        use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, GetCurrentProcessId, GetProcessHandleCount, GetProcessIoCounters,
            GetProcessTimes, IO_COUNTERS,
        };

        let process = unsafe { GetCurrentProcess() };

        // SAFETY: a zeroed struct with its size set, as the call requires.
        let mut memory: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
        memory.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        // SAFETY: `memory` is initialised and `cb` says how big it is.
        let ok = unsafe {
            GetProcessMemoryInfo(
                process,
                (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                memory.cb,
            )
        };
        if ok == 0 {
            return None;
        }

        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: four valid out pointers.
        let ok =
            unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
        if ok == 0 {
            return None;
        }
        let mut now = zero;
        // SAFETY: one valid out pointer.
        unsafe { GetSystemTimeAsFileTime(&mut now) };

        // SAFETY: a plain struct of integers; zero is a valid value.
        let mut io: IO_COUNTERS = unsafe { std::mem::zeroed() };
        // SAFETY: a valid out pointer. A failure leaves the zeros, which read as no traffic.
        unsafe { GetProcessIoCounters(process, &mut io) };

        let mut handles = 0u32;
        // SAFETY: a valid out pointer.
        unsafe { GetProcessHandleCount(process, &mut handles) };

        let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        Some(ProcessUsageSample {
            pid: u64::from(unsafe { GetCurrentProcessId() }),
            working_set: memory.WorkingSetSize as u64,
            private_bytes: memory.PrivateUsage as u64,
            peak_working_set: memory.PeakWorkingSetSize as u64,
            cpu_100ns: ticks(kernel) + ticks(user),
            read_bytes: io.ReadTransferCount,
            write_bytes: io.WriteTransferCount,
            handles: u64::from(handles),
            uptime_ms: ticks(now).saturating_sub(ticks(created)) / 10_000,
            cpus: std::thread::available_parallelism().map_or(1, |n| n.get() as u64),
        })
    }
}
