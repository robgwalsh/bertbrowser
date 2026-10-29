//! One long operation's running counters, its cancel token, and the rate at which it is willing
//! to talk about them. Spec: Core/Services/Transfer/{ProgressCoalescer,PauseGate}.cs and
//! `TransferProgress` in TransferModels.cs.
//!
//! **The coalescing is not optional.** A copy reports per chunk, thousands of times for one large
//! file, and forwarding each one to a bar redraws a pixel per report. Item boundaries always
//! report; in between, at most one report per `REPORT_INTERVAL`. **Bytes are counted per file and
//! snapped at the end of one**, which keeps the figure monotonic and makes it add up to the real
//! total at the end. Pausing lives here too: all four long writes funnel every byte through this,
//! so one gate here pauses all of them.

// TODO(merge): replaced by crate::transfer::{TransferProgress, ProgressCoalescer, PauseGate} (the
// C# names). Only what the archive executors need is here.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::cancel::CancelToken;

/// One report of where a long operation has got to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferProgress {
    pub done: i32,
    pub total: i32,
    pub current_name: String,
    pub bytes_done: i64,
    pub current_item_bytes: i64,
    pub current_item_total: i64,
}

/// Where reports go. A plain closure: the caller owns the marshalling.
pub type ProgressSink<'a> = &'a mut dyn FnMut(&TransferProgress);

/// A latch a long write waits on, so the user can hold it still and let it go again. `wait`
/// never panics or unwinds: a cancelled wait returns false and the run's own token check stops it
/// at the next chunk. Cancelling therefore has to `resume` as well as trip the token.
pub struct PauseGate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Default for PauseGate {
    fn default() -> Self {
        PauseGate::new()
    }
}

impl PauseGate {
    /// A gate starts open: nothing is paused until someone asks.
    pub fn new() -> PauseGate {
        PauseGate {
            open: Mutex::new(true),
            changed: Condvar::new(),
        }
    }

    pub fn is_paused(&self) -> bool {
        !*self.open.lock().unwrap()
    }

    pub fn pause(&self) {
        *self.open.lock().unwrap() = false;
    }

    pub fn resume(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }

    /// Blocks while paused. Returns false when the wait ended because the run was cancelled
    /// rather than resumed.
    pub fn wait(&self, cancel: &CancelToken) -> bool {
        let mut open = self.open.lock().unwrap();
        while !*open {
            if cancel.is_cancelled() {
                return false;
            }
            let (guard, _) = self
                .changed
                .wait_timeout(open, Duration::from_millis(50))
                .unwrap();
            open = guard;
        }
        !cancel.is_cancelled()
    }
}

pub struct ProgressCoalescer<'p, 'g> {
    cancel: CancelToken,
    progress: Option<ProgressSink<'p>>,
    pause: Option<&'g PauseGate>,
    items: i32,
    since_report: Instant,
    bytes_done: i64,
    file_base: i64,
    file_bytes: i64,
    file_total: i64,
    done: i32,
    name: String,
}

impl<'p, 'g> ProgressCoalescer<'p, 'g> {
    const REPORT_INTERVAL: Duration = Duration::from_millis(100);

    /// Above this many items, an item boundary stops being worth an unthrottled report: a merged
    /// folder turns one item into thousands, and forcing a report for each floods the UI with
    /// exactly the work the throttle exists to prevent.
    const FORCE_EVERY_ITEM_UP_TO: i32 = 200;

    pub fn new(
        cancel: &CancelToken,
        progress: Option<ProgressSink<'p>>,
        items: i32,
        pause: Option<&'g PauseGate>,
    ) -> ProgressCoalescer<'p, 'g> {
        ProgressCoalescer {
            cancel: cancel.clone(),
            progress,
            pause,
            items,
            // Far enough back that the first throttled report goes out.
            since_report: Instant::now() - Self::REPORT_INTERVAL,
            bytes_done: 0,
            file_base: 0,
            file_bytes: 0,
            file_total: 0,
            done: 0,
            name: String::new(),
        }
    }

    /// A run for work that must finish once started. Reports nothing, cannot be cancelled, and
    /// cannot be paused.
    pub fn silent() -> ProgressCoalescer<'static, 'static> {
        ProgressCoalescer::new(&CancelToken::none(), None, 0, None)
    }

    pub fn token(&self) -> &CancelToken {
        &self.cancel
    }

    /// Bytes confirmed transferred so far, across every item.
    pub fn bytes_done(&self) -> i64 {
        self.bytes_done
    }

    fn wait_while_paused(&self) {
        if let Some(pause) = self.pause {
            pause.wait(&self.cancel);
        }
    }

    pub fn begin_item(&mut self, name: &str) {
        self.wait_while_paused();
        self.name = name.to_string();
        self.report(self.items <= Self::FORCE_EVERY_ITEM_UP_TO);
    }

    pub fn end_item(&mut self) {
        self.done += 1;
    }

    pub fn finished(&mut self) {
        self.name.clear();
        self.file_bytes = 0;
        self.file_total = 0;
        self.report(true);
    }

    pub fn begin_file(&mut self, known_length: i64) {
        // A tree of small files can finish each one inside a single chunk, so file_progress may
        // never fire; without this a pause would not land until the whole folder was through.
        self.wait_while_paused();
        self.file_base = self.bytes_done;
        self.file_bytes = 0;
        self.file_total = known_length;
    }

    pub fn file_progress(&mut self, transferred: i64, total: i64) {
        self.wait_while_paused();
        self.file_bytes = transferred;
        if total > self.file_total {
            self.file_total = total;
        }
        self.bytes_done = self.file_base + transferred;
        self.report(false);
    }

    pub fn end_file(&mut self) {
        self.bytes_done = self.file_base + self.file_total.max(self.file_bytes);
        self.file_bytes = 0;
        self.file_total = 0;
    }

    fn report(&mut self, force: bool) {
        let Some(progress) = self.progress.as_mut() else {
            return;
        };
        if !force && self.since_report.elapsed() < Self::REPORT_INTERVAL {
            return;
        }
        self.since_report = Instant::now();
        progress(&TransferProgress {
            done: self.done,
            total: self.items,
            current_name: self.name.clone(),
            bytes_done: self.bytes_done,
            current_item_bytes: self.file_bytes,
            current_item_total: self.file_total,
        });
    }
}
