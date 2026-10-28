//! The app's half of the elevated-operation pipe: raise the prompt, hand the plan to a short-lived
//! helper, stream its progress back, and return an outcome the caller can merge as if nothing
//! unusual had happened. Spec: Core/Services/Elevation/ElevationClient.cs.
//!
//! One session per operation, and the session *is* the process: create the pipe, raise the prompt,
//! greet, send one request, read until the helper says it is finished, and let it exit. The index
//! client keeps a worker thread alive for the whole session because its helper does; this one has
//! nothing to keep.
//!
//! Nothing here retries on a timer, and nothing here elevates without having been asked. Every
//! attempt is a UAC prompt, which is the same rule the index client follows for the same reason.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::launcher::{
    ElevationLaunch, ElevationLauncher, ElevationStream, ElevationTransportFactory,
};
use super::models::{
    ElevatedRun, ElevationHeader, ElevationNewItemResult, ElevationOperation,
    ElevationProgressReport, ElevationSummary, ElevationTransferItem, ElevationTransferResult,
};
use super::retry::{DeleteRetry, NewItemRetry, RenameRetry, TransferRetry};
use super::rules::ElevationRules;
use super::wire::WireRecord;
use crate::cancel::CancelToken;
use crate::cli::NavigationRequest;
use crate::delete::{DeleteOutcome, DeleteProgress, DeleteUndoResult, DeletedItem, FailedDelete};
use crate::ipc::{ElevationMessage, ElevationProtocol, ElevationVerb, LineReader, write_line};
use crate::newitem::{FailedNewItem, NewItemOutcome};
use crate::rename::{CompletedRename, FailedRename, RenameOutcome};
use crate::transfer::{
    ConflictResolution, FailedTransfer, Resolutions, TransferOutcome, TransferProgress,
    TransferUndoResult, TransferVerb,
};

/// Running one file operation with an administrator token.
pub trait ElevatedOperationRunner {
    /// Whether the offer should be made at all. See [`ElevationLauncher::can_elevate`].
    fn can_elevate(&self) -> bool;

    fn run_transfer(
        &self,
        retry: &TransferRetry,
        progress: Option<&mut dyn FnMut(TransferProgress)>,
        cancel: &CancelToken,
    ) -> ElevatedRun<TransferOutcome>;

    fn run_delete(
        &self,
        retry: &DeleteRetry,
        progress: Option<&mut dyn FnMut(DeleteProgress)>,
        cancel: &CancelToken,
    ) -> ElevatedRun<DeleteOutcome>;

    fn run_rename(&self, retry: &RenameRetry, cancel: &CancelToken) -> ElevatedRun<RenameOutcome>;

    fn run_new_item(
        &self,
        retry: &NewItemRetry,
        cancel: &CancelToken,
    ) -> ElevatedRun<NewItemOutcome>;

    /// Puts back what an elevated move moved. Takes an outcome rather than a plan, because that is
    /// what `TransferExecutor::undo` takes.
    fn undo_transfer(
        &self,
        outcome: &TransferOutcome,
        cancel: &CancelToken,
    ) -> ElevatedRun<TransferUndoResult>;

    fn undo_delete(
        &self,
        outcome: &DeleteOutcome,
        cancel: &CancelToken,
    ) -> ElevatedRun<DeleteUndoResult>;
}

pub struct ElevationClient {
    launcher: Box<dyn ElevationLauncher>,
    transports: Box<dyn ElevationTransportFactory>,
    user_sid: String,
}

/// What the helper said, before it is turned back into an outcome.
#[derive(Default)]
struct Collected {
    done: Vec<String>,
    fault: Vec<String>,
    summary: Option<ElevationSummary>,
    fatal: Option<String>,
}

impl Collected {
    /// The `Done` lines that parse as `T`. One that does not is dropped rather than failing: a
    /// malformed line from the helper must not lose the ones around it.
    fn done<T: WireRecord>(&self) -> Vec<T> {
        self.done.iter().filter_map(|d| T::from_json(d)).collect()
    }

    fn faults<T: WireRecord>(&self) -> Vec<T> {
        self.fault.iter().filter_map(|f| T::from_json(f)).collect()
    }
}

impl ElevationClient {
    /// Generous: the user has to answer a prompt, and the secure desktop takes a moment.
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(120);

    /// The helper exits on its own once it has answered; this is only to notice it has.
    const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

    pub fn new(
        launcher: Box<dyn ElevationLauncher>,
        transports: Box<dyn ElevationTransportFactory>,
        user_sid: &str,
    ) -> ElevationClient {
        ElevationClient {
            launcher,
            transports,
            user_sid: user_sid.to_string(),
        }
    }

    // --- one session ---

