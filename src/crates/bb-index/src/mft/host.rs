//! The elevated process's half: runs a real index service and reports it down the pipe, taking
//! five verbs and nothing else. Spec: Core/Services/Mft/MftIndexHost.cs.
//!
//! What this accepts is the security surface of the index design: `Hello`, `Start`, `Shutdown`,
//! `Ping` and `Record`, none of which names a file, a folder or a program. That matters more now
//! that the process outlives every app and can be started at sign-in with no supervising parent.
//!
//! **One host, many sessions.** Losing the pipe ends the *session*, not the process: the index and
//! its USN tail keep running and the next app attaches. What ends the process is `Shutdown`,
//! signing out, or the self-checks in the exe. Because an app can attach long after volumes
//! finished building, every session opens by *replaying* what is already known: `index_refreshed`
//! fired for those volumes before this app existed and will not fire again. `Ready` therefore means
//! "I have told you everything I know", and is sent last.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::transport::{IndexStream, StreamRead, StreamWrite};
use super::usage::ProcessUsageProbe;
use super::{IndexListener, MftIndex};
use crate::changes::ChangeLogPolicy;
use crate::ipc::{IndexMessage, IndexProtocol, IndexVerb, LineReader, write_line};
use bb_base::cancel::CancelToken;
use bb_base::cli::NavigationRequest;

/// How a session ended, and therefore whether the process should carry on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexSessionEnd {
    /// The app went away. Another one may arrive; the index keeps running.
    PeerGone,
    /// An app asked this process to stop, or it cannot continue.
    Stop,
}

struct Session {
    /// The stream of the session in progress, or None between sessions. A volume thread's status
    /// change can fire at any moment, including while nothing is attached; dropping those is safe
    /// because the next session replays the whole picture.
    stream: Option<Arc<dyn IndexStream>>,
    /// Each drive this app has been told is building, and the progress it was last told.
    reported_building: HashMap<String, Option<u8>>,
}

struct Inner {
    index: Arc<dyn MftIndex>,
    write_gate: Mutex<Session>,
    /// Whether indexing has been asked for. Shared across sessions: a second app's `Start` must
    /// not start a second set of volume threads. The service's own start is idempotent too; both,
    /// because this is the local guarantee and losing it silently would be hard to notice.
    started: AtomicBool,
    /// What reads this process's own counters, and how often a session is told them. None in the
    /// tests that are not about usage, so their sessions see only the lines they assert on.
    usage: Option<(Arc<dyn ProcessUsageProbe>, Duration)>,
}

pub struct MftIndexHost {
    inner: Arc<Inner>,
}

impl MftIndexHost {
    pub fn new(index: Arc<dyn MftIndex>) -> MftIndexHost {
        MftIndexHost::build(index, None)
    }

    /// As `new`, also telling every attached app what this process costs (`Usage`): once as soon
    /// as the session is `Ready`, then every `interval` until it ends. Nothing is sampled while no
    /// app is attached.
    pub fn with_usage(
        index: Arc<dyn MftIndex>,
        probe: Arc<dyn ProcessUsageProbe>,
        interval: Duration,
    ) -> MftIndexHost {
        MftIndexHost::build(index, Some((probe, interval)))
    }

    fn build(
        index: Arc<dyn MftIndex>,
        usage: Option<(Arc<dyn ProcessUsageProbe>, Duration)>,
    ) -> MftIndexHost {
        MftIndexHost {
            inner: Arc::new(Inner {
                index,
                write_gate: Mutex::new(Session {
                    stream: None,
                    reported_building: HashMap::new(),
                }),
                started: AtomicBool::new(false),
                usage,
            }),
        }
    }

    /// Talks over one connection until the app goes away or asks this process to stop.
    pub fn run(&self, stream: Arc<dyn IndexStream>, cancel: &CancelToken) -> IndexSessionEnd {
        let inner = &self.inner;
        let mut reader = LineReader::new(
            StreamRead(stream.clone()),
            NavigationRequest::MAX_LINE_LENGTH,
        );

        {
            let mut session = inner.session();
            session.stream = Some(stream.clone());
            // Forget what the previous app was told; this one has heard nothing.
            session.reported_building.clear();
        }

        let subscription = inner.index.subscribe(inner.clone());
        inner.open();
        let ticker = inner
            .usage
            .clone()
            .map(|(probe, interval)| UsageTicker::start(inner.clone(), probe, interval));
        let end = inner.converse(&mut reader, cancel);
        // Stopped before the stream is let go, so no sample is written into the next session.
        drop(ticker);
        inner.index.unsubscribe(subscription);
        inner.session().stream = None;
        end
    }
}

impl Inner {
    fn session(&self) -> std::sync::MutexGuard<'_, Session> {
        self.write_gate.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The session's opening: the version, everything already known, then `Ready`.
    fn open(&self) {
        self.send(&IndexMessage::new(
            IndexVerb::Hello,
            &IndexProtocol::PROTOCOL_VERSION.to_string(),
        ));
        self.replay();
        self.send(&IndexMessage::bare(IndexVerb::Ready));
    }

