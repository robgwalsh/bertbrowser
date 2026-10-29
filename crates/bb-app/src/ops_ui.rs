//! What the operations UI reads: the transfer in flight (a tracker that turns progress reports into
//! the words the strip and the Transfers window show), the queue behind it (reorderable, each job
//! cancellable, the running one included), the question a clash puts to the user and the offer of
//! administrator rights. Spec: TransferProgressViewModel, TransferQueueViewModel,
//! TransferConflictsViewModel, ShellViewModel (EnqueueAsync, DrainQueueAsync).
//!
//! Everything here is a *model*: JSON the host draws and verbs the host calls (`ops_query`,
//! `ops_do`). The host never keeps a second copy of the truth, and a harness can pose the same
//! models (`pose-transfer`) because the surfaces it photographs cannot be caught mid-drain.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use bb_core::cancel::CancelToken;
use bb_core::format;
use bb_core::transfer::{
    ClashSide, ConflictDefaults, ConflictResolution, QueuedJobState, QueuedJobSummary, Resolutions,
    TransferEstimate, TransferPlan, TransferProgress, TransferQueueRules, TransferRate,
    TransferVerb,
};
use bb_paths::{PathKey, file_name};
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::events::Model;

// ---- the job records --------------------------------------------------------------------------------

/// What a queued write says about itself before it runs.
#[derive(Clone, Debug)]
pub(crate) struct JobMeta {
    /// "Copy", "Move", "Extract", "Compress", "Archive edit"; or a name that is not shown.
    pub kind: String,
    /// "12 items to Archive".
    pub description: String,
    pub items: usize,
    pub estimate: TransferEstimate,
    /// False for the short, exclusive operations (rename, delete, undo): they run on the same
    /// thread but are not rows of the queue.
    pub show: bool,
}

impl JobMeta {
    pub(crate) fn hidden(kind: &str) -> JobMeta {
        JobMeta {
            kind: kind.to_string(),
            description: String::new(),
            items: 0,
            estimate: TransferEstimate::NOTHING,
            show: false,
        }
    }