    fn run<T>(
        &self,
        header: &ElevationHeader,
        items: Vec<String>,
        on_progress: &mut dyn FnMut(ElevationProgressReport),
        build: impl FnOnce(&Collected) -> T,
        cancel: &CancelToken,
    ) -> ElevatedRun<T> {
        if items.is_empty() {
            return ElevatedRun::unavailable("there was nothing to do");
        }
        if items.len() > ElevationProtocol::MAX_ITEMS {
            return ElevatedRun::unavailable("there are too many items to do at once");
        }
        if !self.launcher.can_elevate() {
            return ElevatedRun::not_administrator();
        }

        let mut transport = self.transports.create();
        let launch = self
            .launcher
            .launch(transport.endpoint(), std::process::id(), &self.user_sid);

        match launch.outcome {
            ElevationLaunch::Declined => return ElevatedRun::declined(),
            ElevationLaunch::NotAdministrator => return ElevatedRun::not_administrator(),
            ElevationLaunch::Failed => return ElevatedRun::unavailable(&launch.detail),
            ElevationLaunch::Started => {}
        }

        let Some(stream) = transport.accept(launch.process_id, Self::CONNECT_TIMEOUT) else {
            self.launcher
                .wait_for_exit(launch.process_id, Self::EXIT_TIMEOUT);
            return ElevatedRun::unavailable("the helper did not connect");
        };

        let collected = Self::converse(stream, header, &items, on_progress, cancel);
        self.launcher
            .wait_for_exit(launch.process_id, Self::EXIT_TIMEOUT);

        if let Some(fatal) = collected.fatal {
            return ElevatedRun::unavailable(&fatal);
        }
        if collected.summary.is_none() {
            return ElevatedRun::unavailable("the helper stopped part-way");
        }
        ElevatedRun::ran(build(&collected))
    }

    fn converse(
        stream: Box<dyn ElevationStream>,
        header: &ElevationHeader,
        items: &[String],
        on_progress: &mut dyn FnMut(ElevationProgressReport),
        cancel: &CancelToken,
    ) -> Collected {
        let mut collected = Collected::default();
        let Ok(reading) = stream.split() else {
            return collected;
        };
        let mut reader = LineReader::new(reading, NavigationRequest::MAX_LINE_LENGTH);
        let writer = Mutex::new(stream);

        let send = |verb: ElevationVerb, payload: &str| {
            let mut stream = writer.lock().unwrap();
            // A failure is the helper gone; the read below will see it too and say so once.
            let _ = write_line(
                stream.as_mut(),
                &ElevationProtocol::format(&ElevationMessage::new(verb, payload)),
            );
        };

        // Watched rather than polled between items, so a cancel lands while the helper is mid-file
        // rather than between items. The helper's executors do the rest: nothing half-written, and
        // whatever got across stays across.
        let finished = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                while !finished.load(Ordering::Acquire) {
                    if cancel.is_cancelled() {
                        send(ElevationVerb::Cancel, "");
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            });

            send(
                ElevationVerb::Hello,
                &ElevationProtocol::PROTOCOL_VERSION.to_string(),
            );

            let mut sent = false;
            #[allow(clippy::while_let_loop)]
            loop {
                let Ok(Some(line)) = reader.read_line() else {
                    break;
                };
                let Some(message) = ElevationProtocol::try_parse(&line) else {
                    continue;
                };

                match message.verb {
                    ElevationVerb::Hello
                        if ElevationProtocol::version_of(&message)
                            != Some(ElevationProtocol::PROTOCOL_VERSION) =>
                    {
                        collected.fatal = Some(
                            "the helper speaks a different version of this protocol".to_string(),
                        );
                        break;
                    }

                    ElevationVerb::Ready if !sent => {
                        sent = true;
                        send(ElevationVerb::Begin, &header.to_json());
                        for item in items {
                            send(ElevationVerb::Item, item);
                        }
                        send(ElevationVerb::Go, "");
                    }

                    ElevationVerb::Progress => {
                        if let Some(report) = ElevationProgressReport::from_json(&message.payload) {
                            on_progress(report);
                        }
                    }

                    ElevationVerb::Done => collected.done.push(message.payload),

                    ElevationVerb::Fault => collected.fault.push(message.payload),

                    ElevationVerb::End => {
                        collected.summary = ElevationSummary::from_json(&message.payload);
                        break;
                    }

                    ElevationVerb::Fatal => {
                        collected.fatal = Some(message.payload);
                        break;
                    }

                    _ => {}
                }
            }

            finished.store(true, Ordering::Release);
        });

        collected
    }

    fn resolution(resolutions: &Resolutions, source: &str) -> ConflictResolution {
        ElevationRules::key_of(source)
            .and_then(|key| resolutions.get(&key).copied())
            .unwrap_or(ConflictResolution::KeepBoth)
    }
}

impl ElevatedOperationRunner for ElevationClient {
    fn can_elevate(&self) -> bool {
        self.launcher.can_elevate()
    }

