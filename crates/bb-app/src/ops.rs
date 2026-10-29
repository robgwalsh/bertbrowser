//! Move, copy, delete, rename and undo, and the queue that runs them. Spec: ShellViewModel
//! (EnqueueAsync, DrainQueueAsync, RunDropAsync, DeleteAsync, RenameAsync, RecordAsync, WalkAsync).
//!
//! One thread, `bb-transfer`, drains the queue. `draining` is "this app is writing", and it is set
//! when a job is *accepted* and cleared only when the queue is empty, so releasing it between two
//! jobs cannot let a rename slip into the gap. A paused run is still draining; the settle predicate
//! alone treats it as quiet. Rename, delete and undo are refused while anything is draining (they
//! share the flag so Ctrl+Z cannot reach the previous record while one of them is running); a drop
//! or paste queues behind the running job.

use std::sync::atomic::Ordering;
use std::sync::{Arc, LazyLock, Weak};

use bb_core::FileTime;
use bb_core::cancel::CancelToken;
use bb_core::delete::{
    DeleteExecutor, DeleteMode, DeletePlanner, DeleteSource, FileSystemDeleteProbe, ShellRecycleBin,
};
use bb_core::elevation::{ElevatedRetry, ElevationOperation};
use bb_core::rename::{RenameExecutor, RenamePlanner, RenameRule, RenameSource};
use bb_core::transfer::{
    DirectorySize, FileSystemTransferProbe, QueuedJobState, TransferEstimate, TransferEstimator,
    TransferExecutor, TransferMergeExpander, TransferPlanner, TransferProgress, TransferSizeSource,
    TransferVerb,
};
use bb_core::undo::{
    DeleteRecord, HeldFolderSizes, HeldSizeEstimator, PlainUndoHost, RenameRecord, TransferRecord,
    UndoDirection, UndoNavigator, UndoStack, UndoableRecord,
};

use crate::app::{Inner, Job, JobBody, OpState, UndoSummary, lock};
use crate::events::AppEvent;
use crate::ids::{AppError, AppResult, OpId};
use crate::ops_ui::{JobMeta, TransferTracker};

pub(crate) static RECYCLE_BIN: LazyLock<ShellRecycleBin> = LazyLock::new(ShellRecycleBin::new);
pub(crate) static DELETE_PROBE: FileSystemDeleteProbe = FileSystemDeleteProbe;

/// What a plan will write, as far as `dir_size_cache` can say: the bar is drawn against it.
struct CachedSource<'a>(&'a bb_core::data::DirSizeRepository);