    pub(crate) fn shown(
        kind: &str,
        description: String,
        items: usize,
        estimate: TransferEstimate,
    ) -> JobMeta {
        JobMeta {
            kind: kind.to_string(),
            description,
            items,
            estimate,
            show: true,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct JobRecord {
    pub id: u64,
    pub meta: JobMeta,
    pub state: QueuedJobState,
}

impl JobRecord {
    fn summary(&self) -> QueuedJobSummary {
        QueuedJobSummary {
            id: self.id as i32,
            kind: self.meta.kind.clone(),
            description: self.meta.description.clone(),
            items: self.meta.items,
            estimate: self.meta.estimate,
            state: self.state,
        }
    }
}

// ---- the tracker ------------------------------------------------------------------------------------

pub(crate) struct TrackedItem {
    pub name: String,
    pub destination: String,
    /// 0 waiting, 1 working, 2 done.
    pub state: u8,
}

/// Turns progress reports into the strings of one job's headline, bar and item rows.
pub(crate) struct TransferTracker {
    verbing: String,
    items_total: usize,
    items: Vec<TrackedItem>,
    estimate: TransferEstimate,
    indeterminate: bool,
    headline: String,
    bytes_done: i64,
    bytes_total: i64,
    fraction: f64,
    bytes_text: String,
    rate_text: String,
    eta_text: String,
    rate: TransferRate,
    started: Instant,
    paused_at: Option<Instant>,
    paused_total: Duration,
    marked_up_to: usize,
}

impl TransferTracker {
    pub(crate) fn new(
        verbing: &str,
        items: Vec<(String, String)>,
        estimate: TransferEstimate,
    ) -> TransferTracker {
        let total = items.len();
        TransferTracker {
            verbing: verbing.to_string(),
            items_total: total,
            items: items
                .into_iter()
                .map(|(name, destination)| TrackedItem {
                    name,
                    destination,
                    state: 0,
                })
                .collect(),
            estimate,
            indeterminate: !estimate.is_usable(),
            headline: format!(
                "{verbing} {} item(s)\u{2026}",
                format::thousands(total as i64)
            ),
            bytes_done: 0,
            bytes_total: estimate.bytes,
            fraction: 0.0,
            bytes_text: String::new(),
            rate_text: String::new(),
            eta_text: String::new(),
            rate: TransferRate::new(),
            started: Instant::now(),
            paused_at: None,
            paused_total: Duration::ZERO,
            marked_up_to: 0,
        }
    }

    pub(crate) fn for_plan(plan: &TransferPlan, estimate: TransferEstimate) -> TransferTracker {
        let verbing = if plan.verb == TransferVerb::Move {
            "Moving"
        } else {
            "Copying"
        };
        let items = plan
            .transfers
            .iter()
            .map(|t| {
                (
                    plan.label_for(t),
                    bb_core::rename::paths::directory_name(&t.destination_path).unwrap_or_default(),
                )
            })
            .collect();
        TransferTracker::new(verbing, items, estimate)
    }

    fn elapsed(&self) -> Duration {
        let now = self.paused_at.unwrap_or_else(Instant::now);
        now.saturating_duration_since(self.started)
            .saturating_sub(self.paused_total)
    }

    /// Stops the clock while paused: the rate is fed elapsed time, so a watch left running turns a
    /// five-minute pause into an invented stall.
    pub(crate) fn set_paused(&mut self, paused: bool) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(Instant::now()),
            (false, Some(at)) => {
                self.paused_total += at.elapsed();
                self.paused_at = None;
            }
            _ => {}
        }
    }

    pub(crate) fn apply(&mut self, p: &TransferProgress, paused: bool) {
        self.bytes_done = p.bytes_done;
        let elapsed = self.elapsed();
        self.rate.observe(p.bytes_done, elapsed);
        self.bytes_text = self.describe_bytes();
        if !self.indeterminate && self.bytes_total > 0 {
            self.fraction = (p.bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0);
        }
        self.rate_text = format::speed(self.rate.bytes_per_second());
        self.eta_text = format::remaining(self.rate.remaining(p.bytes_done, &self.estimate));
        if !paused {
            self.headline = if p.current_name.is_empty() {
                format!(
                    "{} {} item(s)\u{2026}",
                    self.verbing,
                    format::thousands(self.items_total as i64)
                )
            } else {
                format!(
                    "{} {} of {} \u{2014} {}",
                    self.verbing,
                    format::thousands((p.done + 1).min(self.items_total) as i64),
                    format::thousands(self.items_total as i64),
                    p.current_name
                )
            };
        }
        self.mark_rows(p.done, !p.current_name.is_empty());
    }

    fn describe_bytes(&mut self) -> String {
        if !self.indeterminate && self.bytes_done > self.bytes_total {
            self.bytes_total = self.bytes_done;
            self.estimate.bytes = self.bytes_done;
        }
        let done = format::byte_size(self.bytes_done);
        if self.indeterminate || self.bytes_total <= 0 {
            done
        } else {
            format!("{done} of {}", format::byte_size(self.bytes_total))
        }
    }

    fn mark_rows(&mut self, done: usize, working: bool) {
        if self.items.is_empty() {
            return;
        }
        let working_at = if working { done as i64 } else { -1 };
        let last = (done as i64)
            .max(working_at)
            .min(self.items.len() as i64 - 1);
        let mut i = self.marked_up_to as i64;
        while i <= last {
            let item = &mut self.items[i as usize];
            item.state = if (i as usize) < done {
                2
            } else if i == working_at {
                1
            } else {
                0
            };
            i += 1;
        }
        self.marked_up_to = done.min(self.items.len() - 1);
    }

    /// A tracker already part-way through, for a photograph.
    pub(crate) fn posed(
        mut self,
        items_done: usize,
        bytes_done: i64,
        bytes_per_second: f64,
    ) -> TransferTracker {
        self.bytes_done = bytes_done;
        self.bytes_text = self.describe_bytes();
        if !self.indeterminate && self.bytes_total > 0 {
            self.fraction = (bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0);
        }
        self.rate_text = format::speed(Some(bytes_per_second));
        self.eta_text = if bytes_per_second > 0.0 && !self.indeterminate {
            format::remaining(Some(Duration::from_secs_f64(
                ((self.bytes_total - bytes_done) as f64 / bytes_per_second).max(0.0),
            )))
        } else {
            String::new()
        };
        let current = self
            .items
            .get(items_done)
            .map(|i| i.name.clone())
            .unwrap_or_default();
        self.headline = format!(
            "{} {} of {} \u{2014} {current}",
            self.verbing,
            (items_done + 1).min(self.items_total),
            self.items_total
        );
        self.mark_rows(items_done, true);
        self
    }

    pub(crate) fn view(&self, paused: bool, cancelling: bool) -> Value {
        let headline = if cancelling {
            "Stopping\u{2026}".to_string()
        } else if paused {
            format!("Paused \u{2014} {}", self.headline)
        } else {
            self.headline.clone()
        };
        let (rate, eta) = if paused {
            (String::new(), String::new())
        } else {
            (self.rate_text.clone(), self.eta_text.clone())
        };
        let detail = [self.bytes_text.as_str(), rate.as_str(), eta.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join("  \u{b7}  ");
        json!({
            "headline": headline,
            "fraction": self.fraction,
            "indeterminate": self.indeterminate && !paused,
            "bytesText": self.bytes_text,
            "rateText": rate,
            "etaText": eta,
            "detail": detail,
            "items": self.items.iter().map(|i| json!({
                "name": i.name, "destination": i.destination, "state": i.state,
            })).collect::<Vec<_>>(),
        })
    }
}

/// A posed run: what the harness shows instead of a drain it cannot catch.
pub(crate) struct Pose {
    pub tracker: TransferTracker,
    pub records: Vec<JobRecord>,
}

// ---- the shared state -------------------------------------------------------------------------------

/// One question waiting on a person.
pub(crate) struct Waiting<T> {
    pub model: Value,
    pub answer: Option<T>,
}

pub(crate) struct OpsUi {
    /// The questions a window answers; shared with the seams that ask them.
    pub questions: std::sync::Arc<Questions>,
    /// Passwords given for archives this session.
    pub passwords: std::sync::Arc<crate::ops_arch::SessionPasswords>,
    /// The archive executors' own gate, kept in step with the app's.
    pub archive_gate: bb_core::archives::PauseGate,
    pub tracker: Mutex<Option<TransferTracker>>,
    pub records: Mutex<Vec<JobRecord>>,
    pub pose: Mutex<Option<Pose>>,
    pub cancelling: AtomicBool,
    pub running_cancel: Mutex<Option<(u64, CancelToken)>>,
    /// The last offer of administrator rights, for the dialog that explains it and for a script.
    pub elevation_offer: Mutex<Option<Value>>,
    pub last_emit: Mutex<Option<Instant>>,
}

impl OpsUi {
    pub(crate) fn new(
        questions: std::sync::Arc<Questions>,
        passwords: std::sync::Arc<crate::ops_arch::SessionPasswords>,
    ) -> OpsUi {
        OpsUi {
            questions,
            passwords,
            archive_gate: bb_core::archives::PauseGate::new(),
            tracker: Mutex::new(None),
            records: Mutex::new(Vec::new()),
            pose: Mutex::new(None),
            cancelling: AtomicBool::new(false),
            running_cancel: Mutex::new(None),
            elevation_offer: Mutex::new(None),
            last_emit: Mutex::new(None),
        }
    }

    /// Raises `Transfer` at most every 80 ms, so a progress report per megabyte is not an event per
    /// megabyte.
    pub(crate) fn throttled(&self) -> bool {
        let mut last = lock(&self.last_emit);
        let due = last.is_none_or(|t| t.elapsed() >= Duration::from_millis(80));
        if due {
            *last = Some(Instant::now());
        }
        due
    }
}

// ---- the queue ----------------------------------------------------------------------------------------

impl Inner {
    /// The sentence an operation ends on. The folders it touched have been told to reload, and a
    /// reload ends by writing "N items"; waiting for them keeps the sentence the last word.
    pub(crate) fn set_final_status(&self, text: String) {
        let start = Instant::now();
        while self.loading.load(Ordering::SeqCst) > 0 && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(2));
        }
        self.set_active_status(text);
    }