    fn converse(
        &self,
        reader: &mut LineReader<StreamRead>,
        cancel: &CancelToken,
    ) -> IndexSessionEnd {
        while !cancel.is_cancelled() {
            // A broken pipe is the ordinary way an app goes away.
            let Ok(Some(line)) = reader.read_line() else {
                return IndexSessionEnd::PeerGone;
            };
            let Some(message) = IndexProtocol::try_parse(&line) else {
                continue;
            };
            match message.verb {
                IndexVerb::Hello => {
                    if IndexProtocol::version_of(&message) != Some(IndexProtocol::PROTOCOL_VERSION)
                    {
                        self.send(&IndexMessage::new(IndexVerb::Fatal, "Version mismatch."));
                        return IndexSessionEnd::Stop;
                    }
                }
                IndexVerb::Start => {
                    if !self.started.swap(true, Ordering::SeqCst) && !self.start_indexing() {
                        return IndexSessionEnd::Stop;
                    }
                }
                IndexVerb::Ping => self.send(&IndexMessage::bare(IndexVerb::Pong)),
                IndexVerb::Record => {
                    // try_parse admitted only the menu, so this cannot fail.
                    if let Some(policy) = message
                        .argument
                        .parse::<i32>()
                        .ok()
                        .and_then(ChangeLogPolicy::from_hours)
                    {
                        self.index.set_change_log(policy);
                    }
                }
                IndexVerb::Shutdown => return IndexSessionEnd::Stop,
                // Anything else is the app speaking the helper's half of the protocol back at it.
                // Ignored rather than answered.
                _ => {}
            }
        }
        IndexSessionEnd::Stop
    }

    /// Brings a freshly attached app up to date on everything that happened before it existed.
    /// The building set and its progress need no code of their own: `status_changed` already diffs
    /// the service's drives against `reported_building`, which the session just cleared, so one call
    /// announces all of them and re-seeds the map.
    fn replay(&self) {
        self.status_changed();
        for root in self.index.completed_roots() {
            if IndexProtocol::is_acceptable_root_key(&root) {
                self.send(&IndexMessage::new(IndexVerb::Complete, &root));
            }
        }
    }

    /// The app cannot see this process's exceptions, so an indexer that cannot run has to say so on
    /// the wire or it looks exactly like one that is merely slow. False when it could not start.
    fn start_indexing(&self) -> bool {
        match self.index.start() {
            Ok(()) => true,
            Err(message) => {
                self.send(&IndexMessage::new(
                    IndexVerb::Fatal,
                    &Self::summarize(&message),
                ));
                false
            }
        }
    }

    fn send(&self, message: &IndexMessage) {
        Self::send_locked(&self.session(), message);
    }

    /// Between sessions there is nobody to tell, and the next session replays anyway. A write
    /// that fails means the app has gone; the read loop is about to see the same thing.
    fn send_locked(session: &Session, message: &IndexMessage) {
        if let Some(stream) = &session.stream {
            let line = IndexProtocol::format(message);
            let _ = write_line(&mut StreamWrite(stream.as_ref()), &line);
        }
    }

    fn truncate(text: &str) -> String {
        text.chars()
            .take(IndexProtocol::MAX_STATUS_LENGTH)
            .collect()
    }

    /// One line, no control characters, whatever the failure's own message contains.
    fn summarize(message: &str) -> String {
        let clean: String = message
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let clean = clean.trim();
        if clean.is_empty() {
            "The search index could not start.".to_string()
        } else {
            Self::truncate(clean)
        }
    }
}

/// Sends `Usage` for one session on a thread of its own. Dropping it stops the thread and waits for
/// it, so no sample outlives the session it was meant for.
struct UsageTicker {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl UsageTicker {
    fn start(
        inner: Arc<Inner>,
        probe: Arc<dyn ProcessUsageProbe>,
        interval: Duration,
    ) -> UsageTicker {
        let (stop, stopped) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("bb-index-usage".into())
            .spawn(move || {
                loop {
                    if let Some(sample) = probe.sample() {
                        inner.send(&IndexMessage::new(IndexVerb::Usage, &sample.to_argument()));
                    }
                    match stopped.recv_timeout(interval) {
                        Err(RecvTimeoutError::Timeout) => continue,
                        // Asked to stop, or the ticker was dropped: either way the session is over.
                        _ => break,
                    }
                }
            })
            .ok();
        UsageTicker {
            stop: Some(stop),
            thread,
        }
    }
}

impl Drop for UsageTicker {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl IndexListener for Inner {
    fn index_refreshed(&self, root_key: &str) {
        if IndexProtocol::is_acceptable_root_key(root_key) {
            self.send(&IndexMessage::new(IndexVerb::Complete, root_key));
        }
    }

    /// Relays which drives are building and how far along, one message per change. The facts are
    /// sent, never the status *text*: the client holds the same `MftIndexState` and formats the line
    /// with the same function, so there is one place that decides how "Indexing C:, D:…" is worded.
    fn status_changed(&self) {
        // Read and sent under the gate, so two status changes cannot interleave their lines on the
        // wire, nor a volume thread send a percentage older than the one another just sent.
        let mut session = self.session();
        // Anything the protocol would not carry is dropped here rather than tracked and then
        // silently never sent, which would leave this side believing it had reported it.
        let mut current: Vec<(String, Option<u8>)> = self
            .index
            .building_drives()
            .into_iter()
            .filter(|d| IndexProtocol::is_acceptable_drive(d))
            .map(|d| {
                let percent = self.index.drive_progress(&d);
                (d, percent)
            })
            .collect();
        current.sort();
        for (drive, percent) in &current {
            let reported = session.reported_building.insert(drive.clone(), *percent);
            if reported.is_none() {
                Self::send_locked(&session, &IndexMessage::new(IndexVerb::Building, drive));
            }
            if reported.flatten() != *percent {
                let argument = IndexProtocol::format_progress(drive, *percent);
                Self::send_locked(&session, &IndexMessage::new(IndexVerb::Progress, &argument));
            }
        }
        let mut removed: Vec<String> = session
            .reported_building
            .keys()
            .filter(|d| !current.iter().any(|(drive, _)| drive == *d))
            .cloned()
            .collect();
        removed.sort();
        for drive in removed {
            session.reported_building.remove(&drive);
            Self::send_locked(&session, &IndexMessage::new(IndexVerb::Idle, &drive));
        }
    }
}