    fn run_transfer(
        &self,
        retry: &TransferRetry,
        mut progress: Option<&mut dyn FnMut(TransferProgress)>,
        cancel: &CancelToken,
    ) -> ElevatedRun<TransferOutcome> {
        let operation = match retry.plan.verb {
            TransferVerb::Move => ElevationOperation::TransferMove,
            TransferVerb::Copy => ElevationOperation::TransferCopy,
        };
        let header =
            ElevationHeader::new(operation).with_destination(&retry.plan.destination_directory);
        let items = retry
            .plan
            .transfers
            .iter()
            .map(|t| {
                ElevationTransferItem {
                    item: t.clone(),
                    resolution: Self::resolution(&retry.resolutions, &t.source_path),
                }
                .to_json()
            })
            .collect();

        self.run(
            &header,
            items,
            &mut |p| {
                if let Some(report) = progress.as_deref_mut() {
                    report(TransferProgress {
                        done: p.done,
                        total: p.total,
                        current_name: p.current_name,
                        bytes_done: p.bytes_done,
                        current_item_bytes: p.current_item_bytes,
                        current_item_total: p.current_item_total,
                    });
                }
            },
            |collected| {
                let results = collected.done::<ElevationTransferResult>();
                let summary = collected.summary.clone().unwrap_or_default();
                TransferOutcome::new(
                    retry.plan.verb,
                    &retry.plan.destination_directory,
                    results.iter().filter_map(|r| r.completed.clone()).collect(),
                    results.iter().filter_map(|r| r.skipped.clone()).collect(),
                    collected.faults::<FailedTransfer>(),
                    summary.staging_directories,
                    summary.cancelled,
                )
            },
            cancel,
        )
    }

    fn run_delete(
        &self,
        retry: &DeleteRetry,
        mut progress: Option<&mut dyn FnMut(DeleteProgress)>,
        cancel: &CancelToken,
    ) -> ElevatedRun<DeleteOutcome> {
        let header =
            ElevationHeader::new(ElevationOperation::Delete).with_delete_mode(retry.plan.mode);
        let items = retry.plan.deletions.iter().map(|d| d.to_json()).collect();

        self.run(
            &header,
            items,
            &mut |p| {
                if let Some(report) = progress.as_deref_mut() {
                    report(DeleteProgress {
                        done: p.done,
                        total: p.total,
                        current_name: p.current_name,
                    });
                }
            },
            |collected| DeleteOutcome {
                permanent: retry.plan.permanent(),
                deleted: collected.done::<DeletedItem>(),
                failed: collected.faults::<FailedDelete>(),
                staging_directories: collected
                    .summary
                    .as_ref()
                    .map(|s| s.staging_directories.clone())
                    .unwrap_or_default(),
            },
            cancel,
        )
    }

    fn run_rename(&self, retry: &RenameRetry, cancel: &CancelToken) -> ElevatedRun<RenameOutcome> {
        let header = ElevationHeader::new(ElevationOperation::Rename);
        let items = retry.plan.renames.iter().map(|r| r.to_json()).collect();

        self.run(
            &header,
            items,
            &mut |_| {},
            |collected| RenameOutcome {
                completed: collected.done::<CompletedRename>(),
                failed: collected.faults::<FailedRename>(),
            },
            cancel,
        )
    }

    fn run_new_item(
        &self,
        retry: &NewItemRetry,
        cancel: &CancelToken,
    ) -> ElevatedRun<NewItemOutcome> {
        let header = ElevationHeader::new(ElevationOperation::NewItem);
        let items = vec![retry.plan.to_json()];

        self.run(
            &header,
            items,
            &mut |_| {},
            |collected| NewItemOutcome {
                created_path: collected
                    .done::<ElevationNewItemResult>()
                    .into_iter()
                    .next()
                    .map(|r| r.created_path),
                failed: collected.faults::<FailedNewItem>().into_iter().next(),
            },
            cancel,
        )
    }

    fn undo_transfer(
        &self,
        outcome: &TransferOutcome,
        cancel: &CancelToken,
    ) -> ElevatedRun<TransferUndoResult> {
        let header = ElevationHeader::new(ElevationOperation::TransferUndo)
            .with_destination(&outcome.destination_directory);
        let items = outcome.completed.iter().map(|c| c.to_json()).collect();

        self.run(
            &header,
            items,
            &mut |_| {},
            |collected| TransferUndoResult {
                restored: collected.summary.as_ref().map(|s| s.restored).unwrap_or(0),
                failed: collected.faults::<FailedTransfer>(),
                reverted: Vec::new(),
            },
            cancel,
        )
    }

    fn undo_delete(
        &self,
        outcome: &DeleteOutcome,
        cancel: &CancelToken,
    ) -> ElevatedRun<DeleteUndoResult> {
        let header =
            ElevationHeader::new(ElevationOperation::DeleteUndo).with_permanent(outcome.permanent);
        let items = outcome.deleted.iter().map(|d| d.to_json()).collect();

        self.run(
            &header,
            items,
            &mut |_| {},
            |collected| DeleteUndoResult {
                restored: collected.summary.as_ref().map(|s| s.restored).unwrap_or(0),
                failed: collected.faults::<FailedDelete>(),
                reverted: Vec::new(),
            },
            cancel,
        )
    }
}