    pub(crate) fn transfer_changed(&self) {
        self.hub.model(Model::Transfer);
    }

    /// The queue and the run in flight, as the strip and the Transfers window draw them.
    pub(crate) fn queue_json(&self) -> Value {
        let paused = self.gate.is_paused();
        let cancelling = self.extra.ops.cancelling.load(Ordering::SeqCst);
        let pose = lock(&self.extra.ops.pose);
        let (view, records) = match pose.as_ref() {
            Some(p) => (Some(p.tracker.view(paused, false)), p.records.clone()),
            None => {
                let tracker = lock(&self.extra.ops.tracker);
                (
                    tracker.as_ref().map(|t| t.view(paused, cancelling)),
                    lock(&self.extra.ops.records).clone(),
                )
            }
        };
        let summaries: Vec<QueuedJobSummary> = records.iter().map(JobRecord::summary).collect();
        let running = records.iter().any(|r| r.state == QueuedJobState::Running);
        let posed = pose.is_some();
        let jobs: Vec<Value> = records
            .iter()
            .map(|r| {
                let waiting = r.state == QueuedJobState::Waiting;
                let size = if r.meta.estimate.complete && r.meta.estimate.bytes > 0 {
                    format::byte_size(r.meta.estimate.bytes)
                } else {
                    String::new()
                };
                json!({
                    "id": r.id,
                    "kind": r.meta.kind,
                    "description": r.meta.description,
                    "items": r.meta.items,
                    "sizeText": size,
                    "state": match r.state {
                        QueuedJobState::Waiting => "waiting",
                        QueuedJobState::Running => "running",
                        QueuedJobState::Done => "done",
                        QueuedJobState::Cancelled => "cancelled",
                    },
                    "stateText": match r.state {
                        QueuedJobState::Running => "in progress",
                        QueuedJobState::Done => "done",
                        QueuedJobState::Cancelled => "cancelled",
                        QueuedJobState::Waiting => "waiting",
                    },
                    "canUp": waiting && TransferQueueRules::can_move_up(&summaries, r.id as i32),
                    "canDown": waiting && TransferQueueRules::can_move_down(&summaries, r.id as i32),
                })
            })
            .collect();
        let active = view.is_some();
        let view = view.unwrap_or_else(|| json!({}));
        json!({
            "active": active,
            "posed": posed,
            "paused": paused,
            "cancelling": cancelling,
            "canPause": active && !cancelling && !paused && running,
            "canResume": paused,
            "canCancel": active && !cancelling && running,
            "waitingText": TransferQueueRules::waiting_text(&summaries),
            "view": view,
            "jobs": jobs,
        })
    }

