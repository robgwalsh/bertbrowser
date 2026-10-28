//! The elevated end of one file operation: reads a single request, carries it out through the same
//! Core executors the app uses, reports what happened, and returns.
//! Spec: Core/Services/Elevation/ElevationHost.cs.
//!
//! **It hosts the real executors, not a narrower set of primitives, and that is the safety argument
//! rather than a convenience.** Every invariant that matters lives inside them: nothing deleted to
//! make room, a cross-volume move that copies, verifies and only then deletes, junction trees
//! refused, `DirectoryRemoval::remove_tree` instead of a recursive delete, the staging-folder name
//! guard, `ProtectedLocations`, and every planner rule re-applied against live disk before a byte
//! moves. A helper that took "copy this file" primitives would be *following instructions* from a
//! medium-integrity peer; hosting the executors means the process doing the dangerous thing is the
//! process re-checking the rules.
//!
//! **One request, then it returns**, and that is held twice over rather than once. `Begin` is
//! accepted only when there is no header yet and `Item` only before `Go`; after `Go` the only verb
//! read is `Cancel`, and the pipe is closed as the work finishes. So a peer that somehow owned the
//! pipe can neither add a path to a running operation nor start a second one, and neither guard is
//! load-bearing alone, which is why `a_second_header_is_refused` asserts the state machine directly
//! instead of inferring it from what did or did not happen on disk.
//!
//! The work runs on this thread while a second one stays in the read loop. That is not tidiness: a
//! `Cancel` arriving while the only thread sat inside `execute` would never be seen, and (as the
//! index helper's pipe already records) a non-overlapped handle serialises I/O, so a blocking read
//! would also block the progress writes on the same handle.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use super::launcher::ElevationStream;
use super::models::{
    ElevationHeader, ElevationNewItemResult, ElevationOperation, ElevationProgressReport,
    ElevationSummary, ElevationTransferItem, ElevationTransferResult,
};
use super::wire::WireRecord;
use crate::cancel::CancelToken;
use crate::cli::NavigationRequest;
use crate::delete::{
    DeleteExecutor, DeleteOutcome, DeletePlan, DeleteProgress, DeletedItem, FileSystemDeleteProbe,
    PlannedDelete, RecycleBin, RecycleProbe,
};
use crate::ipc::{ElevationMessage, ElevationProtocol, ElevationVerb, LineReader, write_line};
use crate::newitem::{NewItemExecutor, NewItemPlan};
use crate::rename::{PlannedRename, RenameExecutor, RenamePlan};
use crate::transfer::{
    CompletedTransfer, Resolutions, TransferExecutor, TransferOutcome, TransferPlan,
    TransferProgress, TransferVerb,
};

const NOT_A_PATH: &str = "The request named something that is not an acceptable path.";

pub struct ElevationHost<'a> {
    stream: Mutex<Box<dyn ElevationStream>>,
    recycle_bin: Option<&'a dyn RecycleBin>,
    recycle_probe: Option<&'a dyn RecycleProbe>,
    grant_access: Option<&'a dyn Fn(&str)>,
}

