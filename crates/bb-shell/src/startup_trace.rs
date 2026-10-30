//! `BERTBROWSER_STARTUP_TRACE=<file>`: one launch of the real executable, timed by the app itself, for
//! `bb-bench startup`. Spec: App/Services/StartupTrace.cs, Core/Benchmarking/StartupTraceData.cs.
//!
//! Absent the variable nothing here does anything. Present, the launch is a measurement, and three
//! things follow: the window is parked offscreen (the host asks [`StartupTrace::enabled`]); startup
//! returns after the instance listener is up and **skips everything that acts on the machine rather
//! than on this process's scratch data directory** (the index attach, the change-log push, the
//! folder-handler repair, the update check and the staging sweep: the attach in particular would
//! find the user's real helper and push "recording off" at it, wiping their change log); and the
//! marks are written as soon as the content has been drawn and the first listing is in. With
//! `BERTBROWSER_EXIT_AFTER_STARTUP=1` the window is then closed.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use bb_core::FileTime;
use bb_core::benchmarking::{
    StartupMarks, StartupMemory, StartupMemoryPoints, StartupTraceData, StartupTraceJson,
};

pub const FILE_VARIABLE: &str = "BERTBROWSER_STARTUP_TRACE";
pub const EXIT_VARIABLE: &str = "BERTBROWSER_EXIT_AFTER_STARTUP";

/// A launch whose first listing never arrives still writes what it has, after this long.
pub const FALLBACK: Duration = Duration::from_secs(60);

struct Inner {
    clock: Option<Instant>,
    clock_offset_ms: f64,
    process_start: FileTime,
    marks: BTreeMap<String, f64>,
    instance: String,
    flushed: bool,
}

pub struct StartupTrace {
    path: Option<PathBuf>,
    exit_after: bool,
    inner: Mutex<Inner>,
}

static TRACE: OnceLock<StartupTrace> = OnceLock::new();

/// The process-wide trace, configured from the environment on first use.
pub fn global() -> &'static StartupTrace {
    TRACE.get_or_init(|| {
        let path = std::env::var(FILE_VARIABLE)
            .ok()
            .filter(|p| !p.is_empty())
            .map(|p| std::path::absolute(&p).unwrap_or_else(|_| PathBuf::from(p)));
        StartupTrace::new(path, std::env::var(EXIT_VARIABLE).as_deref() == Ok("1"))
    })
}

impl StartupTrace {
    pub fn new(path: Option<PathBuf>, exit_after: bool) -> StartupTrace {
        StartupTrace {
            path,
            exit_after,
            inner: Mutex::new(Inner {
                clock: None,
                clock_offset_ms: 0.0,
                process_start: FileTime::default(),
                marks: BTreeMap::new(),
                instance: "unknown".to_string(),
                flushed: false,
            }),
        }
    }

    pub fn enabled(&self) -> bool {
        self.path.is_some()
    }

    pub fn exit_after(&self) -> bool {
        self.exit_after
    }