    /// Adds a row for a job just accepted.
    pub(crate) fn queue_add(&self, id: u64, meta: &JobMeta) {
        if !meta.show {
            return;
        }
        lock(&self.extra.ops.records).push(JobRecord {
            id,
            meta: meta.clone(),
            state: QueuedJobState::Waiting,
        });
        self.transfer_changed();
    }

    pub(crate) fn queue_state_of(&self, id: u64, state: QueuedJobState) {
        let mut records = lock(&self.extra.ops.records);
        if let Some(r) = records.iter_mut().find(|r| r.id == id) {
            r.state = state;
        }
        drop(records);
        self.transfer_changed();
    }

    /// The queue emptied: nothing it listed is worth keeping.
    pub(crate) fn queue_clear(&self) {
        lock(&self.extra.ops.records).clear();
        *lock(&self.extra.ops.tracker) = None;
        self.extra.ops.cancelling.store(false, Ordering::SeqCst);
        self.transfer_changed();
    }

    /// Moves a waiting job among the waiting ones.
    pub(crate) fn queue_move(&self, id: u64, delta: i32) -> Result<(), String> {
        if let Some(p) = lock(&self.extra.ops.pose).as_mut() {
            let summaries: Vec<_> = p.records.iter().map(JobRecord::summary).collect();
            let ordered = TransferQueueRules::move_job(&summaries, id as i32, delta);
            let mut next = Vec::new();
            for s in &ordered {
                if let Some(r) = p.records.iter().find(|r| r.id == s.id as u64) {
                    next.push(r.clone());
                }
            }
            p.records = next;
            drop(summaries);
            self.transfer_changed();
            return Ok(());
        }
        let mut queue = lock(&self.queue.state);
        let mut records = lock(&self.extra.ops.records);
        let summaries: Vec<_> = records.iter().map(JobRecord::summary).collect();
        let ordered = TransferQueueRules::move_job(&summaries, id as i32, delta);
        let order: Vec<u64> = ordered.iter().map(|s| s.id as u64).collect();
        let mut next = Vec::new();
        for want in &order {
            if let Some(r) = records.iter().find(|r| r.id == *want) {
                next.push(r.clone());
            }
        }
        *records = next;
        // The waiting jobs follow the order their records now have.
        let mut jobs: Vec<_> = queue.jobs.drain(..).collect();
        jobs.sort_by_key(|j| {
            order
                .iter()
                .position(|o| *o == j.id.0)
                .unwrap_or(usize::MAX)
        });
        queue.jobs = jobs.into();
        drop(records);
        drop(queue);
        self.transfer_changed();
        Ok(())
    }