impl<'a> ElevationHost<'a> {
    pub fn new(stream: Box<dyn ElevationStream>) -> ElevationHost<'a> {
        ElevationHost {
            stream: Mutex::new(stream),
            recycle_bin: None,
            recycle_probe: None,
            grant_access: None,
        }
    }

    pub fn with_recycle_bin(mut self, bin: &'a dyn RecycleBin) -> ElevationHost<'a> {
        self.recycle_bin = Some(bin);
        self
    }

    pub fn with_recycle_probe(mut self, probe: &'a dyn RecycleProbe) -> ElevationHost<'a> {
        self.recycle_probe = Some(probe);
        self
    }

    /// Called for every staging folder the run created, so the process that launched this one can
    /// still commit and purge it. A folder an elevated process makes at a volume root inherits
    /// that root's ACL, which grants ordinary users read and not delete, so without this an
    /// elevated staged delete leaves a holding folder the app can neither erase nor undo from. The
    /// database's own note about Administrators-owned files does not cover this: that reasoning is
    /// about the profile, which grants the interactive user inheritable full control, and a volume
    /// root does not.
    pub fn with_grant_access(mut self, grant: &'a dyn Fn(&str)) -> ElevationHost<'a> {
        self.grant_access = Some(grant);
        self
    }

    /// Serves one request and returns. A missing line (the app has gone) ends it too. `cancel` is
    /// what the executors watch: the caller's own reason to stop, and a `Cancel` verb from the app,
    /// both land on it.
    pub fn run(&self, cancel: &CancelToken) {
        let Ok(reading) = self.stream.lock().unwrap().split() else {
            return;
        };
        let mut reader = LineReader::new(reading, NavigationRequest::MAX_LINE_LENGTH);

        self.send(
            ElevationVerb::Hello,
            &ElevationProtocol::PROTOCOL_VERSION.to_string(),
        );
        self.send(ElevationVerb::Ready, "");

        let mut header: Option<ElevationHeader> = None;
        let mut items: Vec<String> = Vec::new();

        loop {
            let Ok(Some(line)) = reader.read_line() else {
                return;
            };
            let Some(message) = ElevationProtocol::try_parse(&line) else {
                continue;
            };

            match message.verb {
                ElevationVerb::Hello => {
                    if ElevationProtocol::version_of(&message)
                        != Some(ElevationProtocol::PROTOCOL_VERSION)
                    {
                        self.send(ElevationVerb::Fatal, "Version mismatch.");
                        return;
                    }
                }

                ElevationVerb::Begin if header.is_none() => {
                    match ElevationHeader::from_json(&message.payload) {
                        Some(parsed) => header = Some(parsed),
                        None => {
                            self.send(
                                ElevationVerb::Fatal,
                                "The request header could not be read.",
                            );
                            return;
                        }
                    }
                }

                ElevationVerb::Item if header.is_some() => {
                    if items.len() >= ElevationProtocol::MAX_ITEMS {
                        // Refused whole rather than truncated: a partial request is a different
                        // operation from the one the user consented to.
                        self.send(
                            ElevationVerb::Fatal,
                            "The request names more items than this helper accepts.",
                        );
                        return;
                    }
                    items.push(message.payload);
                }

                ElevationVerb::Go if header.is_some() => {
                    let header = header.take().expect("checked above");
                    self.carry(&header, &items, cancel, reader);
                    return;
                }

                _ => {
                    self.send(
                        ElevationVerb::Fatal,
                        "That is not something this helper accepts here.",
                    );
                    return;
                }
            }
        }
    }

    // --- carrying out the one request ---

    fn carry(
        &self,
        header: &ElevationHeader,
        items: &[String],
        cancel: &CancelToken,
        mut reader: LineReader<Box<dyn ElevationStream>>,
    ) {
        std::thread::scope(|scope| {
            let watcher_cancel = cancel.clone();
            let watcher = scope.spawn(move || {
                #[allow(clippy::while_let_loop)]
                loop {
                    // A missing line is the app gone or the work finished and the stream shut
                    // down. Either way there is nothing more to read; cancelling is right in both
                    // cases, and harmless in the second because the work is already done.
                    let Ok(Some(line)) = reader.read_line() else {
                        break;
                    };
                    let Some(message) = ElevationProtocol::try_parse(&line) else {
                        continue;
                    };
                    if message.verb != ElevationVerb::Cancel {
                        continue;
                    }
                    watcher_cancel.cancel();
                    break;
                }
            });

            // Reported from in here rather than after the join, because the shutdown below ends
            // the stream: a Fatal sent afterwards would be swallowed as a broken pipe and the app
            // would see the helper simply vanish.
            if let Err(panic) =
                catch_unwind(AssertUnwindSafe(|| self.perform(header, items, cancel)))
            {
                let text = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                self.send(ElevationVerb::Fatal, &ElevationProtocol::summarize(&text));
            }

            // Unparks the read loop, which is otherwise waiting for a Cancel that is now never
            // coming.
            self.stream.lock().unwrap().shutdown();
            let _ = watcher.join();
        });
    }

    fn perform(&self, header: &ElevationHeader, items: &[String], cancel: &CancelToken) {
        match header.operation {
            ElevationOperation::TransferMove | ElevationOperation::TransferCopy => {
                self.transfer(header, items, cancel)
            }
            ElevationOperation::TransferUndo => self.undo_transfer(header, items),
            ElevationOperation::Delete => self.delete_items(header, items, cancel),
            ElevationOperation::DeleteUndo => self.undo_delete(header, items),
            ElevationOperation::Rename => self.rename_items(items),
            ElevationOperation::NewItem => self.create_item(items),
        }
    }

    fn transfer(&self, header: &ElevationHeader, items: &[String], cancel: &CancelToken) {
        let Some(parsed) = read_all::<ElevationTransferItem>(items) else {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        };
        if !all_acceptable(parsed.iter().map(|p| p.item.source_path.as_str()))
            || !NavigationRequest::is_acceptable_path(&header.destination_directory)
        {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        }

        let verb = if header.operation == ElevationOperation::TransferMove {
            TransferVerb::Move
        } else {
            TransferVerb::Copy
        };
        let plan = TransferPlan::new(
            verb,
            &header.destination_directory,
            parsed.iter().map(|p| p.item.clone()).collect(),
            Vec::new(),
        );
        let resolutions: Resolutions = parsed
            .iter()
            .filter_map(|p| {
                bb_paths::PathKey::canonicalize(&p.item.source_path)
                    .ok()
                    .map(|key| (key, p.resolution))
            })
            .collect();

        let mut report = |progress: TransferProgress| self.report_transfer(progress);
        let outcome = TransferExecutor::default().execute(
            &plan,
            Some(&resolutions),
            cancel,
            Some(&mut report),
            None,
        );

        for done in &outcome.completed {
            self.send(
                ElevationVerb::Done,
                &ElevationTransferResult {
                    completed: Some(done.clone()),
                    skipped: None,
                }
                .to_json(),
            );
        }
        for skipped in &outcome.skipped {
            self.send(
                ElevationVerb::Done,
                &ElevationTransferResult {
                    completed: None,
                    skipped: Some(skipped.clone()),
                }
                .to_json(),
            );
        }
        for fault in &outcome.failed {
            self.send(ElevationVerb::Fault, &fault.to_json());
        }

        self.finish(outcome.cancelled, &outcome.staging_directories, 0);
    }

    fn undo_transfer(&self, header: &ElevationHeader, items: &[String]) {
        let Some(parsed) = read_all::<CompletedTransfer>(items) else {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        };
        if !all_acceptable(parsed.iter().map(|p| p.source_path.as_str()))
            || !all_acceptable(parsed.iter().map(|p| p.final_path.as_str()))
        {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        }

        // staging_directories: empty deliberately. The unelevated half still holds items in its
        // own staging folders, and this pass must not purge them out from under it.
        let outcome = TransferOutcome::new(
            TransferVerb::Move,
            &header.destination_directory,
            parsed,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
        );
        let result = TransferExecutor::default().undo(&outcome, &CancelToken::none());

        for fault in &result.failed {
            self.send(ElevationVerb::Fault, &fault.to_json());
        }
        self.finish(false, &[], result.restored);
    }

    fn delete_items(&self, header: &ElevationHeader, items: &[String], cancel: &CancelToken) {
        let Some(parsed) = read_all::<PlannedDelete>(items) else {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        };
        if !all_acceptable(parsed.iter().map(|p| p.source_path.as_str())) {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        }

        let plan = DeletePlan {
            mode: header.delete_mode,
            deletions: parsed,
            rejected: Vec::new(),
        };
        let report = |progress: DeleteProgress| self.report_delete(progress);
        let outcome = self.deleter().execute(&plan, cancel, Some(&report));

        for done in &outcome.deleted {
            self.send(ElevationVerb::Done, &done.to_json());
        }
        for fault in &outcome.failed {
            self.send(ElevationVerb::Fault, &fault.to_json());
        }

        self.finish(false, &outcome.staging_directories, 0);
    }

    fn undo_delete(&self, header: &ElevationHeader, items: &[String]) {
        let Some(parsed) = read_all::<DeletedItem>(items) else {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        };
        if !all_acceptable(parsed.iter().map(|p| p.source_path.as_str())) {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        }

        let outcome = DeleteOutcome {
            permanent: header.permanent,
            deleted: parsed,
            failed: Vec::new(),
            staging_directories: Vec::new(),
        };
        let result = self.deleter().undo(&outcome, &CancelToken::none());

        for fault in &result.failed {
            self.send(ElevationVerb::Fault, &fault.to_json());
        }
        self.finish(false, &[], result.restored);
    }

    fn rename_items(&self, items: &[String]) {
        let Some(parsed) = read_all::<PlannedRename>(items) else {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        };
        if !all_acceptable(parsed.iter().map(|p| p.source_path.as_str()))
            || !all_acceptable(parsed.iter().map(|p| p.target_path.as_str()))
        {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        }

        let outcome = RenameExecutor.execute(&RenamePlan::new(parsed, Vec::new()));

        for done in &outcome.completed {
            self.send(ElevationVerb::Done, &done.to_json());
        }
        for fault in &outcome.failed {
            self.send(ElevationVerb::Fault, &fault.to_json());
        }

        self.finish(false, &[], 0);
    }

    fn create_item(&self, items: &[String]) {
        let Some(parsed) = read_all::<NewItemPlan>(items) else {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        };
        let [plan] = parsed.as_slice() else {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        };
        if !NavigationRequest::is_acceptable_path(&plan.directory)
            || plan
                .template_path
                .as_deref()
                .is_some_and(|t| !NavigationRequest::is_acceptable_path(t))
        {
            return self.send(ElevationVerb::Fatal, NOT_A_PATH);
        }

        let outcome = NewItemExecutor.execute(plan);

        if let Some(created) = &outcome.created_path {
            self.send(
                ElevationVerb::Done,
                &ElevationNewItemResult {
                    created_path: created.clone(),
                }
                .to_json(),
            );
        }
        if let Some(failure) = &outcome.failed {
            self.send(ElevationVerb::Fault, &failure.to_json());
        }

        self.finish(false, &[], 0);
    }

    fn deleter(&self) -> DeleteExecutor<'_> {
        let mut executor = DeleteExecutor::new(&FileSystemDeleteProbe);
        if let Some(bin) = self.recycle_bin {
            executor = executor.recycle_bin(bin);
        }
        if let Some(probe) = self.recycle_probe {
            executor = executor.recycle_probe(probe);
        }
        executor
    }

    fn finish(&self, cancelled: bool, staging: &[String], restored: usize) {
        if let Some(grant) = self.grant_access {
            for directory in staging {
                grant(directory);
            }
        }
        self.send(
            ElevationVerb::End,
            &ElevationSummary {
                cancelled,
                staging_directories: staging.to_vec(),
                restored,
            }
            .to_json(),
        );
    }

    // --- the wire ---

    fn report_transfer(&self, progress: TransferProgress) {
        self.send(
            ElevationVerb::Progress,
            &ElevationProgressReport {
                done: progress.done,
                total: progress.total,
                current_name: progress.current_name,
                bytes_done: progress.bytes_done,
                current_item_bytes: progress.current_item_bytes,
                current_item_total: progress.current_item_total,
            }
            .to_json(),
        );
    }

    fn report_delete(&self, progress: DeleteProgress) {
        self.send(
            ElevationVerb::Progress,
            &ElevationProgressReport {
                done: progress.done,
                total: progress.total,
                current_name: progress.current_name,
                ..ElevationProgressReport::default()
            }
            .to_json(),
        );
    }

    fn send(&self, verb: ElevationVerb, payload: &str) {
        let mut stream = self.stream.lock().unwrap();
        // A failure here is the app gone. Losing the pipe is a cancel with nobody left to tell,
        // and the executors' own guarantees mean nothing is half-written.
        let _ = write_line(
            stream.as_mut(),
            &ElevationProtocol::format(&ElevationMessage::new(verb, payload)),
        );
    }

    /// One record as it goes on the wire, for the app end and for tests that drive this end by
    /// hand.
    pub fn write<T: WireRecord>(value: &T) -> String {
        value.to_json()
    }
}

fn all_acceptable<'p>(paths: impl Iterator<Item = &'p str>) -> bool {
    let mut any = false;
    for path in paths {
        any = true;
        if !NavigationRequest::is_acceptable_path(path) {
            return false;
        }
    }
    let _ = any;
    true
}

/// Every item, or None if any one of them could not be read: a malformed request is refused whole
/// rather than carried out in part.
fn read_all<T: WireRecord>(payloads: &[String]) -> Option<Vec<T>> {
    payloads.iter().map(|p| T::from_json(p)).collect()
}
