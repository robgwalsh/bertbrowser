//! One long operation's running counters, its cancellation token, and the rate at which it is
//! willing to talk about them. Spec: Core/Services/Transfer/ProgressCoalescer.cs.
//!
//! **The coalescing is not optional.** `CopyFileEx` calls back per chunk, thousands of times for
//! one large file, and forwarding each one to a UI-bound reporter floods the dispatcher with work
//! whose only job is to redraw a bar that has moved a pixel. Item boundaries always report; in
//! between, at most one report per `REPORT_INTERVAL`.
//!
//! **Bytes are counted per file and snapped at the end of one.** A copy may report coarsely or,
//! for a small file, not at all, so the running total is set from the size we already knew rather
//! than from whatever the last callback happened to say. That keeps the figure monotonic and makes
//! it add up to the real total at the end.
//!
//! **Pausing lives here too.** All four long writes (transfer, extract, create-archive,
//! archive-edit) funnel every byte through this type, so one gate here pauses all of them; a gate
//! in the copier would have reached only the first. It is waited on at the three places a run can
//! be held without losing its place: the start of an item, the start of a file, and each chunk
//! within one.

use std::time::{Duration, Instant};

use super::models::TransferProgress;
use super::pause_gate::PauseGate;
use crate::cancel::CancelToken;

const REPORT_INTERVAL: Duration = Duration::from_millis(100);

/// Above this many items, an item boundary stops being worth an unthrottled report. A merged
/// folder turns one item into thousands, and forcing a marshal for each one floods the UI thread
/// with exactly the work the throttle exists to prevent.
const FORCE_EVERY_ITEM_UP_TO: usize = 200;

pub struct ProgressCoalescer<'p, 'g> {
    token: CancelToken,
    progress: Option<&'p mut dyn FnMut(TransferProgress)>,
    pause: Option<&'g PauseGate>,
    items: usize,
    since_report: Instant,
    bytes_done: i64,
    file_base: i64,
    file_bytes: i64,
    file_total: i64,
    done: usize,
    name: String,
}

impl<'p, 'g> ProgressCoalescer<'p, 'g> {
    pub fn new(
        token: CancelToken,
        progress: Option<&'p mut dyn FnMut(TransferProgress)>,
        items: usize,
        pause: Option<&'g PauseGate>,
    ) -> ProgressCoalescer<'p, 'g> {
        ProgressCoalescer {
            token,
            progress,
            pause,
            items,
            since_report: Instant::now(),
            bytes_done: 0,
            file_base: 0,
            file_bytes: 0,
            file_total: 0,
            done: 0,
            name: String::new(),
        }
    }

    /// A run for work that must finish once started: clearing a name into staging, putting a
    /// displaced entry back, undoing. Reports nothing, cannot be cancelled, and, the same argument,
    /// cannot be paused: a displaced entry held in staging behind a pause is a name that exists
    /// nowhere the user can see.
    pub fn silent() -> ProgressCoalescer<'static, 'static> {
        ProgressCoalescer::new(CancelToken::none(), None, 0, None)
    }

    pub fn token(&self) -> &CancelToken {
        &self.token
    }

    /// Bytes confirmed transferred so far, across every item.
    pub fn bytes_done(&self) -> i64 {
        self.bytes_done
    }

    /// Holds the calling thread while the run is paused. Cheap when it is not.
    fn wait_while_paused(&self) {
        if let Some(pause) = self.pause {
            pause.wait(&self.token);
        }
    }

    pub fn begin_item(&mut self, name: &str) {
        self.wait_while_paused();
        self.name = name.to_string();
        self.report(self.items <= FORCE_EVERY_ITEM_UP_TO);
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
        // A tree of small files can finish each one inside a single chunk, so `file_progress` may
        // never fire; without this a pause would not land until the whole folder was through.
        self.wait_while_paused();
        self.file_base = self.bytes_done;
        self.file_bytes = 0;
        self.file_total = known_length;
    }

    /// Called from inside `CopyFileExW`'s progress routine, which is what makes blocking here a
    /// pause in the middle of a single large file rather than only between them.
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
        if !force && self.since_report.elapsed() < REPORT_INTERVAL {
            return;
        }
        self.since_report = Instant::now();
        progress(TransferProgress {
            done: self.done,
            total: self.items,
            current_name: self.name.clone(),
            bytes_done: self.bytes_done,
            current_item_bytes: self.file_bytes,
            current_item_total: self.file_total,
        });
    }
}
