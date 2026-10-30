//! A blocking in-memory stream pair, so both halves of the index protocol can be driven without a
//! real pipe, and therefore without a real elevation prompt, which no test can ever answer. Plus
//! the fakes the client's state machine is tested against. Port of
//! tests/BertBrowser.Core.Tests/PipeFakes.cs (the index half; the elevation peer is the Elevation
//! area's).

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use bb_core::changes::ChangeLogPolicy;
use bb_core::ipc::{IndexMessage, IndexProtocol, IndexVerb, LineReader, write_line};
use bb_core::mft::{
    IndexHostLaunchResult, IndexHostLauncher, IndexListener, IndexListeners, IndexStartMode,
    IndexStream, IndexTransport, IndexTransportFactory, IndexerPresence, MftIndex, MftIndexState,
    StreamRead, StreamWrite,
};

/// One direction: writes append, reads block until there is something or it is closed.
pub struct BlockingStreamBuffer {
    state: Mutex<(VecDeque<u8>, bool)>,
    signal: Condvar,
}

impl BlockingStreamBuffer {
    pub fn new() -> BlockingStreamBuffer {
        BlockingStreamBuffer {
            state: Mutex::new((VecDeque::new(), false)),
            signal: Condvar::new(),
        }
    }

    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.1 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "The pipe has been closed.",
            ));
        }
        state.0.extend(bytes.iter().copied());
        self.signal.notify_all();
        Ok(())
    }

    pub fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        while state.0.is_empty() && !state.1 {
            state = self.signal.wait(state).unwrap_or_else(|p| p.into_inner());
        }
        let mut read = 0;
        while read < buffer.len() {
            match state.0.pop_front() {
                Some(b) => {
                    buffer[read] = b;
                    read += 1;
                }
                None => break,
            }
        }
        Ok(read) // 0 only once closed and drained, which is end-of-stream.
    }

    pub fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.1 = true;
        self.signal.notify_all();
    }
}

impl Default for BlockingStreamBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// One end of a pair. Closing both directions is what a broken pipe looks like to the other end.
pub struct DuplexStream {
    input: Arc<BlockingStreamBuffer>,
    output: Arc<BlockingStreamBuffer>,
}

impl IndexStream for DuplexStream {
    fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        self.input.read(buffer)
    }

    fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        self.output.write(bytes)
    }

    fn close(&self) {
        self.input.close();
        self.output.close();
    }
}

pub struct DuplexPair;

impl DuplexPair {
    pub fn create() -> (Arc<dyn IndexStream>, Arc<dyn IndexStream>) {
        let left_to_right = Arc::new(BlockingStreamBuffer::new());
        let right_to_left = Arc::new(BlockingStreamBuffer::new());
        (
            Arc::new(DuplexStream {
                input: right_to_left.clone(),
                output: left_to_right.clone(),
            }),
            Arc::new(DuplexStream {
                input: left_to_right,
                output: right_to_left,
            }),
        )
    }
}

/// Reads and writes protocol messages on the far end of a `DuplexPair`.
pub struct ProtocolPeer {
    stream: Arc<dyn IndexStream>,
    reader: Mutex<LineReader<StreamRead>>,
}

impl ProtocolPeer {
    pub fn new(stream: Arc<dyn IndexStream>) -> ProtocolPeer {
        ProtocolPeer {
            reader: Mutex::new(LineReader::new(StreamRead(stream.clone()), 64 * 1024)),
            stream,
        }
    }

    pub fn send(&self, verb: IndexVerb, argument: &str) {
        let _ = write_line(
            &mut StreamWrite(self.stream.as_ref()),
            &IndexProtocol::format(&IndexMessage::new(verb, argument)),
        );
    }

    /// Sends a line the protocol will refuse, to prove a session survives one.
    pub fn send_raw(&self, line: &str) {
        let _ = write_line(&mut StreamWrite(self.stream.as_ref()), line);
    }

