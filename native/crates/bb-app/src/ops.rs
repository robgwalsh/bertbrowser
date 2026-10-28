//! Move, copy, delete, rename and undo, and the queue that runs them. Spec: ShellViewModel
//! (EnqueueAsync, DrainQueueAsync, RunDropAsync, DeleteAsync, RenameAsync, RecordAsync, WalkAsync).
//!
//! One thread, `bb-transfer`, drains the queue. `draining` is "this app is writing", and it is set
//! when a job is *accepted* and cleared only when the queue is empty, so releasing it between two
//! jobs cannot let a rename slip into the gap. A paused run is still draining; the settle predicate
//! alone treats it as quiet. Rename, delete and undo are refused while anything is draining (they
//! share the flag so Ctrl+Z cannot reach the previous record while one of them is running); a drop
//! or paste queues behind the running job.

use std::sync::{Arc, LazyLock, Weak};

use bb_core::FileTime;
use bb_core::cancel::CancelToken;
use bb_core::delete::{
    DeleteExecutor, DeleteMode, DeletePlanner, DeleteSource, FileSystemDeleteProbe, ShellRecycleBin,
};
use bb_core::rename::{RenameExecutor, RenamePlanner, RenameSource};
use bb_core::transfer::{
    FileSystemTransferProbe, TransferExecutor, TransferMergeExpander, TransferPlanner,
    TransferProgress, TransferVerb,
};
use bb_core::undo::{
    DeleteRecord, HeldFolderSizes, HeldSizeEstimator, PlainUndoHost, RenameRecord, TransferRecord,
    UndoDirection, UndoNavigator, UndoStack, UndoableRecord,
};

use crate::app::{Inner, Job, JobBody, OpState, UndoSummary, lock};
use crate::events::AppEvent;
use crate::ids::{AppError, AppResult, OpId};

static RECYCLE_BIN: LazyLock<ShellRecycleBin> = LazyLock::new(ShellRecycleBin::new);
static DELETE_PROBE: FileSystemDeleteProbe = FileSystemDeleteProbe;

/// What `dir_size_cache` already knows about a folder, for sizing what an undo record holds.
struct CachedSizes<'a>(&'a bb_core::data::DirSizeRepository);

impl HeldFolderSizes for CachedSizes<'_> {
    fn of(&self, original_path: &str) -> Option<i64> {
        match self.0.get(original_path) {
            Ok(Some(row)) if !row.incomplete => Some(row.size_bytes),
            _ => None,
        }
    }
}

fn count_text(n: usize) -> String {
    bb_core::format::thousands(n as i64)
}

impl Inner {
    pub(crate) fn start_transfer_thread(self: &Arc<Self>) {
        let weak: Weak<Inner> = Arc::downgrade(self);
        let queue = self.queue.clone();
        let spawned = std::thread::Builder::new()
            .name("bb-transfer".into())
            .spawn(move || {
                loop {
                    let job = {
                        let mut state = lock(&queue.state);
                        loop {
                            if state.shutdown {
                                return;
                            }
                            if let Some(job) = state.jobs.pop_front() {
                                state.running = Some(job.id);
                                break job;
                            }
                            // Empty and idle: this is the only place `draining` is released.
                            if state.draining {
                                state.draining = false;
                                drop(state);
                                if let Some(inner) = weak.upgrade() {
                                    inner.gate.resume();
                                    inner.update_busy();
                                    inner.hub.emit(AppEvent::UndoChanged);
                                }
                                state = lock(&queue.state);
                                continue;
                            }
                            state = queue.cv.wait(state).unwrap_or_else(|p| p.into_inner());
                        }
                    };
                    let Some(inner) = weak.upgrade() else { return };
                    inner.run_job(job);
                }
            });
        if let Ok(handle) = spawned {
            *lock(&self.transfer_thread) = Some(handle);
        }
    }