    /// Cancels a job: one still waiting is taken out of the queue; the running one is told to stop.
    pub(crate) fn queue_cancel(&self, id: u64) -> Result<(), String> {
        if let Some(p) = lock(&self.extra.ops.pose).as_mut() {
            if let Some(r) = p.records.iter_mut().find(|r| r.id == id) {
                match r.state {
                    QueuedJobState::Waiting => r.state = QueuedJobState::Cancelled,
                    QueuedJobState::Running => {
                        self.extra.ops.cancelling.store(true, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }
            self.transfer_changed();
            return Ok(());
        }
        let state = lock(&self.extra.ops.records)
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.state);
        match state {
            Some(QueuedJobState::Waiting) => {
                {
                    let queue = lock(&self.queue.state);
                    if let Some(job) = queue.jobs.iter().find(|j| j.id.0 == id) {
                        job.cancel.cancel();
                    }
                }
                self.queue_state_of(id, QueuedJobState::Cancelled);
                Ok(())
            }
            Some(QueuedJobState::Running) => {
                self.cancel_running();
                Ok(())
            }
            _ => Err("that job is not in the queue".to_string()),
        }
    }

    /// Stops the job in flight: its token is cancelled and a pause is lifted so it can notice.
    pub(crate) fn cancel_running(&self) {
        let token = lock(&self.extra.ops.running_cancel).clone();
        if let Some((_, token)) = token {
            self.extra.ops.cancelling.store(true, Ordering::SeqCst);
            token.cancel();
            self.gate.resume();
            self.update_busy();
            self.transfer_changed();
        }
    }

    /// The tracker of the running job reports; the strip hears it, rarely.
    pub(crate) fn tracker_apply(&self, p: &TransferProgress) {
        let paused = self.gate.is_paused();
        if let Some(t) = lock(&self.extra.ops.tracker).as_mut() {
            t.apply(p, paused);
        }
        if self.extra.ops.throttled() || p.current_name.is_empty() {
            self.transfer_changed();
        }
    }

    pub(crate) fn tracker_begin(&self, tracker: TransferTracker) {
        *lock(&self.extra.ops.tracker) = Some(tracker);
        self.extra.ops.cancelling.store(false, Ordering::SeqCst);
        self.transfer_changed();
    }

    pub(crate) fn tracker_end(&self) {
        *lock(&self.extra.ops.tracker) = None;
        self.transfer_changed();
    }

    /// Poses a run for a photograph: `sized`, `unsized` or `queued`; `off` takes it away.
    pub(crate) fn pose_transfer(&self, kind: &str, destination: &str) {
        if kind == "off" {
            *lock(&self.extra.ops.pose) = None;
            self.gate.resume();
            self.update_busy();
            self.transfer_changed();
            return;
        }
        const TOTAL: i64 = 50 * 1024 * 1024 * 1024;
        let sized = kind != "unsized";
        let estimate = if sized {
            TransferEstimate::new(TOTAL, 3, true)
        } else {
            TransferEstimate::new(0, 3, false)
        };
        let dest = destination.to_string();
        let tracker = TransferTracker::new(
            "Copying",
            vec![
                ("archive.zip".to_string(), dest.clone()),
                ("photo.jpg".to_string(), dest.clone()),
                ("Pictures".to_string(), dest),
            ],
            estimate,
        )
        .posed(1, 4_509_715_660, 117_440_512.0);
        let mut records = vec![JobRecord {
            id: 1,
            meta: JobMeta::shown("Copy", "3 item(s) to Archive".to_string(), 3, estimate),
            state: QueuedJobState::Running,
        }];
        if kind == "queued" {
            let rows = [
                (
                    "Move",
                    "128 item(s) to Backup",
                    128,
                    TransferEstimate::new(9_663_676_416, 128, true),
                ),
                (
                    "Extract",
                    "41 item(s) to Photos",
                    41,
                    TransferEstimate::new(214_748_364, 41, true),
                ),
                (
                    "Compress",
                    "site-backup.zip",
                    6,
                    TransferEstimate::new(0, 0, false),
                ),
            ];
            for (n, (k, d, items, est)) in rows.into_iter().enumerate() {
                records.push(JobRecord {
                    id: 2 + n as u64,
                    meta: JobMeta::shown(k, d.to_string(), items, est),
                    state: QueuedJobState::Waiting,
                });
            }
        }
        *lock(&self.extra.ops.pose) = Some(Pose { tracker, records });
        self.transfer_changed();
    }
}

// ---- a clash, as the dialog asks it -----------------------------------------------------------------

fn describe_side(side: &ClashSide) -> String {
    let modified = if side.modified_utc.is_unknown() {
        None
    } else {
        Some(bb_core::time::format_short_local(side.modified_utc))
    };
    if side.is_directory {
        match modified {
            Some(m) => format!("folder \u{2014} modified {m}"),
            None => "folder".to_string(),
        }
    } else {
        let size = format::byte_size(side.bytes);
        match modified {
            Some(m) => format!("{size} \u{2014} modified {m}"),
            None => size,
        }
    }
}

fn describe_on_disk(path: &str, is_directory: bool) -> (String, i64) {
    let Ok(meta) = std::fs::metadata(path) else {
        return ("missing".to_string(), 0);
    };
    let modified = meta
        .modified()
        .ok()
        .map(bb_core::FileTime::from_system_time);
    let when = modified
        .map(|m| {
            format!(
                " \u{2014} modified {}",
                bb_core::time::format_short_local(m)
            )
        })
        .unwrap_or_default();
    let ticks = modified.map_or(0, |m| m.to_parts().1 as i64);
    if is_directory {
        (format!("folder{when}"), ticks)
    } else {
        (
            format!("{}{when}", format::byte_size(meta.len() as i64)),
            ticks,
        )
    }
}

pub(crate) fn resolution_word(r: ConflictResolution) -> &'static str {
    match r {
        ConflictResolution::Skip => "skip",
        ConflictResolution::KeepBoth => "keep-both",
        ConflictResolution::Replace => "replace",
        ConflictResolution::Overwrite => "overwrite",
    }
}

pub(crate) fn parse_resolution(word: &str) -> Option<ConflictResolution> {
    match word.to_ascii_lowercase().as_str() {
        "skip" => Some(ConflictResolution::Skip),
        "replace" => Some(ConflictResolution::Replace),
        "keep-both" | "keepboth" => Some(ConflictResolution::KeepBoth),
        "overwrite" => Some(ConflictResolution::Overwrite),
        _ => None,
    }
}

/// The key the harness and the dialog name a row by: the label with `/` as `\`.
pub(crate) fn label_key(label: &str) -> String {
    label.replace('/', "\\").to_uppercase()
}

/// Everything the conflicts dialog shows for a plan, and what each row starts on.
pub(crate) fn conflict_model(plan: &TransferPlan, undo_gesture: &str) -> Value {
    let folder = {
        let name = file_name(plan.destination_directory.trim_end_matches(['\\', '/']));
        if name.is_empty() {
            plan.destination_directory.clone()
        } else {
            name.to_string()
        }
    };
    let verbing = if plan.verb == TransferVerb::Move {
        "moving"
    } else {
        "copying"
    };
    let conflicts = plan.conflicts();
    let rows: Vec<Value> = conflicts
        .iter()
        .map(|t| {
            let label = plan.label_for(t);
            let merge_note = if t.merge_declined {
                format!(
                    "Too large to merge item by item (over {} entries), so this answer applies to the whole folder.",
                    format::thousands(bb_core::transfer::TransferMergeLimits::MAX_EXPANDED_TRANSFERS as i64)
                )
            } else {
                String::new()
            };
            let (incoming, existing_label, existing, identical, newer);
            if let Some(clash) = t.clash {
                incoming = describe_side(&clash.incoming);
                identical = clash.identical;
                newer = clash.incoming_is_newer;
                if let Some(side) = clash.existing {
                    existing_label = "Already here:".to_string();
                    existing = if identical {
                        "identical \u{2014} same size and date".to_string()
                    } else {
                        describe_side(&side)
                    };
                } else if let Some(earlier) = plan.earlier_claimant_of(t) {
                    existing_label = "Also arriving:".to_string();
                    existing = format!(
                        "{} from {}",
                        earlier.name(),
                        bb_core::rename::paths::directory_name(&earlier.source_path)
                            .unwrap_or_default()
                    );
                } else {
                    existing_label = "Already here:".to_string();
                    existing = "missing".to_string();
                }
            } else {
                let (inc, inc_time) = describe_on_disk(&t.source_path, t.is_directory);
                incoming = inc;
                identical = false;
                let on_disk = std::path::Path::new(&t.destination_path).exists();
                if on_disk {
                    let (ex, ex_time) = describe_on_disk(
                        &t.destination_path,
                        std::path::Path::new(&t.destination_path).is_dir(),
                    );
                    existing_label = "Already here:".to_string();
                    existing = ex;
                    newer = inc_time > ex_time;
                } else if let Some(earlier) = plan.earlier_claimant_of(t) {
                    existing_label = "Also arriving:".to_string();
                    existing = format!(
                        "{} from {}",
                        earlier.name(),
                        bb_core::rename::paths::directory_name(&earlier.source_path)
                            .unwrap_or_default()
                    );
                    newer = false;
                } else {
                    existing_label = "Already here:".to_string();
                    existing = "missing".to_string();
                    newer = false;
                }
            }
            json!({
                "name": label,
                "key": label_key(&label),
                "groupKey": PathKey::canonicalize(&t.source_path).unwrap_or_else(|_| t.source_path.clone()),
                "sourcePath": t.source_path,
                "incoming": incoming,
                "existingLabel": existing_label,
                "existing": existing,
                "identical": identical,
                "incomingIsNewer": newer,
                "mergeNote": merge_note,
                "allowReplace": ConflictDefaults::allows_replace(plan, t),
                "default": resolution_word(ConflictDefaults::for_transfer(t)),
            })
        })
        .collect();
    let identical = rows
        .iter()
        .filter(|r| r["identical"] == json!(true))
        .count();
    let n = rows.len();
    let (title, summary, mut subtitle);
    if plan.is_merged() {
        let merged = if plan.merged_folders.len() == 1 {
            format!("'{}'", plan.merged_folders[0])
        } else {
            format!(
                "{} folders",
                format::thousands(plan.merged_folders.len() as i64)
            )
        };
        title = if plan.merged_folders.len() == 1 {
            format!("Merging {} into {folder}", plan.merged_folders[0])
        } else {
            format!(
                "Merging {} folders into {folder}",
                format::thousands(plan.merged_folders.len() as i64)
            )
        };
        summary = if n == 1 {
            format!("{merged} already exists in {folder}. One item inside already exists too.")
        } else {
            format!(
                "{merged} already exists in {folder}. {} of the items inside already exist too.",
                format::thousands(n as i64)
            )
        };
        subtitle = format!(
            "Choose what to do with each. Everything else is {} across without asking.",
            if plan.verb == TransferVerb::Move {
                "moved"
            } else {
                "copied"
            }
        );
    } else {
        title = if n == 1 {
            "An item already exists".to_string()
        } else {
            format!("{} items already exist", format::thousands(n as i64))
        };
        summary = if n == 1 {
            format!(
                "'{}' already exists in {folder}.",
                rows[0]["name"].as_str().unwrap_or_default()
            )
        } else {
            format!(
                "{} of the items you are {verbing} already exist in {folder}.",
                format::thousands(n as i64)
            )
        };
        subtitle = "Choose what to do with each of them.".to_string();
    }
    if identical > 0 {
        subtitle.push_str(&if identical == 1 {
            " One is the same file on both sides and is set to skip.".to_string()
        } else {
            format!(
                " {} are the same file on both sides and are set to skip.",
                format::thousands(identical as i64)
            )
        });
    }
    json!({
        "title": title,
        "summary": summary,
        "subtitle": subtitle,
        "replaceHint": format!(
            "Replace keeps the incoming copy. What it displaces is set aside, and {undo_gesture} restores both sides until you do something else."
        ),
        "allowReplace": rows.iter().any(|r| r["allowReplace"] == json!(true)),
        "replaceMeans": resolution_word(ConflictDefaults::replace_means(plan)),
        "verb": if plan.verb == TransferVerb::Move { "move" } else { "copy" },
        "merged": plan.is_merged(),
        "rows": rows,
    })
}

// ---- the questions of the product ---------------------------------------------------------------------

/// The pending questions of a window that can ask: a transfer thread parks here until the host
/// answers through `ops_do`.
#[derive(Default)]
pub struct Questions {
    conflict: Mutex<Option<Waiting<Option<Resolutions>>>>,
    elevation: Mutex<Option<Waiting<bool>>>,
    confirm: Mutex<Option<Waiting<bool>>>,
    /// Things the user has to be told, oldest first; each stays until the host dismisses it.
    notices: Mutex<std::collections::VecDeque<Value>>,
    cv: Condvar,
}

impl Questions {
    /// A yes/no question from a worker thread (a flat view of a huge folder, an update that is
    /// ready): parks it until the host answers, `false` if the question is withdrawn.
    pub(crate) fn ask_confirm(
        &self,
        hub: &crate::events::EventHub,
        message: &str,
        title: &str,
        confirm_label: &str,
    ) -> bool {
        *lock(&self.confirm) = Some(Waiting {
            model: json!({ "message": message, "title": title, "confirm": confirm_label }),
            answer: None,
        });
        hub.model(Model::Confirm);
        let mut slot = lock(&self.confirm);
        loop {
            if let Some(w) = slot.as_mut()
                && let Some(answer) = w.answer.take()
            {
                *slot = None;
                return answer;
            }
            if slot.is_none() {
                return false;
            }
            slot = self.cv.wait(slot).unwrap_or_else(|p| p.into_inner());
        }
    }

