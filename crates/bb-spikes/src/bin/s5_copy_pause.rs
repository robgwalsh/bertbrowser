//! Spike S5: pause and cancel inside a single `CopyFileExW` call from Rust.
//!
//! The C# app pauses a transfer by blocking inside the progress routine (`ProgressCoalescer` waits on
//! a `PauseGate`), which works because `CopyFileExW` invokes that routine synchronously on the calling
//! thread. The risks for the Rust port: the routine is an `unsafe extern "system"` callback, so a
//! panic inside it must never unwind into kernel32 (that aborts the process since Rust 1.81); the
//! gate must make the copy really stop writing, not merely stop reporting; and a cancel mid-file must
//! leave no partial destination behind.
//!
//! Run: `cargo run -p bb-spikes --bin s5_copy_pause --release`. Writes and removes a few hundred MB
//! under %TEMP%.

use std::ffi::c_void;
use std::fs;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{ERROR_REQUEST_ABORTED, GetLastError, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    CALLBACK_CHUNK_FINISHED, COPY_FILE_FAIL_IF_EXISTS, CopyFileExW, PROGRESS_CANCEL,
    PROGRESS_CONTINUE,
};

const SOURCE_BYTES: u64 = 256 * 1024 * 1024;

/// Paused is a state, not an event: a routine that arrives while paused waits here until resumed or
/// cancelled. `wait` returns false for cancelled and never panics, because it is called from inside a
/// callback Windows is still executing.
struct PauseGate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[derive(Clone, Copy)]
struct GateState {
    paused: bool,
    cancelled: bool,
}

impl PauseGate {
    fn new() -> PauseGate {
        PauseGate {
            state: Mutex::new(GateState {
                paused: false,
                cancelled: false,
            }),
            changed: Condvar::new(),
        }
    }

    fn set_paused(&self, paused: bool) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).paused = paused;
        self.changed.notify_all();
    }

    fn cancel(&self) {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .cancelled = true;
        self.changed.notify_all();
    }

    /// Blocks while paused. True to continue, false to cancel.
    fn wait(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        while state.paused && !state.cancelled {
            state = self.changed.wait(state).unwrap_or_else(|p| p.into_inner());
        }
        !state.cancelled
    }
}

/// What the callback sees through `lpData`. Lives on the heap for the duration of the call.
struct Progress {
    gate: Arc<PauseGate>,
    callbacks: AtomicU32,
    transferred: AtomicI64,
    panic_on_callback: Option<u32>,
    panics_caught: AtomicU32,
    waiting: AtomicBool,
}

