//! The app's half of the split: launches the elevated indexer, mirrors what it reports, and answers
//! as if the index were running here. Spec: Core/Services/Mft/MftIndexClient.cs.
//!
//! Everything `MftIndex` exposes is answered from a local `MftIndexState` fed by pushes from the
//! helper, so nothing the UI or the search router asks ever waits on a round trip. **Nothing here
//! retries by itself**: a retry raises a UAC prompt, so a failure is a state with `can_retry` set,
//! and the user clicks. Losing the helper is not survived silently: the mirrored state is
//! *cleared*, because claiming a volume is indexed when the process that indexed it has gone would
//! route searches to a database nothing is keeping current.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use super::presence::{IndexerPresence, IndexerPresenceProbe, NoIndexerPresence};
use super::state::MftIndexState;
use super::transport::{
    IndexHostLaunch, IndexHostLauncher, IndexStream, IndexTransportFactory, StreamRead, StreamWrite,
};
use super::{IndexListener, IndexListeners, IndexStartMode, MftIndex};
use crate::cancel::CancelToken;
use crate::changes::ChangeLogPolicy;
use crate::cli::NavigationRequest;
use crate::ipc::{IndexMessage, IndexProtocol, IndexVerb, LineReader, write_line};

/// Long enough for an elevated process to start behind a UAC prompt the user has to read, short
/// enough that a helper which never arrives does not hold the status bar forever.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to look for a helper that is already running before concluding there is none. Short,
/// because a helper parked waiting for an app connects within milliseconds: this is a rendezvous,
/// not a launch. A backstop rather than the answer: the presence lock is what actually decides.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a clean shutdown waits. A courtesy, not a guarantee.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// A worker's "I have finished", waited on with a timeout since a `JoinHandle` has none.
#[derive(Default)]
struct Done {
    finished: Mutex<bool>,
    signal: Condvar,
}

impl Done {
    fn set(&self) {
        *self.finished.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.signal.notify_all();
    }

    fn wait(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut finished = self.finished.lock().unwrap_or_else(|p| p.into_inner());
        while !*finished {
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            let (guard, _) = self
                .signal
                .wait_timeout(finished, deadline - now)
                .unwrap_or_else(|p| p.into_inner());
            finished = guard;
        }
    }
}

struct Worker {
    id: ThreadId,
    done: Arc<Done>,
}

struct Gate {
    session: Option<CancelToken>,
    worker: Option<Worker>,
    stream: Option<Arc<dyn IndexStream>>,
    failure: Option<String>,
    can_retry: bool,
    /// Whether asking for a helper could produce one. Separate from `can_retry`: retrying answers
    /// a failure being reported; starting also covers the ordinary case where nothing has gone
    /// wrong and nobody has asked for an index yet, which has an empty status line.
    can_start: bool,
    disposed: bool,
    change_log: ChangeLogPolicy,
    presence: IndexerPresence,
}

struct Inner {
    launcher: Arc<dyn IndexHostLauncher>,
    transports: Arc<dyn IndexTransportFactory>,
    presence_probe: Arc<dyn IndexerPresenceProbe>,
    state: MftIndexState,
    gate: Mutex<Gate>,
    /// Serialises writes to the pipe: the policy setter sends from whichever thread changed the
    /// setting, and two lines interleaved on the wire parse as nothing.
    send_gate: Mutex<()>,
    listeners: IndexListeners,
}

pub struct MftIndexClient {
    inner: Arc<Inner>,
}

impl MftIndexClient {
    pub fn new(
        launcher: Arc<dyn IndexHostLauncher>,
        transports: Arc<dyn IndexTransportFactory>,
        presence: Option<Arc<dyn IndexerPresenceProbe>>,
    ) -> MftIndexClient {
        MftIndexClient {
            inner: Arc::new(Inner {
                launcher,
                transports,
                presence_probe: presence.unwrap_or_else(|| Arc::new(NoIndexerPresence)),
                state: MftIndexState::new(),
                gate: Mutex::new(Gate {
                    session: None,
                    worker: None,
                    stream: None,
                    failure: None,
                    can_retry: false,
                    can_start: false,
                    disposed: false,
                    change_log: ChangeLogPolicy::OFF,
                    presence: IndexerPresence::NotRunning,
                }),
                send_gate: Mutex::new(()),
                listeners: IndexListeners::new(),
            }),
        }
    }