    pub(crate) fn confirm_model(&self) -> Option<Value> {
        lock(&self.confirm).as_ref().map(|w| w.model.clone())
    }

    pub(crate) fn answer_confirm(&self, yes: bool) {
        if let Some(w) = lock(&self.confirm).as_mut() {
            w.answer = Some(yes);
        }
        self.cv.notify_all();
    }

    /// Something to tell the user; never waits.
    pub(crate) fn tell(&self, hub: &crate::events::EventHub, message: &str, title: &str) {
        lock(&self.notices).push_back(json!({ "message": message, "title": title }));
        hub.model(Model::Notice);
    }

    pub(crate) fn notice_model(&self) -> Option<Value> {
        lock(&self.notices).front().cloned()
    }

    /// The oldest notice has been read; the next, if any, is raised.
    pub(crate) fn dismiss_notice(&self, hub: &crate::events::EventHub) {
        let more = {
            let mut q = lock(&self.notices);
            q.pop_front();
            !q.is_empty()
        };
        if more {
            hub.model(Model::Notice);
        }
    }

    pub(crate) fn ask_conflicts(
        &self,
        hub: &crate::events::EventHub,
        plan: &TransferPlan,
    ) -> Option<Resolutions> {
        let model = conflict_model(plan, "Ctrl+Z");
        {
            let mut slot = lock(&self.conflict);
            *slot = Some(Waiting {
                model,
                answer: None,
            });
        }
        hub.model(Model::Conflicts);
        let mut slot = lock(&self.conflict);
        loop {
            if let Some(w) = slot.as_mut()
                && let Some(answer) = w.answer.take()
            {
                *slot = None;
                return answer;
            }
            if slot.is_none() {
                return None;
            }
            slot = self.cv.wait(slot).unwrap_or_else(|p| p.into_inner());
        }
    }

