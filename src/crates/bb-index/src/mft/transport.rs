//! The seams the client's state machine is tested through: the launcher (a UAC prompt no test can
//! drive), the transport (the app's one listening endpoint) and the stream a session runs over.
//! Spec: Core/Services/Mft/IIndexHostLauncher.cs, IIndexTransport.cs.
//!
//! **The app listens and the helper connects, not the other way round.** A named pipe created by a
//! high-integrity process carries a High mandatory label, and mandatory policy is no-write-up, so a
//! medium-integrity client could not write to it. Creating the pipe on the medium side makes the
//! helper's connection a write-*down*, always permitted. It is also why a helper that outlives the
//! app is still not a server: nothing may connect *to* it.
//!
//! The real `IndexTransport` (the named-pipe server with its DACL, `GetNamedPipeClientProcessId`
//! and `PeerIntegrity` checks) is the App's, like `NamedPipeIndexTransport.cs`; the helper's client
//! end lives in `bb-indexer`.

use std::io::{self, Read};
use std::sync::Arc;
use std::time::Duration;

/// Why the elevated indexer is or is not running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexHostLaunch {
    /// It started. The client should expect a connection.
    Started,
    /// The user was asked for administrator rights and said no.
    Declined,
    /// This account cannot elevate at all, so there was nothing worth asking.
    NotAdministrator,
    /// It could not be started, for a reason the user did not choose.
    Failed,
}

/// The outcome of asking for the indexer, and the process if there is one. `process_id` is the
/// elevated process when one started: the client compares it against the process that actually
/// connects (the DACL proves the peer is this user; only this proves it is the one we started). A
/// session that attaches to a helper started under an earlier app has no such id and checks the
/// peer's integrity level instead. `detail` is a short reason for `Failed`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexHostLaunchResult {
    pub outcome: IndexHostLaunch,
    pub process_id: u32,
    pub detail: String,
}

impl IndexHostLaunchResult {
    pub fn started(process_id: u32) -> IndexHostLaunchResult {
        IndexHostLaunchResult {
            outcome: IndexHostLaunch::Started,
            process_id,
            detail: String::new(),
        }
    }

    pub fn declined() -> IndexHostLaunchResult {
        IndexHostLaunchResult {
            outcome: IndexHostLaunch::Declined,
            process_id: 0,
            detail: String::new(),
        }
    }

    pub fn not_administrator() -> IndexHostLaunchResult {
        IndexHostLaunchResult {
            outcome: IndexHostLaunch::NotAdministrator,
            process_id: 0,
            detail: String::new(),
        }
    }

    pub fn failed(detail: &str) -> IndexHostLaunchResult {
        IndexHostLaunchResult {
            outcome: IndexHostLaunch::Failed,
            process_id: 0,
            detail: detail.to_string(),
        }
    }
}

/// Starts the elevated indexer. A trait so the client can be tested against every outcome without
/// a real UAC prompt.
pub trait IndexHostLauncher: Send + Sync {
    /// Starts the indexer. It is told nothing but where the database is: the pipe it calls back on
    /// is derived from its own token, and it has no parent to watch.
    fn launch(&self) -> IndexHostLaunchResult;

    /// False when this account could not elevate even if it wanted to. Checked *before*
    /// prompting: a standard user gets a credential prompt asking for someone else's password, and
    /// firing that at somebody who never asked for it, at startup, is much worse than no index.
    fn can_elevate(&self) -> bool;

    /// Waits for a launched indexer to exit, up to `timeout`.
    fn wait_for_exit(&self, process_id: u32, timeout: Duration);
}

/// One session's byte stream, shared between the thread reading it and whichever thread writes.
/// `close` hangs up: it ends the peer's reads and unblocks a read pending on this side, which is
/// how a session is ended from outside the worker.
pub trait IndexStream: Send + Sync {
    fn read(&self, buffer: &mut [u8]) -> io::Result<usize>;
    fn write_all(&self, bytes: &[u8]) -> io::Result<()>;
    fn close(&self);
}

/// `std::io::Read` over a shared stream, for `LineReader`.
pub struct StreamRead(pub Arc<dyn IndexStream>);

impl Read for StreamRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

/// `std::io::Write` over a shared stream, for `write_line`.
pub struct StreamWrite<'a>(pub &'a dyn IndexStream);

impl std::io::Write for StreamWrite<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// One listening endpoint waiting for the elevated indexer to call back.
pub trait IndexTransport: Send + Sync {
    /// What the helper should be told to connect to.
    fn endpoint(&self) -> String;

    /// Waits for an indexer to connect; None if nobody did in time or the peer was not
    /// acceptable. Safe to call again after a timeout: the endpoint stays up, so a helper that
    /// starts later can still be picked up. `launched_process_id` is the process this app started,
    /// when it started one; None when *attaching* to one already running, where the peer's
    /// integrity level is checked instead.
    fn accept(
        &self,
        launched_process_id: Option<u32>,
        timeout: Duration,
    ) -> Option<Arc<dyn IndexStream>>;
}

/// Hands out the app's one endpoint: one for the app's life, not one per attempt. The name is
/// well-known, so there is only one endpoint to have, and it must stay up between attempts.
pub trait IndexTransportFactory: Send + Sync {
    /// The endpoint, or a short reason it cannot be had, which for a per-user name means another
    /// copy of this app already owns it.
    fn try_create(&self) -> Result<Arc<dyn IndexTransport>, String>;
}