    /// Lets go of the helper without ending it. **This used to send `Shutdown`, and that line was
    /// what killed the helper every time the app closed.** The helper is meant to outlive the app,
    /// so closing a window hangs up and nothing more. Nothing here could force the point anyway: a
    /// medium-integrity process may not open a high-integrity one for `PROCESS_TERMINATE`.
    pub fn dispose(&self) {
        {
            let mut gate = self.inner.gate();
            if gate.disposed {
                return;
            }
            gate.disposed = true;
        }
        self.inner.end_session();
    }
}

impl Drop for MftIndexClient {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl Inner {
    fn gate(&self) -> std::sync::MutexGuard<'_, Gate> {
        self.gate.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn record_message(policy: ChangeLogPolicy) -> IndexMessage {
        IndexMessage::new(IndexVerb::Record, &policy.to_hours().to_string())
    }

    fn run_session(self: &Arc<Inner>, mode: IndexStartMode, cancel: CancelToken, done: Arc<Done>) {
        self.session_body(mode, &cancel);
        // The transport is emphatically not disposed here. It is the app's one endpoint and has
        // to stay listening between sessions, or a helper started after this attempt would have
        // nothing to connect to. The factory owns its lifetime.
        self.retire_session();
        done.set();
    }

    fn session_body(&self, mode: IndexStartMode, cancel: &CancelToken) {
        let transport = match self.transports.try_create() {
            Ok(transport) => transport,
            Err(why_not) => {
                // Another copy of this app owns the endpoint. A prompt could not help, so this is
                // deliberately not retryable-by-prompting.
                self.settle(
                    IndexerPresence::NotRunning,
                    Some(Self::unavailable(&why_not)),
                    false,
                );
                return;
            }
        };

        // Attaching costs nothing and prompts nobody, so it comes before every other question,
        // including whether this account could elevate. A standard user who somehow has a helper
        // running should still get to use it.
        if let Some(attached) = transport.accept(None, ATTACH_TIMEOUT) {
            self.gate().stream = Some(attached.clone());
            self.converse(attached, None, cancel);
            return;
        }

        if self.presence_probe.is_running() {
            // A helper is there but would not talk to us. Starting another cannot work (its own
            // single-instance guard would refuse), so the prompt is withheld and the state is
            // reported as what it is.
            self.settle(
                IndexerPresence::Running,
                Some(
                    "Search index unavailable — the index helper is running but not responding."
                        .to_string(),
                ),
                false,
            );
            return;
        }

        if mode == IndexStartMode::AttachOnly {
            // Nothing is running and nobody asked for one. Say nothing in the status bar; the
            // banner is what speaks now, and two voices saying it is worse than one.
            self.settle(
                IndexerPresence::NotRunning,
                None,
                self.launcher.can_elevate(),
            );
            return;
        }

        if !self.launcher.can_elevate() {
            // Deliberately without prompting: see IndexHostLauncher::can_elevate.
            self.fail(
                "Search index off — this account is not an administrator.",
                false,
            );
            return;
        }

        let launch = self.launcher.launch();
        match launch.outcome {
            IndexHostLaunch::Declined => {
                self.fail("Search index off — permission declined.", true);
                return;
            }
            IndexHostLaunch::NotAdministrator => {
                self.fail(
                    "Search index off — this account is not an administrator.",
                    false,
                );
                return;
            }
            IndexHostLaunch::Failed => {
                self.fail(&Self::unavailable(&launch.detail), true);
                return;
            }
            IndexHostLaunch::Started => {}
        }

        let Some(stream) = transport.accept(Some(launch.process_id), CONNECT_TIMEOUT) else {
            self.fail("Search index unavailable.", true);
            return;
        };
        self.gate().stream = Some(stream.clone());
        self.converse(stream, Some(launch.process_id), cancel);
    }

    /// Lets go of the session this thread was running, so a later `start` can begin a new one.
    /// **Only if the fields still point at *this* thread.** `end_session` may already have replaced
    /// them with a newer session's, and clearing those would leave a live worker nothing can reach
    /// and a `start` that begins a second one beside it.
    fn retire_session(&self) {
        let mut gate = self.gate();
        gate.stream = None;
        if gate
            .worker
            .as_ref()
            .is_some_and(|w| w.id == thread::current().id())
        {
            gate.worker = None;
            gate.session = None;
        }
    }