    pub(crate) fn conflict_model(&self) -> Option<Value> {
        lock(&self.conflict).as_ref().map(|w| w.model.clone())
    }

    /// The host's answer: `None` cancels the transfer.
    pub(crate) fn answer_conflicts(&self, answer: Option<Resolutions>) {
        if let Some(w) = lock(&self.conflict).as_mut() {
            w.answer = Some(answer);
        }
        self.cv.notify_all();
    }

    pub(crate) fn ask_elevation(&self, hub: &crate::events::EventHub, model: Value) -> bool {
        *lock(&self.elevation) = Some(Waiting {
            model,
            answer: None,
        });
        hub.model(Model::Elevation);
        let mut slot = lock(&self.elevation);
        loop {
            if let Some(w) = slot.as_mut()
                && let Some(answer) = w.answer.take()
            {
                *slot = None;
                return answer;
            }
            if slot.is_none() {
                return false;
            }
            slot = self.cv.wait(slot).unwrap_or_else(|p| p.into_inner());
        }
    }

    pub(crate) fn elevation_model(&self) -> Option<Value> {
        lock(&self.elevation).as_ref().map(|w| w.model.clone())
    }

    pub(crate) fn answer_elevation(&self, yes: bool) {
        if let Some(w) = lock(&self.elevation).as_mut() {
            w.answer = Some(yes);
        }
        self.cv.notify_all();
    }
}