    /// The next parseable message, skipping anything malformed. None at end of stream.
    pub fn receive(&self) -> Option<IndexMessage> {
        let mut reader = self.reader.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            let line = reader.read_line().ok()??;
            if let Some(message) = IndexProtocol::try_parse(&line) {
                return Some(message);
            }
        }
    }

    /// The next message with one of `verbs`, skipping the others.
    pub fn receive_one_of(&self, verbs: &[IndexVerb]) -> Option<IndexMessage> {
        while let Some(message) = self.receive() {
            if verbs.contains(&message.verb) {
                return Some(message);
            }
        }
        None
    }

    pub fn close(&self) {
        self.stream.close();
    }
}

/// An `IndexHostLauncher` that starts nothing and reports what it was told to.
pub struct FakeIndexHostLauncher {
    result: IndexHostLaunchResult,
    launches: AtomicUsize,
    can_elevate: bool,
}

impl FakeIndexHostLauncher {
    pub fn new(result: IndexHostLaunchResult) -> FakeIndexHostLauncher {
        FakeIndexHostLauncher {
            result,
            launches: AtomicUsize::new(0),
            can_elevate: true,
        }
    }

    pub fn cannot_elevate(mut self) -> FakeIndexHostLauncher {
        self.can_elevate = false;
        self
    }

    pub fn launches(&self) -> usize {
        self.launches.load(Ordering::SeqCst)
    }
}

impl IndexHostLauncher for FakeIndexHostLauncher {
    fn launch(&self) -> IndexHostLaunchResult {
        self.launches.fetch_add(1, Ordering::SeqCst);
        self.result.clone()
    }

    fn can_elevate(&self) -> bool {
        self.can_elevate
    }

    fn wait_for_exit(&self, _process_id: u32, _timeout: Duration) {}
}

type Accept = Box<dyn Fn(Option<u32>) -> Option<Arc<dyn IndexStream>> + Send + Sync>;

/// Hands the client one end of an already-connected pair. The closure is told whether this is the
/// *attach* look (no launched process id) or the one after a launch, so a test can make a helper
/// appear only in the case it is about.
pub struct FakeIndexTransportFactory {
    inner: Arc<FactoryInner>,
}

struct FactoryInner {
    accept: Accept,
    error: Option<String>,
    created: AtomicUsize,
    attaches: AtomicUsize,
    transport: Mutex<Option<Arc<FakeIndexTransport>>>,
}

impl FakeIndexTransportFactory {
    pub fn new(
        accept: impl Fn(Option<u32>) -> Option<Arc<dyn IndexStream>> + Send + Sync + 'static,
    ) -> FakeIndexTransportFactory {
        FakeIndexTransportFactory {
            inner: Arc::new(FactoryInner {
                accept: Box::new(accept),
                error: None,
                created: AtomicUsize::new(0),
                attaches: AtomicUsize::new(0),
                transport: Mutex::new(None),
            }),
        }
    }

    /// The same stream whichever look it is.
    pub fn always(stream: Option<Arc<dyn IndexStream>>) -> FakeIndexTransportFactory {
        FakeIndexTransportFactory::new(move |_| stream.clone())
    }

    /// A factory that cannot make an endpoint: another copy of the app owns it.
    pub fn unavailable(error: &str) -> FakeIndexTransportFactory {
        FakeIndexTransportFactory {
            inner: Arc::new(FactoryInner {
                accept: Box::new(|_| None),
                error: Some(error.to_string()),
                created: AtomicUsize::new(0),
                attaches: AtomicUsize::new(0),
                transport: Mutex::new(None),
            }),
        }
    }

    pub fn created(&self) -> usize {
        self.inner.created.load(Ordering::SeqCst)
    }

    /// How many times a session looked for an already-running helper.
    pub fn attaches(&self) -> usize {
        self.inner.attaches.load(Ordering::SeqCst)
    }
}