    /// Records the moment, in milliseconds since the process started. The first mark of a name wins.
    pub fn mark(&self, name: &str) {
        if !self.enabled() {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        if inner.clock.is_none() {
            // The gap from process start to the first mark is the OS's clock against ours, a
            // millisecond or so coarse, which is why `main` is reported rather than used as a zero.
            let start = process_start();
            inner.clock_offset_ms = FileTime::now().ticks_since(start) as f64 / 10_000.0;
            inner.process_start = start;
            inner.clock = Some(Instant::now());
        }
        let at = inner.clock_offset_ms
            + inner
                .clock
                .map_or(0.0, |c| c.elapsed().as_secs_f64() * 1000.0);
        inner.marks.entry(name.to_string()).or_insert(at);
    }

    pub fn set_instance(&self, is_first: bool) {
        self.inner.lock().unwrap().instance = if is_first { "first" } else { "second" }.to_string();
    }

    pub fn has(&self, name: &str) -> bool {
        self.inner.lock().unwrap().marks.contains_key(name)
    }

    /// Writes the trace once `contentRendered`, `firstListing` and `gfxSettled` are in. The last is why the
    /// memory it reads is the settled app's: the window starts on a software device and the GPU's driver
    /// loads behind it, and a snapshot taken while that loads is mostly the driver. True when this call
    /// wrote it.
    pub fn finish_if_complete(&self) -> bool {
        if !(self.has(StartupMarks::CONTENT_RENDERED)
            && self.has(StartupMarks::FIRST_LISTING)
            && self.has(StartupMarks::GFX_SETTLED))
        {
            return false;
        }
        self.finish(None)
    }

    /// Writes what there is, with the reason it is incomplete if it is. Once.
    pub fn finish(&self, error: Option<&str>) -> bool {
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.flushed {
                return false;
            }
            inner.flushed = true;
        }
        let Some(path) = &self.path else {
            return false;
        };
        // After every mark is in, so the compaction it forces is on nobody's clock.
        let mut memory = BTreeMap::new();
        memory.insert(StartupMemoryPoints::FIRST_LISTING.to_string(), snapshot());
        compact_heap();
        memory.insert(StartupMemoryPoints::COLLECTED.to_string(), snapshot());

        let trace = {
            let inner = self.inner.lock().unwrap();
            StartupTraceData {
                pid: std::process::id(),
                process_start_utc: inner.process_start,
                marks_ms: inner.marks.clone(),
                instance: inner.instance.clone(),
                indexer: "skipped".to_string(),
                side_effects_skipped: true,
                error: error.map(str::to_string),
                memory: Some(memory),
            }
        };
        // A trace that cannot be written is a failed measurement, not a failed launch.
        StartupTraceJson::write(&trace, path).is_ok()
    }

    /// Watches until the first listing is in (`first_listing`), marks it, and writes the trace when
    /// the content has also been drawn. Runs on its own thread; `on_written` is called once the trace
    /// is on disk (the host closes the window when the launch asked it to).
    pub fn watch(
        &'static self,
        first_listing: impl Fn() -> bool + Send + 'static,
        on_written: impl Fn() + Send + 'static,
    ) {
        if !self.enabled() {
            return;
        }
        let _ = std::thread::Builder::new()
            .name("bertbrowser startup trace".to_string())
            .spawn(move || {
                let started = Instant::now();
                loop {
                    if self.has(StartupMarks::FIRST_LISTING) || first_listing() {
                        self.mark(StartupMarks::FIRST_LISTING);
                        if self.has(StartupMarks::CONTENT_RENDERED)
                            && self.has(StartupMarks::GFX_SETTLED)
                        {
                            if self.finish_if_complete() {
                                on_written();
                            }
                            return;
                        }
                    }
                    if started.elapsed() > FALLBACK {
                        if self.finish(Some("the first listing never arrived")) {
                            on_written();
                        }
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            });
    }
}

fn process_start() -> FileTime {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: four valid out pointers.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        return FileTime::now();
    }
    FileTime::from_parts(created.dwLowDateTime, created.dwHighDateTime)
}

fn snapshot() -> StartupMemory {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: a zeroed struct with its size set, as the call requires.
    let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    // SAFETY: `counters` is initialised and `cb` says how big it is.
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            counters.cb,
        )
    };
    StartupMemory {
        working_set_bytes: counters.WorkingSetSize as i64,
        private_bytes: counters.PrivateUsage as i64,
        heap_bytes: heap_committed(),
        peak_working_set_bytes: counters.PeakWorkingSetSize as i64,
    }
}

/// Bytes committed in the process heap, which is what Rust's global allocator draws from on Windows.
fn heap_committed() -> i64 {
    use windows_sys::Win32::System::Memory::{GetProcessHeap, HEAP_SUMMARY, HeapSummary};

    // SAFETY: a zeroed struct with its size set, as the call requires.
    let mut summary: HEAP_SUMMARY = unsafe { std::mem::zeroed() };
    summary.cb = std::mem::size_of::<HEAP_SUMMARY>() as u32;
    // SAFETY: `summary` is initialised.
    if unsafe { HeapSummary(GetProcessHeap(), 0, &mut summary) } == 0 {
        return 0;
    }
    summary.cbCommitted as i64
}

