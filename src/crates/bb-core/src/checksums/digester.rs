//! Digesting one file under several algorithms at once. Spec: Core/Services/Checksums/IFileDigester.cs.
//!
//! A separate seam from `FileHasher` on purpose, rather than an algorithm parameter added to it.
//! The duplicate finder's safety argument is that it uses SHA-256 *specifically*, because what a
//! user does with its answer is delete files; widening that trait would make the argument
//! unenforceable. Two traits put the rule in the type system instead — a duplicate scanner cannot
//! be handed a CRC-32 by anybody, including a future us. Both are implemented by one type over one
//! read loop (`FileSystemFileHasher`), so the sharing rules, the placeholder refusals and the
//! buffer live once.

use std::collections::HashMap;

use super::ChecksumAlgorithm;
use crate::cancel::{CancelToken, Cancelled};

/// What digesting one file produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDigests {
    /// A digest per algorithm asked for, uppercase hex.
    pub by_algorithm: HashMap<ChecksumAlgorithm, String>,
    /// What was actually read. Said out loud rather than assumed, for the same reason
    /// `FileFingerprint` says it: a file can change size between being listed and being read.
    pub bytes_read: i64,
}

/// A sink for chunk sizes as they land: each call is one chunk's byte count, never a running
/// total, and it is called from whichever thread is reading.
pub type ByteProgress<'a> = Option<&'a (dyn Fn(i64) + Sync)>;

pub trait FileDigester: Sync {
    /// Digests `path` under every algorithm in `algorithms`, reading the file exactly once.
    ///
    /// `progress` is called with each chunk's byte count as it lands, and *once* per chunk however
    /// many algorithms are running — so a caller digesting several files can add them up itself
    /// and a five-algorithm run does not report five times the bytes there are.
    ///
    /// `Ok(None)` when the file cannot be read, or must not be. Not an error: one unreadable file
    /// marks a run incomplete and the rest carry on. A cancelled run is `Err(Cancelled)` rather
    /// than `None`; the difference is load-bearing and is the same one `FileHasher` draws: one is
    /// this file having a problem, the other is the whole run stopping.
    fn digest(
        &self,
        path: &str,
        algorithms: &[ChecksumAlgorithm],
        progress: ByteProgress<'_>,
        cancel: &CancelToken,
    ) -> Result<Option<FileDigests>, Cancelled>;
}