    fn run_job(self: &Arc<Self>, job: Job) {
        let id = job.id;
        lock(&self.ops).insert(id, OpState::Running);
        self.update_busy();
        let body = job.body;
        let cancel = job.cancel;
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(self, id, &cancel)));
        let (ok, message) = match result {
            Ok(done) => done,
            Err(_) => (false, "the operation failed unexpectedly".to_string()),
        };
        *lock(&self.transfer_progress) = None;
        lock(&self.queue.state).running = None;
        lock(&self.ops).insert(id, OpState::Done { ok, message });
        self.hub.emit(AppEvent::OpFinished { op: id, ok });
    }

    /// Accepts a job. `exclusive` jobs (rename, delete, undo) are refused while anything is draining.
    pub(crate) fn submit(
        self: &Arc<Self>,
        kind: &'static str,
        exclusive: bool,
        body: JobBody,
    ) -> AppResult<OpId> {
        let id = OpId(self.new_id());
        let mut state = lock(&self.queue.state);
        if state.shutdown {
            return Err(AppError::Invalid("the app is shutting down".into()));
        }
        if exclusive && state.draining {
            drop(state);
            self.set_active_status(format!(
                "Busy \u{2014} try {} again once the current operation finishes.",
                kind.to_lowercase()
            ));
            return Err(AppError::Busy);
        }
        state.draining = true;
        state.jobs.push_back(Job {
            id,
            cancel: CancelToken::new(),
            body,
        });
        lock(&self.ops).insert(id, OpState::Queued);
        drop(state);
        self.queue.cv.notify_one();
        self.update_busy();
        Ok(id)
    }

    pub(crate) fn cancel_op(&self, op: OpId) -> AppResult<()> {
        let state = lock(&self.queue.state);
        for job in &state.jobs {
            if job.id == op {
                job.cancel.cancel();
                return Ok(());
            }
        }
        if state.running == Some(op) {
            // The running job's token is held by the job; cancelling the gate wakes a paused one.
            drop(state);
            self.gate.resume();
            return Err(AppError::Invalid(
                "a running operation cannot be cancelled from here yet".into(),
            ));
        }
        Err(AppError::Stale)
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        if paused {
            self.gate.pause();
        } else {
            self.gate.resume();
        }
        self.update_busy();
    }

    // ---- recording ----

    pub(crate) fn record(&self, record: Option<Box<dyn UndoableRecord>>) {
        let Some(record) = record else { return };
        let sizes = CachedSizes(&self.dir_sizes);
        let held = HeldSizeEstimator::new(Some(&sizes)).measure_record(record.as_ref());
        let released = {
            let mut undo = lock(&self.undo);
            let released = undo.push(record, held, FileTime::now());
            self.refresh_undo_summary(&undo);
            released
        };
        if let Ok((_, released)) = released {
            released.retire_all();
        }
        self.hub.emit(AppEvent::UndoChanged);
    }

    pub(crate) fn refresh_undo_summary(&self, undo: &UndoStack) {
        *lock(&self.undo_summary) = UndoSummary {
            undo: undo
                .next_undo()
                .map(|e| e.record().description().to_string()),
            redo: undo
                .next_redo()
                .map(|e| e.record().description().to_string()),
            entries: undo.entries().len(),
        };
    }

    /// Commits whatever the history still holds. Called when the window closes.
    pub(crate) fn release_undo_history(&self) {
        let released = {
            let mut undo = lock(&self.undo);
            let released = undo.clear();
            self.refresh_undo_summary(&undo);
            released
        };
        released.retire_all();
    }

    // ---- move and copy ----

    pub(crate) fn op_transfer(
        self: &Arc<Self>,
        sources: Vec<String>,
        destination: String,
        verb: TransferVerb,
    ) -> AppResult<OpId> {
        if sources.is_empty() {
            return Err(AppError::Invalid("nothing to transfer".into()));
        }
        let kind = if verb == TransferVerb::Move {
            "Move"
        } else {
            "Copy"
        };
        self.submit(
            kind,
            false,
            Box::new(move |inner, _op, cancel| {
                inner.run_transfer(&sources, &destination, verb, cancel)
            }),
        )
    }

    fn run_transfer(
        self: &Arc<Self>,
        sources: &[String],
        destination: &str,
        verb: TransferVerb,
        cancel: &CancelToken,
    ) -> (bool, String) {
        let word = if verb == TransferVerb::Move {
            "move"
        } else {
            "copy"
        };
        let planner = TransferPlanner::default();
        let mut plan = planner.plan(sources, destination, verb);

        if !plan.has_work() {
            let why = plan
                .rejected
                .iter()
                .find(|r| !r.is_benign())
                .map(|r| format!(" {}", r.message))
                .unwrap_or_default();
            let text = format!("Nothing to {word}.{why}");
            self.set_active_status(text.clone());
            return (false, text);
        }

        // A folder landing on a folder of the same name is merged, so the question becomes the items
        // inside it that really clash. Here, off the planner's hot path, because it opens directories.
        let mut resolutions = None;
        if !plan.conflicts().is_empty() {
            let into = bb_paths::file_name(destination.trim_end_matches(['\\', '/']));
            self.set_active_status(format!("Checking what is already in {into}\u{2026}"));
            let expander = TransferMergeExpander::new(Box::new(FileSystemTransferProbe));
            match expander.expand(plan, cancel) {
                Ok(expanded) => plan = expanded,
                Err(_) => return (false, "cancelled".to_string()),
            }
            if !plan.has_work() {
                let text = format!("Nothing to {word}.");
                self.set_active_status(text.clone());
                return (false, text);
            }
            if !plan.conflicts().is_empty() {
                resolutions = self.seams.conflicts.ask(&plan);
                if resolutions.is_none() {
                    self.set_active_status("Cancelled \u{2014} nothing was moved.");
                    return (false, "cancelled".to_string());
                }
            }
        }

        let total = plan.transfers.len();
        let headline = format!(
            "{} {} item(s)",
            if verb == TransferVerb::Move {
                "Moving"
            } else {
                "Copying"
            },
            count_text(total)
        );
        self.set_active_status(headline);
        let me = self.clone();
        let mut on_progress = move |p: TransferProgress| {
            if !p.current_name.is_empty() {
                me.set_active_status(format!(
                    "{} {} of {} \u{2014} {}",
                    if verb == TransferVerb::Move {
                        "Moving"
                    } else {
                        "Copying"
                    },
                    p.done + 1,
                    p.total,
                    p.current_name
                ));
            }
            *lock(&me.transfer_progress) = Some(p);
        };
        let executor = TransferExecutor::default();
        let mut outcome = executor.execute(
            &plan,
            resolutions.as_ref(),
            cancel,
            Some(&mut on_progress),
            Some(&self.gate),
        );

        // A merged paste may have replaced files by staging what it displaced; that only stays true
        // while somebody holds the record, which is the undertaking `Overwrite` was written under.
        if plan.is_merged() && plan.verb == TransferVerb::Copy && !outcome.completed.is_empty() {
            outcome = TransferExecutor::stamp_copies(outcome);
            outcome.can_undo_copy = true;
        }

        self.record(TransferRecord::for_outcome(outcome.clone()).map(|r| Box::new(r) as _));

        let mut directories: Vec<String> = outcome
            .completed
            .iter()
            .filter_map(|c| bb_core::rename::paths::directory_name(&c.source_path))
            .chain(
                outcome
                    .pruned_directories
                    .iter()
                    .filter_map(|p| bb_core::rename::paths::directory_name(p)),
            )
            .collect();
        directories.push(plan.destination_directory.clone());
        directories.sort_by_key(|d| d.to_uppercase());
        directories.dedup_by_key(|d| d.to_uppercase());
        self.refresh_tabs_showing(&directories);

        let past = if verb == TransferVerb::Move {
            "Moved"
        } else {
            "Copied"
        };
        let mut text = if outcome.cancelled {
            format!(
                "{} cancelled \u{2014} {} of {} item(s) {}",
                if verb == TransferVerb::Move {
                    "Move"
                } else {
                    "Copy"
                },
                outcome.completed.len(),
                total,
                past.to_lowercase()
            )
        } else {
            format!("{past} {} item(s)", outcome.completed.len())
        };
        if !outcome.skipped.is_empty() {
            text.push_str(&format!(", skipped {}", outcome.skipped.len()));
        }
        if let Some(first) = outcome.failed.first() {
            text.push_str(&format!(
                ", {} failed \u{2014} {}",
                outcome.failed.len(),
                first.message
            ));
        } else if outcome.can_undo() || outcome.can_undo_copy {
            text.push_str(" \u{2014} this can be undone");
        }
        self.set_active_status(text.clone());
        (outcome.failed.is_empty() && !outcome.cancelled, text)
    }

    // ---- delete ----

    pub(crate) fn op_delete(
        self: &Arc<Self>,
        paths: Vec<String>,
        mode: DeleteMode,
    ) -> AppResult<OpId> {
        if paths.is_empty() {
            return Err(AppError::Invalid("nothing to delete".into()));
        }
        self.submit(
            "Delete",
            true,
            Box::new(move |inner, _op, cancel| inner.run_delete(&paths, mode, cancel)),
        )
    }

    fn delete_executor(&self) -> DeleteExecutor<'static> {
        let mut executor = DeleteExecutor::new(&DELETE_PROBE)
            .recycle_bin(&*RECYCLE_BIN)
            .recycle_probe(&*RECYCLE_BIN);
        if let Some(root) = &self.config.delete_staging_root {
            executor = executor.staging_root(root);
        }
        executor
    }

    fn run_delete(
        self: &Arc<Self>,
        paths: &[String],
        mode: DeleteMode,
        cancel: &CancelToken,
    ) -> (bool, String) {
        let sources: Vec<DeleteSource> = paths
            .iter()
            .map(|p| DeleteSource::new(p, std::path::Path::new(p).is_dir()))
            .collect();
        let plan = DeletePlanner::new(&DELETE_PROBE)
            .recycle_probe(&*RECYCLE_BIN)
            .plan(&sources, mode);
        if !plan.has_work() {
            let why = plan
                .rejected
                .iter()
                .find(|r| !r.is_benign())
                .map(|r| format!(" {}", r.message))
                .unwrap_or_default();
            let text = format!("Nothing to delete.{why}");
            self.set_active_status(text.clone());
            return (false, text);
        }

        self.set_active_status(format!(
            "Deleting {} item(s)\u{2026}",
            count_text(plan.deletions.len())
        ));
        let me = self.clone();
        let on_progress = move |p: bb_core::delete::DeleteProgress| {
            if !p.current_name.is_empty() {
                me.set_active_status(format!(
                    "Deleting {} of {} \u{2014} {}",
                    p.done + 1,
                    p.total,
                    p.current_name
                ));
            }
        };
        let outcome = self
            .delete_executor()
            .execute(&plan, cancel, Some(&on_progress));

        self.record(DeleteRecord::for_outcome(outcome.clone(), None).map(|r| Box::new(r) as _));

        // Vacate first: a tab still pointing inside a folder that has gone would otherwise be
        // reloaded onto a missing path on its way out of it.
        let gone: Vec<String> = outcome
            .deleted
            .iter()
            .filter(|d| d.is_directory)
            .map(|d| d.source_path.clone())
            .collect();
        self.leave_gone_folders(&gone);
        let directories: Vec<String> = plan.deletions.iter().map(|d| d.parent_path()).collect();
        self.refresh_tabs_showing(&directories);

        let mut text = format!("Deleted {} item(s)", outcome.deleted.len());
        if let Some(first) = outcome.failed.first() {
            text.push_str(&format!(
                "; {} failed \u{2014} {}",
                outcome.failed.len(),
                first.message
            ));
        } else if outcome.can_undo() {
            text.push_str(" \u{2014} this can be undone");
        }
        self.set_active_status(text.clone());
        (outcome.failed.is_empty(), text)
    }

    // ---- rename ----

    pub(crate) fn op_rename(
        self: &Arc<Self>,
        paths: Vec<String>,
        pattern: String,
    ) -> AppResult<OpId> {
        if paths.is_empty() {
            return Err(AppError::Invalid("nothing to rename".into()));
        }
        self.submit(
            "Rename",
            true,
            Box::new(move |inner, _op, _cancel| inner.run_rename(&paths, &pattern)),
        )
    }

    fn run_rename(self: &Arc<Self>, paths: &[String], pattern: &str) -> (bool, String) {
        let sources: Vec<RenameSource> = paths
            .iter()
            .map(|p| {
                if std::path::Path::new(p).is_dir() {
                    RenameSource::dir(p)
                } else {
                    RenameSource::file(p)
                }
            })
            .collect();
        let plan = RenamePlanner::local().plan_simple(&sources, pattern);
        if !plan.has_work() {
            let why = plan
                .rejected
                .first()
                .map(|r| format!(" {}", r.message))
                .unwrap_or_default();
            let text = format!("Nothing to rename.{why}");
            self.set_active_status(text.clone());
            return (false, text);
        }
        self.set_active_status(format!(
            "Renaming {} item(s)\u{2026}",
            count_text(plan.work().len())
        ));
        let outcome = RenameExecutor.execute(&plan);

        self.record(RenameRecord::for_outcome(outcome.clone()).map(|r| Box::new(r) as _));

        let mut directories: Vec<String> = outcome
            .completed
            .iter()
            .filter_map(|c| bb_core::rename::paths::directory_name(&c.source_path))
            .collect();
        directories.sort_by_key(|d| d.to_uppercase());
        directories.dedup_by_key(|d| d.to_uppercase());
        self.refresh_tabs_showing(&directories);

        let mut text = format!("Renamed {} item(s)", outcome.completed.len());
        if let Some(first) = outcome.failed.first() {
            text.push_str(&format!(
                "; {} failed \u{2014} {}",
                outcome.failed.len(),
                first.message
            ));
        } else if outcome.can_undo() {
            text.push_str(" \u{2014} this can be undone");
        }
        self.set_active_status(text.clone());
        (outcome.failed.is_empty(), text)
    }

    // ---- undo and redo ----

    /// `steps` > 0 undoes that many entries, < 0 redoes. Refused while anything is draining.
    pub(crate) fn op_undo_step(self: &Arc<Self>, steps: i32) -> AppResult<OpId> {
        if steps == 0 {
            return Err(AppError::Invalid("zero steps".into()));
        }
        {
            let undo = lock(&self.undo);
            let have = if steps > 0 {
                undo.cursor()
            } else {
                undo.entries().len() - undo.cursor()
            };
            if have == 0 {
                return Err(AppError::Invalid(if steps > 0 {
                    "nothing to undo".into()
                } else {
                    "nothing to redo".into()
                }));
            }
        }
        self.submit(
            if steps > 0 { "Undo" } else { "Redo" },
            true,
            Box::new(move |inner, _op, _cancel| inner.run_undo(steps)),
        )
    }

    fn run_undo(self: &Arc<Self>, steps: i32) -> (bool, String) {
        let direction = if steps > 0 {
            UndoDirection::Undo
        } else {
            UndoDirection::Redo
        };
        let count = steps.unsigned_abs() as usize;
        self.set_active_status(if steps > 0 {
            "Undoing\u{2026}"
        } else {
            "Redoing\u{2026}"
        });

        let transfers = TransferExecutor::default();
        let deletes = self.delete_executor();
        let renames = RenameExecutor;
        let host = PlainUndoHost::without_archives(&transfers, &deletes, &renames);
        let sizes = CachedSizes(&self.dir_sizes);
        let estimator = HeldSizeEstimator::new(Some(&sizes));
        let measure = |r: &dyn UndoableRecord| estimator.measure_record(r);

        let (span, result) = {
            let mut undo = lock(&self.undo);
            let span = {
                let entries = undo.entries();
                let cursor = undo.cursor();
                let target = if steps > 0 {
                    entries.get(cursor.saturating_sub(count)).map(|e| e.id())
                } else {
                    entries
                        .get((cursor + count - 1).min(entries.len().saturating_sub(1)))
                        .map(|e| e.id())
                };
                match target {
                    Some(target) => {
                        if steps > 0 {
                            undo.undo_span(target)
                        } else {
                            undo.redo_span(target)
                        }
                    }
                    None => Ok(Vec::new()),
                }
            };
            let Ok(span) = span else {
                return (false, "the history moved".to_string());
            };
            if span.is_empty() {
                return (false, "nothing to do".to_string());
            }
            let result = UndoNavigator::walk(&mut undo, &span, &host, &measure, &FileTime::now);
            self.refresh_undo_summary(&undo);
            (span, result)
        };

        result.released.retire_all();
        for refresh in &result.refreshes {
            let gone: Vec<String> = refresh
                .vacated
                .iter()
                .filter(|d| d.is_directory)
                .map(|d| d.source_path.clone())
                .collect();
            if !gone.is_empty() {
                self.leave_gone_folders(&gone);
            }
        }
        let mut directories: Vec<String> = result
            .refreshes
            .iter()
            .flat_map(|r| r.directories.iter().cloned())
            .collect();
        directories.sort_by_key(|d| d.to_uppercase());
        directories.dedup_by_key(|d| d.to_uppercase());
        self.refresh_tabs_showing(&directories);

        let did = if direction == UndoDirection::Undo {
            "Undone"
        } else {
            "Redone"
        };
        let text = match (&result.stopped_at, &result.stop_report) {
            (Some(_), Some(report)) => {
                let first = report.failures.first().cloned().unwrap_or_default();
                if report.nothing_happened() {
                    format!("Could not {} \u{2014} {first}", did.to_lowercase())
                } else {
                    format!(
                        "{did} only in part: {} could not be \u{2014} {first}",
                        report.failures.len()
                    )
                }
            }
            _ if span.len() == 1 => did.to_string(),
            _ => format!("{did} {} actions", span.len()),
        };
        self.set_active_status(text.clone());
        self.hub.emit(AppEvent::UndoChanged);
        (result.stopped_at.is_none(), text)
    }
}