/// The steadier of the two readings: what the app is actually holding once free blocks are returned.
fn compact_heap() {
    use windows_sys::Win32::System::Memory::{GetProcessHeap, HeapCompact};
    // SAFETY: plain call on the process heap.
    unsafe { HeapCompact(GetProcessHeap(), 0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = bb_test_support::unique_temp_path(&format!("trace-{tag}"), "");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("trace.json")
    }

    #[test]
    fn without_a_path_every_call_is_a_no_op() {
        let t = StartupTrace::new(None, false);
        t.mark("main");
        assert!(!t.enabled());
        assert!(!t.has("main"));
        assert!(!t.finish(None));
    }

    #[test]
    fn the_first_mark_of_a_name_wins_and_marks_only_move_forward() {
        let t = StartupTrace::new(Some(scratch("marks")), false);
        t.mark(StartupMarks::MAIN);
        std::thread::sleep(Duration::from_millis(15));
        t.mark(StartupMarks::VELOPACK_DONE);
        t.mark(StartupMarks::MAIN);
        let inner = t.inner.lock().unwrap();
        assert!(inner.marks[StartupMarks::VELOPACK_DONE] >= inner.marks[StartupMarks::MAIN] + 10.0);
    }

    #[test]
    fn a_complete_trace_is_written_once_in_the_schema_the_bench_tool_reads() {
        let path = scratch("write");
        let t = StartupTrace::new(Some(path.clone()), false);
        t.set_instance(true);
        t.mark(StartupMarks::MAIN);
        assert!(!t.finish_if_complete(), "neither mark is in yet");
        t.mark(StartupMarks::CONTENT_RENDERED);
        assert!(
            !t.finish_if_complete(),
            "the first listing is still missing"
        );
        t.mark(StartupMarks::FIRST_LISTING);
        assert!(
            !t.finish_if_complete(),
            "the drawing has not settled: a driver may still be loading"
        );
        t.mark(StartupMarks::GFX_SETTLED);
        assert!(t.finish_if_complete());
        assert!(!t.finish_if_complete(), "written once");

        let read = StartupTraceJson::read(&path).unwrap();
        assert_eq!("first", read.instance);
        assert_eq!("skipped", read.indexer);
        assert!(read.side_effects_skipped);
        assert_eq!(None, read.error);
        assert_eq!(std::process::id(), read.pid);
        assert!(read.marks_ms.contains_key(StartupMarks::FIRST_LISTING));
        let memory = read.memory.unwrap();
        assert!(memory[StartupMemoryPoints::FIRST_LISTING].working_set_bytes > 0);
        assert!(memory[StartupMemoryPoints::COLLECTED].private_bytes > 0);
    }

    #[test]
    fn a_launch_that_never_lists_reports_why() {
        let path = scratch("error");
        let t = StartupTrace::new(Some(path.clone()), false);
        t.mark(StartupMarks::MAIN);
        assert!(t.finish(Some("the first listing never arrived")));
        assert_eq!(
            Some("the first listing never arrived".to_string()),
            StartupTraceJson::read(&path).unwrap().error
        );
    }

    #[test]
    fn the_watcher_marks_the_listing_and_calls_back_once_the_content_is_drawn() {
        let path = scratch("watch");
        let t: &'static StartupTrace =
            Box::leak(Box::new(StartupTrace::new(Some(path.clone()), true)));
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        t.watch(
            || true,
            move || {
                let _ = tx.lock().unwrap().send(());
            },
        );
        std::thread::sleep(Duration::from_millis(30));
        assert!(rx.try_recv().is_err(), "content has not been drawn yet");
        t.mark(StartupMarks::CONTENT_RENDERED);
        std::thread::sleep(Duration::from_millis(30));
        assert!(rx.try_recv().is_err(), "the drawing has not settled yet");
        t.mark(StartupMarks::GFX_SETTLED);
        rx.recv_timeout(Duration::from_secs(5)).expect("written");
        assert!(t.exit_after());
        assert!(
            StartupTraceJson::read(&path)
                .unwrap()
                .marks_ms
                .contains_key(StartupMarks::FIRST_LISTING)
        );
    }
}