impl TransferSizeSource for CachedSource<'_> {
    fn directory(&self, path: &str) -> Option<DirectorySize> {
        match self.0.get(path) {
            Ok(Some(row)) if !row.incomplete => Some(DirectorySize {
                bytes: row.size_bytes,
                files: row.file_count.max(0) as usize,
            }),
            _ => None,
        }
    }

    fn file(&self, path: &str) -> Option<i64> {
        std::fs::metadata(path).ok().map(|m| m.len() as i64)
    }
}

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
                                    inner.queue_clear();
                                    inner.gate.resume();
                                    inner.extra.ops.archive_gate.resume();
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
        // Cancelled while it waited: it never starts.
        if job.cancel.is_cancelled() {
            lock(&self.queue.state).running = None;
            lock(&self.ops).insert(
                id,
                OpState::Done {
                    ok: false,
                    message: "cancelled".to_string(),
                },
            );
            self.queue_state_of(id.0, QueuedJobState::Cancelled);
            self.hub.emit(AppEvent::OpFinished { op: id, ok: false });
            return;
        }
        lock(&self.ops).insert(id, OpState::Running);
        *lock(&self.extra.ops.running_cancel) = Some((id.0, job.cancel.clone()));
        self.extra.ops.cancelling.store(false, Ordering::SeqCst);
        self.queue_state_of(id.0, QueuedJobState::Running);
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
        *lock(&self.extra.ops.running_cancel) = None;
        self.tracker_end();
        lock(&self.queue.state).running = None;
        lock(&self.ops).insert(id, OpState::Done { ok, message });
        self.queue_state_of(
            id.0,
            if cancel.is_cancelled() {
                QueuedJobState::Cancelled
            } else {
                QueuedJobState::Done
            },
        );
        self.hub.emit(AppEvent::OpFinished { op: id, ok });
    }

    /// Accepts a job that is not a row of the queue.
    pub(crate) fn submit(
        self: &Arc<Self>,
        kind: &'static str,
        exclusive: bool,
        body: JobBody,
    ) -> AppResult<OpId> {
        self.submit_with(JobMeta::hidden(kind), exclusive, body)
    }

    /// Accepts a job. `exclusive` jobs (rename, delete, undo) are refused while anything is draining.
    pub(crate) fn submit_with(
        self: &Arc<Self>,
        meta: JobMeta,
        exclusive: bool,
        body: JobBody,
    ) -> AppResult<OpId> {
        let kind = meta.kind.as_str();
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
        self.queue_add(id.0, &meta);
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

    /// Stops an operation: one still waiting never starts, the running one is told to stop.
    pub(crate) fn cancel_op(&self, op: OpId) -> AppResult<()> {
        let running = lock(&self.queue.state).running == Some(op);
        if running {
            self.cancel_running();
            return Ok(());
        }
        let waiting = {
            let state = lock(&self.queue.state);
            state
                .jobs
                .iter()
                .find(|j| j.id == op)
                .map(|j| j.cancel.cancel())
                .is_some()
        };
        if waiting {
            self.queue_state_of(op.0, QueuedJobState::Cancelled);
            return Ok(());
        }
        Err(AppError::Stale)
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        if paused {
            self.gate.pause();
            self.extra.ops.archive_gate.pause();
        } else {
            self.gate.resume();
            self.extra.ops.archive_gate.resume();
        }
        if let Some(t) = lock(&self.extra.ops.tracker).as_mut() {
            t.set_paused(paused);
        }
        self.update_busy();
        self.transfer_changed();
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

    /// What a status line adds after something undoable: the key for Undo, asked of the keymap each
    /// time because it can be rebound, or the plain sentence when it has none.
    /// Spec: ShellViewModel.UndoHint.
    pub(crate) fn undo_hint(&self) -> String {
        let gesture = self.keymap_gesture("edit.undo");
        if gesture.is_empty() {
            " \u{2014} this can be undone".to_string()
        } else {
            format!(" \u{2014} {gesture} to undo")
        }
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

    /// Says `text` in the tab showing `directory` (the active one when several do), else in the
    /// active tab. A drop reports where it landed. Spec: FileDragDropController, `tab.StatusText`.
    fn set_status_where(&self, directory: &str, text: String) {
        let active = lock(&self.shell).active_tab_id();
        let mut found = None;
        if let Ok(key) = bb_paths::PathKey::canonicalize(directory) {
            for (id, cell) in self.all_tabs() {
                let showing = {
                    let t = lock(&cell);
                    !t.path.is_empty()
                        && bb_paths::PathKey::canonicalize(&t.path).is_ok_and(|k| k == key)
                };
                if showing {
                    if Some(id) == active {
                        found = Some(id);
                        break;
                    }
                    found.get_or_insert(id);
                }
            }
        }
        if let Some(tab) = found.or(active) {
            self.set_status(tab, text);
        }
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
        let meta = JobMeta::shown(
            kind,
            format!(
                "{} item(s) to {}",
                count_text(sources.len()),
                bb_paths::file_name(destination.trim_end_matches(['\\', '/']))
            ),
            sources.len(),
            TransferEstimate::new(0, 0, false),
        );
        self.submit_with(
            meta,
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
        let planner = TransferPlanner::default();
        let mut plan = planner.plan(sources, destination, verb);

        if !plan.has_work() {
            // The first thing the planner refused, verbatim, else the no-op it is. Said in the tab
            // that shows the destination: the pane dropped into, not whichever has the focus.
            // Spec: DropPipeline.PerformAsync.
            let text = plan.problems().first().map_or_else(
                || "Nothing to move \u{2014} those items are already there.".to_string(),
                |r| r.message.clone(),
            );
            self.set_status_where(destination, text.clone());
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
                let text = "Nothing to move \u{2014} those items are already there.".to_string();
                self.set_status_where(destination, text.clone());
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
        let estimate = TransferEstimator::estimate(&plan, &CachedSource(&self.dir_sizes));
        self.tracker_begin(TransferTracker::for_plan(&plan, estimate));
        let me = self.clone();
        let mut on_progress = move |p: TransferProgress| {
            me.tracker_apply(&p);
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

        // Refused for permissions: offer to try again with administrator rights, before the entry
        // is recorded (a retry is one action to the user, however many passes it takes).
        let mut elevated_note = String::new();
        if let Some(retry) =
            ElevatedRetry::retry_for_transfer(&plan, &outcome, resolutions.as_ref())
        {
            let items: Vec<String> = retry
                .plan
                .transfers
                .iter()
                .map(|t| t.source_path.clone())
                .collect();
            let operation = if verb == TransferVerb::Move {
                ElevationOperation::TransferMove
            } else {
                ElevationOperation::TransferCopy
            };
            let (second, note) = self.elevate(operation, &items, false, |runner| {
                // Its own progress, built from the retry plan: the first pass's item counts and byte
                // total describe an operation that has already finished.
                let estimate =
                    TransferEstimator::estimate(&retry.plan, &CachedSource(&self.dir_sizes));
                self.tracker_begin(TransferTracker::for_plan(&retry.plan, estimate));
                self.set_active_status(format!(
                    "Retrying {} item(s) as administrator\u{2026}",
                    count_text(retry.plan.transfers.len())
                ));
                runner.run_transfer(&retry, Some(&mut on_progress), cancel)
            });
            elevated_note = note;
            if let Some(second) = second {
                match ElevatedRetry::merge_transfer(&outcome, &retry, &second) {
                    Ok(merged) => outcome = merged,
                    Err(e) => elevated_note = format!(" \u{2014} {e}"),
                }
            }
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
            text.push_str(&self.undo_hint());
        }
        text.push_str(&elevated_note);
        self.set_final_status(text.clone());
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
        // Inside a container a delete is a rewrite of it, held so it can be taken back.
        if let Some(first) = crate::ops_arch::archive_of(&paths[0]) {
            let mut edits = Vec::new();
            for p in &paths {
                match crate::ops_arch::archive_of(p) {
                    Some(a) if a.archive_file.eq_ignore_ascii_case(&first.archive_file) => {
                        edits.push(bb_core::archives::ArchiveEdit::remove(&a.entry_path));
                    }
                    _ => {
                        return Err(AppError::Invalid(
                            "Items from different places cannot be deleted together inside an archive."
                                .into(),
                        ));
                    }
                }
            }
            return self.op_archive_edit(first.archive_file, edits, "Deleted");
        }
        self.submit(
            "Delete",
            true,
            Box::new(move |inner, _op, cancel| inner.run_delete(&paths, mode, cancel)),
        )
    }

    pub(crate) fn delete_executor(&self) -> DeleteExecutor<'static> {
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
        let mut outcome = self
            .delete_executor()
            .execute(&plan, cancel, Some(&on_progress));

        let mut elevated_note = String::new();
        if let Some(retry) = ElevatedRetry::retry_for_delete(&plan, &outcome) {
            let items: Vec<String> = retry
                .plan
                .deletions
                .iter()
                .map(|d| d.source_path.clone())
                .collect();
            let (second, note) =
                self.elevate(ElevationOperation::Delete, &items, false, |runner| {
                    self.set_active_status(format!(
                        "Retrying {} item(s) as administrator\u{2026}",
                        count_text(retry.plan.deletions.len())
                    ));
                    let mut forward = |p: bb_core::delete::DeleteProgress| on_progress(p);
                    runner.run_delete(&retry, Some(&mut forward), cancel)
                });
            elevated_note = note;
            if let Some(second) = second {
                match ElevatedRetry::merge_delete(&outcome, &retry, &second) {
                    Ok(merged) => outcome = merged,
                    Err(e) => elevated_note = format!(" \u{2014} {e}"),
                }
            }
        }

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
            // What could not be deleted is said where it will be read, not only in the footer.
            // Spec: DirectoryTabView.DeleteSelectionAsync.
            let all: Vec<&str> = outcome.failed.iter().map(|f| f.message.as_str()).collect();
            self.seams.notice.say(&all.join("\n\n"), "Delete");
        } else if outcome.can_undo() {
            text.push_str(&self.undo_hint());
        }
        text.push_str(&elevated_note);
        self.set_final_status(text.clone());
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
        if let Some(a) = crate::ops_arch::archive_of(&paths[0]) {
            if paths.len() != 1 {
                return Err(AppError::Invalid(
                    "Entries inside an archive are renamed one at a time.".into(),
                ));
            }
            return self.op_archive_edit(
                a.archive_file,
                vec![bb_core::archives::ArchiveEdit::rename(
                    &a.entry_path,
                    &pattern,
                )],
                "Renamed",
            );
        }
        self.submit(
            "Rename",
            true,
            Box::new(move |inner, _op, _cancel| {
                inner.run_rename(&paths, &|sources| {
                    RenamePlanner::local().plan_simple(sources, &pattern)
                })
            }),
        )
    }

    /// The expanded panel's rename: a template, a find and replace, a case and a counter. A
    /// template that names a tag has each file's own metadata read onto it first.
    pub(crate) fn op_rename_rule(
        self: &Arc<Self>,
        paths: Vec<String>,
        rule: RenameRule,
    ) -> AppResult<OpId> {
        if paths.is_empty() {
            return Err(AppError::Invalid("nothing to rename".into()));
        }
        self.submit(
            "Rename",
            true,
            Box::new(move |inner, _op, _cancel| {
                let uses_tags = bb_core::rename::RenameTemplate::uses_tags(&rule.template);
                inner.run_rename(&paths, &|sources| {
                    let mut sources = sources.to_vec();
                    if uses_tags {
                        for s in &mut sources {
                            s.tags = Some(crate::metadata_pane::tags_of(&s.path, s.is_directory));
                        }
                    }
                    RenamePlanner::local().plan(&sources, &rule)
                })
            }),
        )
    }

    fn run_rename(
        self: &Arc<Self>,
        paths: &[String],
        make_plan: &dyn Fn(&[RenameSource]) -> bb_core::rename::RenamePlan,
    ) -> (bool, String) {
        let sources: Vec<RenameSource> = paths
            .iter()
            .map(|p| {
                let mut source = if std::path::Path::new(p).is_dir() {
                    RenameSource::dir(p)
                } else {
                    RenameSource::file(p)
                };
                source.modified = std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .ok()
                    .map(|t| FileTime::from_system_time(t).to_local());
                source
            })
            .collect();
        let plan = make_plan(&sources);
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
        let mut outcome = RenameExecutor.execute(&plan);

        let mut elevated_note = String::new();
        if let Some(retry) = ElevatedRetry::retry_for_rename(&plan, &outcome) {
            let items: Vec<String> = retry
                .plan
                .renames
                .iter()
                .map(|r| r.source_path.clone())
                .collect();
            let (second, note) =
                self.elevate(ElevationOperation::Rename, &items, false, |runner| {
                    self.set_active_status(format!(
                        "Retrying {} item(s) as administrator\u{2026}",
                        count_text(retry.plan.renames.len())
                    ));
                    runner.run_rename(&retry, &CancelToken::new())
                });
            elevated_note = note;
            if let Some(second) = second {
                outcome = ElevatedRetry::merge_rename(&outcome, &retry, &second);
            }
        }

        self.record(RenameRecord::for_outcome(outcome.clone()).map(|r| Box::new(r) as _));

        let mut directories: Vec<String> = outcome
            .completed
            .iter()
            .filter_map(|c| bb_core::rename::paths::directory_name(&c.source_path))
            .collect();
        directories.sort_by_key(|d| d.to_uppercase());
        directories.dedup_by_key(|d| d.to_uppercase());
        // What was renamed is still selected, under its new name.
        for dir in &directories {
            let finals: Vec<String> = outcome
                .completed
                .iter()
                .filter(|c| {
                    bb_core::rename::paths::directory_name(&c.source_path)
                        .is_some_and(|d| d.eq_ignore_ascii_case(dir))
                })
                .map(|c| c.final_path.clone())
                .collect();
            self.select_many_after_refresh(dir, finals);
        }
        self.refresh_tabs_showing(&directories);
        self.follow_renamed_folders(&outcome.completed);

        let mut text = format!("Renamed {} item(s)", outcome.completed.len());
        if let Some(first) = outcome.failed.first() {
            text.push_str(&format!(
                "; {} failed \u{2014} {}",
                outcome.failed.len(),
                first.message
            ));
            let all: Vec<&str> = outcome.failed.iter().map(|f| f.message.as_str()).collect();
            self.seams.notice.say(&all.join("\n\n"), "Rename");
        } else if outcome.can_undo() {
            text.push_str(&self.undo_hint());
        }
        text.push_str(&elevated_note);
        self.set_final_status(text.clone());
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
        // The plain host's executors, with an elevated second pass offered where Windows refuses part of
        // a step - as the forward operations do.
        let host = crate::undo_host::AppUndoHost::new(
            self,
            PlainUndoHost::without_archives(&transfers, &deletes, &renames),
        );
        let sizes = CachedSizes(&self.dir_sizes);
        let estimator = HeldSizeEstimator::new(Some(&sizes));
        let measure = |r: &dyn UndoableRecord| estimator.measure_record(r);

        let (span, result, described) = {
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
            let described: Vec<(bb_core::undo::EntryId, String)> = undo
                .entries()
                .iter()
                .map(|e| (e.id(), e.record().description().to_string()))
                .collect();
            let result = UndoNavigator::walk(&mut undo, &span, &host, &measure, &FileTime::now);
            self.refresh_undo_summary(&undo);
            (span, result, described)
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
        self.refresh_archive_tabs();
        let renamed: Vec<_> = result
            .refreshes
            .iter()
            .flat_map(|r| r.renamed.iter().cloned())
            .collect();
        self.follow_renamed_folders(&renamed);
        if result.refreshes.iter().any(|r| r.rescan_compare) {
            let _ = self.compare_rescan();
        }

        // "Undone \u{2014} move 3 items to Documents", "Undone 4 actions", or where and why it stopped.
        // Spec: ShellViewModel.DescribeWalk.
        let (did, verb) = if direction == UndoDirection::Undo {
            ("Undone", "undo")
        } else {
            ("Redone", "redo")
        };
        let name_of = |id: &bb_core::undo::EntryId| {
            described
                .iter()
                .find(|(e, _)| e == id)
                .map(|(_, d)| bb_core::undo::UndoText::as_object(d))
                .unwrap_or_default()
        };
        let notes: String = result.notes.concat();
        let text = match (&result.stopped_at, &result.stop_report) {
            (Some(stopped), Some(report)) => {
                let name = name_of(stopped);
                let first = report.failures.first().cloned().unwrap_or_default();
                let mut text = if report.nothing_happened() {
                    format!("Could not {verb} {name} \u{2014} {first}")
                } else {
                    format!(
                        "{did} {name} only in part: {} could not be \u{2014} {first}",
                        count_text(report.failures.len())
                    )
                };
                let remaining = span
                    .len()
                    .saturating_sub(result.steps_taken)
                    .saturating_sub(usize::from(report.nothing_happened()));
                if remaining > 0 {
                    text.push_str(&format!(
                        " (stopped there; {} more not {})",
                        count_text(remaining),
                        did.to_lowercase()
                    ));
                }
                text + &notes
            }
            _ if span.len() == 1 => format!("{did} \u{2014} {}{notes}", name_of(&span[0])),
            _ => format!("{did} {} actions{notes}", count_text(span.len())),
        };
        self.set_final_status(text.clone());
        self.hub.emit(AppEvent::UndoChanged);
        (result.stopped_at.is_none(), text)
    }
}
