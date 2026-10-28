//! Whether there is an elevated index helper to talk to, and the probe that answers it without
//! talking to it. Spec: Core/Services/Mft/IndexerPresence.cs, IIndexerPresence.cs,
//! App/Services/Indexing/WindowsIndexerPresence.cs.
//!
//! Three values rather than a bool, and the third is the point: only `MftIndexClient` is a client
//! of a separate helper; the in-process service and the null one are not, and a bool would force
//! them to answer "not running", which would raise the "start the indexer" banner in every harness
//! run over a helper those hosts were never going to have.
//!
//! The probe is not the same question as "did anything connect": a helper that is running but
//! cannot be reached must not be reported as absent, because the offer to start another would
//! raise an elevation prompt and then be refused by the helper's own single-instance guard.

use crate::ipc::{IndexEndpoint, IndexerPresenceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexerPresence {
    /// This host does not use a separate helper, so the question does not arise.
    NotApplicable,
    /// No helper is running. The user may be offered one.
    NotRunning,
    /// A helper is running, whether or not this process has attached to it yet.
    Running,
}

impl IndexerPresence {
    pub const ALL: [IndexerPresence; 3] = [
        IndexerPresence::NotApplicable,
        IndexerPresence::NotRunning,
        IndexerPresence::Running,
    ];
}

/// Answers whether an index helper is running, without talking to it.
pub trait IndexerPresenceProbe: Send + Sync {
    fn is_running(&self) -> bool;
}

/// Reports that nothing is ever running, for hosts with no helper to find.
pub struct NoIndexerPresence;

impl IndexerPresenceProbe for NoIndexerPresence {
    fn is_running(&self) -> bool {
        false
    }
}

/// Asks Windows, by looking for the name a helper holds for its life. A medium-integrity process
/// may open a high-integrity one's mutex for `SYNCHRONIZE`, since that is not a write right and
/// mandatory policy is no-write-up; see `IndexerPresenceLock`.
pub struct WindowsIndexerPresence;

impl IndexerPresenceProbe for WindowsIndexerPresence {
    fn is_running(&self) -> bool {
        match IndexEndpoint::current_user_sid() {
            Ok(sid) => IndexerPresenceLock::is_held(&sid),
            // No SID for this token: nothing can be running that we could reach anyway.
            Err(_) => false,
        }
    }
}