unsafe extern "system" fn progress_routine(
    _total: i64,
    transferred: i64,
    _stream_size: i64,
    _stream_transferred: i64,
    _stream: u32,
    reason: u32,
    _source: HANDLE,
    _dest: HANDLE,
    data: *const c_void,
) -> u32 {
    // Nothing may unwind out of here. A panic becomes a cancel, which Windows handles cleanly.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `data` is the `Progress` the caller boxed and keeps alive for the whole call.
        let p = unsafe { &*(data as *const Progress) };
        let n = p.callbacks.fetch_add(1, Ordering::SeqCst) + 1;
        if reason == CALLBACK_CHUNK_FINISHED {
            p.transferred.store(transferred, Ordering::SeqCst);
        }
        if p.panic_on_callback == Some(n) {
            panic!("deliberate panic inside the progress routine");
        }
        p.waiting.store(true, Ordering::SeqCst);
        let go_on = p.gate.wait();
        p.waiting.store(false, Ordering::SeqCst);
        go_on
    }));
    match outcome {
        Ok(true) => PROGRESS_CONTINUE,
        Ok(false) => PROGRESS_CANCEL,
        Err(_) => {
            // SAFETY: as above; the pointer is valid for the whole call.
            let p = unsafe { &*(data as *const Progress) };
            p.panics_caught.fetch_add(1, Ordering::SeqCst);
            PROGRESS_CANCEL
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

struct CopyOutcome {
    ok: bool,
    error: u32,
    elapsed: Duration,
}

/// Runs one copy on its own thread, returning the shared progress block and the join handle.
fn start_copy(
    source: &Path,
    dest: &Path,
    gate: Arc<PauseGate>,
    panic_on_callback: Option<u32>,
) -> (Arc<Progress>, thread::JoinHandle<CopyOutcome>) {
    let progress = Arc::new(Progress {
        gate,
        callbacks: AtomicU32::new(0),
        transferred: AtomicI64::new(0),
        panic_on_callback,
        panics_caught: AtomicU32::new(0),
        waiting: AtomicBool::new(false),
    });
    let source = wide(source);
    let dest = wide(dest);
    let shared = Arc::clone(&progress);
    let handle = thread::spawn(move || {
        let started = Instant::now();
        let data: *const Progress = Arc::as_ptr(&shared);
        // SAFETY: both paths are NUL-terminated; `data` outlives the call because `shared` does.
        let ok = unsafe {
            CopyFileExW(
                source.as_ptr(),
                dest.as_ptr(),
                Some(progress_routine),
                data as *const c_void,
                std::ptr::null_mut(),
                COPY_FILE_FAIL_IF_EXISTS,
            )
        } != 0;
        // SAFETY: trivially safe; reads the thread's last error.
        let error = if ok { 0 } else { unsafe { GetLastError() } };
        CopyOutcome {
            ok,
            error,
            elapsed: started.elapsed(),
        }
    });
    (progress, handle)
}

fn wait_until(what: &str, timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    println!("S5 FAIL: timed out waiting for {what}");
    false
}

fn main() {
    let mut failures = 0u32;
    let mut check = |ok: bool, what: &str| {
        if !ok {
            failures += 1;
            println!("S5 FAIL: {what}");
        }
    };

    let dir: PathBuf = std::env::temp_dir().join(format!("bb-s5-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let source = dir.join("source.bin");
    {
        let mut f = fs::File::create(&source).expect("create source");
        let chunk = vec![0xA5u8; 1024 * 1024];
        let mut written = 0u64;
        while written < SOURCE_BYTES {
            f.write_all(&chunk).expect("write source");
            written += chunk.len() as u64;
        }
    }

    // 1. Pause mid-file: bytes stop moving while the copying thread stays blocked inside the routine.
    {
        let dest = dir.join("paused.bin");
        let gate = Arc::new(PauseGate::new());
        let (p, handle) = start_copy(&source, &dest, Arc::clone(&gate), None);
        check(
            wait_until("the first chunk", Duration::from_secs(30), || {
                p.transferred.load(Ordering::SeqCst) > 0
            }),
            "copy reported progress",
        );
        gate.set_paused(true);
        check(
            wait_until("the routine to block", Duration::from_secs(10), || {
                p.waiting.load(Ordering::SeqCst)
            }),
            "routine blocked on the gate",
        );
        let frozen_at = p.transferred.load(Ordering::SeqCst);
        let frozen_calls = p.callbacks.load(Ordering::SeqCst);
        thread::sleep(Duration::from_millis(700));
        check(
            p.transferred.load(Ordering::SeqCst) == frozen_at,
            "no bytes moved while paused",
        );
        check(
            p.callbacks.load(Ordering::SeqCst) == frozen_calls,
            "no callbacks arrived while paused",
        );
        check(
            !handle.is_finished(),
            "the copy thread is still inside CopyFileExW",
        );
        // CopyFileExW pre-sizes the destination, so its length says nothing; what matters is that the
        // half-written file is still held open, so another write to it stays blocked (the C# app
        // relies on this: a paused queue is quiescent because the next job cannot touch the file).
        check(
            dest.exists() && fs::remove_file(&dest).is_err(),
            "the destination is held open while paused",
        );
        gate.set_paused(false);
        let outcome = handle.join().expect("join");
        check(outcome.ok, "copy completed after resume");
        check(
            fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) == SOURCE_BYTES,
            "destination has every byte after resume",
        );
        println!(
            "S5 pause: {} callbacks, frozen at {} MB for 700 ms, finished in {:?}",
            p.callbacks.load(Ordering::SeqCst),
            frozen_at / (1024 * 1024),
            outcome.elapsed
        );
    }

    // 2. Cancel while paused: CopyFileExW returns ERROR_REQUEST_ABORTED and removes the partial file.
    {
        let dest = dir.join("cancelled.bin");
        let gate = Arc::new(PauseGate::new());
        let (p, handle) = start_copy(&source, &dest, Arc::clone(&gate), None);
        check(
            wait_until("the first chunk", Duration::from_secs(30), || {
                p.transferred.load(Ordering::SeqCst) > 0
            }),
            "copy reported progress before cancel",
        );
        gate.set_paused(true);
        check(
            wait_until("the routine to block", Duration::from_secs(10), || {
                p.waiting.load(Ordering::SeqCst)
            }),
            "routine blocked before cancel",
        );
        gate.cancel();
        let outcome = handle.join().expect("join");
        check(!outcome.ok, "cancelled copy reports failure");
        check(
            outcome.error == ERROR_REQUEST_ABORTED,
            "cancelled copy fails with ERROR_REQUEST_ABORTED",
        );
        check(!dest.exists(), "no partial destination after cancel");
        println!(
            "S5 cancel: aborted after {} callbacks, error {}, partial removed",
            p.callbacks.load(Ordering::SeqCst),
            outcome.error
        );
    }

    // 3. A panic inside the routine is contained: it becomes a cancel, the process lives on.
    {
        let dest = dir.join("panicked.bin");
        let gate = Arc::new(PauseGate::new());
        let (p, handle) = start_copy(&source, &dest, Arc::clone(&gate), Some(3));
        let outcome = handle.join().expect("join");
        check(!outcome.ok, "copy with a panicking routine reports failure");
        check(
            outcome.error == ERROR_REQUEST_ABORTED,
            "a panic cancels the copy",
        );
        check(
            p.panics_caught.load(Ordering::SeqCst) == 1,
            "exactly one panic was caught",
        );
        check(!dest.exists(), "no partial destination after a panic");
    }

    let _ = fs::remove_dir_all(&dir);
    if failures > 0 {
        println!("S5 FAILED: {failures} check(s)");
        std::process::exit(1);
    }
    println!(
        "S5 OK: pause blocks CopyFileExW mid-file, cancel leaves no partial file, a panic in the routine is contained"
    );
}
