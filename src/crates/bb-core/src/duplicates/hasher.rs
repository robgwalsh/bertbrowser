//! Hashing the bytes of one file, with progress and interruption. Spec:
//! Core/Services/Duplicates/IFileHasher.cs (the interface; the real implementation is
//! `crate::checksums::FileSystemFileHasher`, one read loop under both seams).
//!
//! A seam for the same reason the copier is one: it is what lets `duplicate_scanner_tests` drive a
//! cancel that lands *in the middle of a file* deterministically and in milliseconds, and lets the
//! grouping rules be tested against contrived collisions without writing files that really collide.

use crate::cancel::{CancelToken, Cancelled};
use crate::checksums::ByteProgress;

/// Which file on disk a name actually refers to, when it answers to more than one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    pub volume_serial_number: u32,
    pub file_index: u64,
}

/// What hashing one file learned about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFingerprint {
    pub hash: String,
    /// What was actually read, which is not necessarily what the index said the file was: the row
    /// may be stale, and a file that shrank between the shortlist and the hash must not be
    /// compared against one that did not.
    pub bytes_read: i64,
    /// Set only when the file carries more than one name. None is the common case and means "this
    /// is its own file"; the scanner then does no identity bookkeeping at all.
    pub identity: Option<FileIdentity>,
}

pub trait FileHasher: Sync {
    /// Hashes at most `max_bytes` of `path` — the whole file when that is zero or negative.
    /// `progress` is called with each chunk's byte count as it lands, never a running total, so a
    /// caller hashing several files at once can add them up itself.
    ///
    /// `Ok(None)` when the file cannot be read, or must not be. That is not an error: one
    /// unreadable candidate marks the scan incomplete and the rest carry on, exactly as one item's
    /// failure never costs the others anywhere else in this app. A cancelled hash is
    /// `Err(Cancelled)` rather than `None`. The difference is load-bearing: one is this file
    /// having a problem, the other is the whole run stopping, and conflating them would let a
    /// cancel look like a disk full of unreadable files.
    fn hash(
        &self,
        path: &str,
        max_bytes: i64,
        progress: ByteProgress<'_>,
        cancel: &CancelToken,
    ) -> Result<Option<FileFingerprint>, Cancelled>;
}