    fn converse(
        &self,
        stream: Arc<dyn IndexStream>,
        launched_process_id: Option<u32>,
        cancel: &CancelToken,
    ) {
        let mut reader = LineReader::new(
            StreamRead(stream.clone()),
            NavigationRequest::MAX_LINE_LENGTH,
        );

        {
            let mut gate = self.gate();
            gate.presence = IndexerPresence::Running;
            gate.can_retry = false;
            gate.can_start = false;
            gate.failure = None;
        }
        self.send(
            stream.as_ref(),
            &IndexMessage::new(
                IndexVerb::Hello,
                &IndexProtocol::PROTOCOL_VERSION.to_string(),
            ),
        );

        let mut started = false;
        while !cancel.is_cancelled() {
            // The helper is gone (or the pipe broke, which is the same thing). See the module doc.
            let Ok(Some(line)) = reader.read_line() else {
                break;
            };
            // One malformed message must not end the session.
            let Some(message) = IndexProtocol::try_parse(&line) else {
                continue;
            };
            match message.verb {
                IndexVerb::Hello => {
                    if IndexProtocol::version_of(&message) != Some(IndexProtocol::PROTOCOL_VERSION)
                    {
                        // A helper left over from a previous build of the app, still holding the
                        // endpoint. Mirroring state from something that means something else by
                        // it is the worse option, and since it will not exit on its own while it
                        // has an index to keep, ask it to.
                        self.send(stream.as_ref(), &IndexMessage::bare(IndexVerb::Shutdown));
                        self.fail(
                            "Search index unavailable — restart to update the index helper.",
                            true,
                        );
                        return;
                    }
                }
                IndexVerb::Ready if !started => {
                    started = true;
                    self.send(stream.as_ref(), &IndexMessage::bare(IndexVerb::Start));
                    // Every session, even when it is the default: a retried helper is a fresh
                    // process whose own default is off, and the user's setting may not be.
                    let policy = self.gate().change_log;
                    self.send(stream.as_ref(), &Self::record_message(policy));
                }
                IndexVerb::Building => {
                    self.state.mark_building(&message.argument);
                    self.announce();
                }
                IndexVerb::Idle => {
                    self.state.clear_building(&message.argument);
                    self.announce();
                }
                IndexVerb::Complete => {
                    self.state.mark_complete(&message.argument);
                    self.announce();
                    self.listeners.index_refreshed(&message.argument);
                }
                IndexVerb::Fatal => {
                    self.fail(&message.argument, true);
                    return;
                }
                IndexVerb::Ping => self.send(stream.as_ref(), &IndexMessage::bare(IndexVerb::Pong)),
                _ => {}
            }
        }

        if !cancel.is_cancelled() {
            self.fail("Search index stopped.", true);
        }

        // Only a helper this app launched can be waited on; an attached one has no handle here.
        if let Some(process_id) = launched_process_id {
            self.launcher.wait_for_exit(process_id, SHUTDOWN_TIMEOUT);
        }
    }

    fn send(&self, stream: &dyn IndexStream, message: &IndexMessage) {
        let _serialised = self.send_gate.lock().unwrap_or_else(|p| p.into_inner());
        // A failed write means the reader will see the same break and end the session.
        let _ = write_line(&mut StreamWrite(stream), &IndexProtocol::format(message));
    }

    /// A launch failure the user did not choose, said as specifically as the launcher can manage.
    /// The status bar is the only place this can be said, and "unavailable" on its own sends the
    /// reader to a debugger. The details are lowercase fragments that compose into this line,
    /// except the one that is an OS message quoted verbatim, which is why the terminator is
    /// trimmed rather than assumed absent.
    fn unavailable(detail: &str) -> String {
        let detail = detail.trim().trim_end_matches('.');
        if detail.is_empty() {
            "Search index unavailable.".to_string()
        } else {
            format!("Search index unavailable — {detail}.")
        }
    }

    fn fail(&self, status: &str, can_retry: bool) {
        self.state.clear();
        {
            let mut gate = self.gate();
            gate.failure = Some(status.to_string());
            gate.can_retry = can_retry;
            // A failure worth retrying is a failure a start could answer.
            gate.can_start = can_retry;
            gate.presence = IndexerPresence::NotRunning;
        }
        self.listeners.status_changed();
    }

    /// Records that there is no helper, without calling it a failure. **The difference from
    /// `fail` is the empty status line, and it is the whole point.** "No helper is running because
    /// nobody has asked for one" is the ordinary state of a freshly launched app, not an error.
    fn settle(&self, presence: IndexerPresence, failure: Option<String>, can_start: bool) {
        self.state.clear();
        {
            let mut gate = self.gate();
            // Only a reported failure may offer to be retried. Nothing running because nobody has
            // asked is not a failure, has no status line, and must not put a "try again" beside an
            // empty one.
            gate.can_retry = failure.is_some() && can_start;
            gate.failure = failure;
            gate.can_start = can_start;
            gate.presence = presence;
        }
        self.listeners.status_changed();
    }