impl IndexTransportFactory for FakeIndexTransportFactory {
    fn try_create(&self) -> Result<Arc<dyn IndexTransport>, String> {
        if let Some(error) = &self.inner.error {
            return Err(error.clone());
        }
        let mut slot = self
            .inner
            .transport
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            self.inner.created.fetch_add(1, Ordering::SeqCst);
            *slot = Some(Arc::new(FakeIndexTransport {
                owner: self.inner.clone(),
            }));
        }
        Ok(slot.clone().expect("just created"))
    }
}

struct FakeIndexTransport {
    owner: Arc<FactoryInner>,
}

impl IndexTransport for FakeIndexTransport {
    fn endpoint(&self) -> String {
        "BertBrowser.Index.Test".to_string()
    }

    fn accept(
        &self,
        launched_process_id: Option<u32>,
        _timeout: Duration,
    ) -> Option<Arc<dyn IndexStream>> {
        if launched_process_id.is_none() {
            self.owner.attaches.fetch_add(1, Ordering::SeqCst);
        }
        (self.owner.accept)(launched_process_id)
    }
}

/// An index service a test drives by hand, standing in for a real volume walk.
pub struct ControllableIndexService {
    state: MftIndexState,
    starts: AtomicUsize,
    start_fails_with: Option<String>,
    change_log: Mutex<ChangeLogPolicy>,
    listeners: IndexListeners,
}

impl ControllableIndexService {
    pub fn new() -> ControllableIndexService {
        ControllableIndexService {
            state: MftIndexState::new(),
            starts: AtomicUsize::new(0),
            start_fails_with: None,
            change_log: Mutex::new(ChangeLogPolicy::OFF),
            listeners: IndexListeners::new(),
        }
    }

    /// What the C# `StartThrows` was: the message `start` fails with.
    pub fn start_fails_with(message: &str) -> ControllableIndexService {
        let mut service = ControllableIndexService::new();
        service.start_fails_with = Some(message.to_string());
        service
    }

    pub fn starts(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    pub fn begin_building(&self, drive: &str) {
        self.state.mark_building(drive);
        self.listeners.status_changed();
    }

    pub fn finish_building(&self, drive: &str, root_key: &str) {
        self.state.mark_complete(root_key);
        self.state.clear_building(drive);
        self.listeners.status_changed();
        self.listeners.index_refreshed(root_key);
    }

    pub fn abandon_building(&self, drive: &str) {
        self.state.clear_building(drive);
        self.listeners.status_changed();
    }
}

impl Default for ControllableIndexService {
    fn default() -> Self {
        Self::new()
    }
}

impl MftIndex for ControllableIndexService {
    fn start_with(&self, _mode: IndexStartMode) -> Result<(), String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        match &self.start_fails_with {
            Some(message) => Err(message.clone()),
            None => Ok(()),
        }
    }

    fn stop(&self) {}

    fn any_indexed(&self) -> bool {
        self.state.any_indexed()
    }

    fn is_building(&self) -> bool {
        self.state.is_building()
    }

    fn building_drives(&self) -> Vec<String> {
        self.state.building_drives()
    }

    fn completed_roots(&self) -> Vec<String> {
        self.state.completed_roots()
    }

    fn presence(&self) -> IndexerPresence {
        IndexerPresence::NotApplicable
    }

    fn is_indexed(&self, path_key: &str) -> bool {
        self.state.is_indexed(path_key)
    }

    fn status_text(&self) -> String {
        self.state.format_status()
    }

    fn can_retry(&self) -> bool {
        false
    }

    fn can_start(&self) -> bool {
        false
    }

    fn retry(&self) {}

    fn change_log(&self) -> ChangeLogPolicy {
        *self.change_log.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn set_change_log(&self, policy: ChangeLogPolicy) {
        *self.change_log.lock().unwrap_or_else(|p| p.into_inner()) = policy;
    }

    fn subscribe(&self, listener: Arc<dyn IndexListener>) -> u64 {
        self.listeners.subscribe(listener)
    }

    fn unsubscribe(&self, id: u64) {
        self.listeners.unsubscribe(id);
    }
}
