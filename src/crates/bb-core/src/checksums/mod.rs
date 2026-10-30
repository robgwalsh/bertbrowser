//! Checksums: the algorithm table, the one read loop both hashing seams share, the files the rest
//! of the world exchanges, the untrusted-name rule, the compare box and the verify report. Spec:
//! Core/Services/Checksums/*.cs plus the `FileSystemFileHasher` class in
//! Core/Services/Duplicates/IFileHasher.cs.
//!
//! Two seams over one read loop: `FileHasher` (in `crate::duplicates`) stays SHA-256-only because
//! the duplicate finder's safety argument is that answer authorises a delete; `FileDigester` is the
//! user-chosen one. Digests are uppercase everywhere inside Core; casing is decided in exactly one
//! place (`ChecksumAlgorithms::is_written_lowercase`) and applied only when rendering.

mod algorithm;
mod compare;
mod digester;
mod file;
mod file_hasher;
mod path;
mod runner;
mod sink;
mod verify;

pub use algorithm::{ChecksumAlgorithm, ChecksumAlgorithms};
pub use compare::{ChecksumCompare, ChecksumMatchState};
pub use digester::{ByteProgress, FileDigester, FileDigests};
pub use file::{
    ChecksumFile, ChecksumFileParse, ChecksumFileProblem, ChecksumFileProblemKind, ChecksumLine,
};
pub use file_hasher::FileSystemFileHasher;
pub use path::ChecksumPath;
pub use runner::{
    ChecksumOutcome, ChecksumProgress, ChecksumProgressSink, ChecksumRow, ChecksumRunner,
};
pub use sink::to_hex_upper;
pub use verify::{ChecksumVerify, ChecksumVerifyRow, ChecksumVerifyState};