    fn announce(&self) {
        // A helper reporting progress is a working helper: nothing to retry, nothing to start.
        {
            let mut gate = self.gate();
            gate.failure = None;
            gate.can_retry = false;
            gate.can_start = false;
        }
        self.listeners.status_changed();
    }

    fn end_session(&self) {
        let (session, worker, stream) = {
            let mut gate = self.gate();
            (gate.session.take(), gate.worker.take(), gate.stream.clone())
        };
        if let Some(session) = &session {
            session.cancel();
        }
        // The worker is parked in a blocking read; closing the stream is what returns it.
        if let Some(stream) = stream {
            stream.close();
        }
        if let Some(worker) = worker {
            worker.done.wait(SHUTDOWN_TIMEOUT);
        }
    }
}

impl MftIndex for MftIndexClient {
    /// Looks for a helper, and, depending on `mode`, starts one if there is none. One entry point
    /// rather than a separate attach, because the guard is on a session being in flight: an attach
    /// followed by a start would find a worker already running and quietly do nothing.
    fn start_with(&self, mode: IndexStartMode) -> Result<(), String> {
        let mut gate = self.inner.gate();
        if gate.disposed || gate.worker.is_some() {
            return Ok(());
        }
        // Captured rather than re-read after the lock: a dispose racing this would otherwise null
        // the field and leave a session that never starts.
        let cancel = CancelToken::new();
        let done = Arc::new(Done::default());
        let worker_inner = self.inner.clone();
        let (worker_cancel, worker_done) = (cancel.clone(), done.clone());
        let handle = thread::Builder::new()
            .name("bertbrowser index client".to_string())
            .spawn(move || worker_inner.run_session(mode, worker_cancel, worker_done))
            .map_err(|e| e.to_string())?;
        // The worker cannot reach the gate until this lock is released, so its id is on record
        // before `retire_session` compares against it.
        gate.session = Some(cancel);
        gate.worker = Some(Worker {
            id: handle.thread().id(),
            done,
        });
        Ok(())
    }

    /// Asks the helper to stop, and waits briefly for it to. The only way an app ends a helper,
    /// and therefore the only thing behind the Stop button in Settings. `dispose` does not do this.
    fn stop(&self) {
        let stream = self.inner.gate().stream.clone();
        if let Some(stream) = stream {
            self.inner
                .send(stream.as_ref(), &IndexMessage::bare(IndexVerb::Shutdown));
        }
        self.inner.end_session();
        self.inner.settle(
            IndexerPresence::NotRunning,
            None,
            self.inner.launcher.can_elevate(),
        );
    }

    fn any_indexed(&self) -> bool {
        self.inner.state.any_indexed()
    }

    fn is_building(&self) -> bool {
        self.inner.state.is_building()
    }

    fn building_drives(&self) -> Vec<String> {
        self.inner.state.building_drives()
    }

    fn completed_roots(&self) -> Vec<String> {
        self.inner.state.completed_roots()
    }

    fn presence(&self) -> IndexerPresence {
        self.inner.gate().presence
    }

    fn is_indexed(&self, path_key: &str) -> bool {
        self.inner.state.is_indexed(path_key)
    }

    /// The wording is `MftIndexState`'s, the same function the in-process indexer uses, so the two
    /// can never phrase the same state differently.
    fn status_text(&self) -> String {
        let gate = self.inner.gate();
        gate.failure
            .clone()
            .unwrap_or_else(|| self.inner.state.format_status())
    }

    fn can_retry(&self) -> bool {
        self.inner.gate().can_retry
    }

    fn can_start(&self) -> bool {
        self.inner.gate().can_start
    }

    fn retry(&self) {
        {
            let mut gate = self.inner.gate();
            if gate.disposed || !gate.can_retry {
                return;
            }
            gate.can_retry = false;
            gate.failure = None;
        }
        self.inner.end_session();
        let _ = self.start_with(IndexStartMode::AttachOrLaunch);
    }

    fn change_log(&self) -> ChangeLogPolicy {
        self.inner.gate().change_log
    }

    /// Relayed to the helper the moment it is set, if one is up, and again after every Start. The
    /// helper is never trusted to remember it across sessions.
    fn set_change_log(&self, policy: ChangeLogPolicy) {
        let stream = {
            let mut gate = self.inner.gate();
            gate.change_log = policy;
            gate.stream.clone()
        };
        if let Some(stream) = stream {
            self.inner
                .send(stream.as_ref(), &Inner::record_message(policy));
        }
    }

    fn subscribe(&self, listener: Arc<dyn IndexListener>) -> u64 {
        self.inner.listeners.subscribe(listener)
    }

    fn unsubscribe(&self, id: u64) {
        self.inner.listeners.unsubscribe(id);
    }
}
