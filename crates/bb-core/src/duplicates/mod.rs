//! The duplicate finder: the shortlist seam over the index, the SHA-256-only hashing seam, the
//! pure rules, the three-pass scanner and the facade that resolves the index facts. Spec:
//! Core/Services/Duplicates/*.cs, Core/Models/DuplicateModels.cs.
//!
//! The shortlist comes from `dir_size_cache`'s sibling, the `fs_entry` index — sizes are never
//! computed on demand — and the hasher is `FileHasher`, which only ever answers SHA-256, because
//! what the user does with a group is delete files from it.

mod candidate_source;
mod finder;
mod hasher;
mod models;
mod rules;
mod scanner;

pub use candidate_source::{DuplicateCandidateSource, IndexedDuplicateCandidateSource};
pub use finder::{DuplicateFinder, DuplicateIndexFacts};
pub use hasher::{FileFingerprint, FileHasher, FileIdentity};
pub use models::{
    DuplicateFile, DuplicateGroup, DuplicateScanAvailability, DuplicateScanOutcome,
    DuplicateScanPhase, DuplicateScanProgress, DuplicateScanRequest, DuplicateShortlist,
};
pub use rules::{DuplicateRules, KeepStrategy};
pub use scanner::{DuplicateProgressSink, DuplicateScanner};
