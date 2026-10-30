//! Where a scan gets its shortlist of files that might be duplicates. Spec:
//! Core/Services/Duplicates/IDuplicateCandidateSource.cs.
//!
//! A seam for the reason the probes elsewhere in this app are: it lets `duplicate_scanner_tests`
//! exercise the grouping, the hardlink folding, the cancellation and every availability case
//! against a handful of made-up rows, with no SQLite and no disk.

use super::{DuplicateRules, DuplicateScanRequest, DuplicateShortlist};
use crate::cancel::CancelToken;
use crate::data::{self, FsIndexRepository};

pub trait DuplicateCandidateSource: Sync {
    /// Files sharing a byte length with at least one other, plus what the index turned out to
    /// hold — which is what decides whether the answer can be believed at all. `Err(Cancelled)`
    /// when the token stopped it; any other error is the index failing to answer.
    fn shortlist(
        &self,
        request: &DuplicateScanRequest,
        cancel: &CancelToken,
    ) -> data::Result<DuplicateShortlist>;
}

/// The real shortlist, straight out of the MFT/search index. No file is opened here and nothing
/// walks the filesystem. Every byte length this reads was written by the MFT pass as a side effect
/// of building the search index, which is what makes the expensive half of finding duplicates
/// already paid for.
pub struct IndexedDuplicateCandidateSource<'r> {
    index: &'r FsIndexRepository,
}

impl<'r> IndexedDuplicateCandidateSource<'r> {
    pub fn new(index: &'r FsIndexRepository) -> IndexedDuplicateCandidateSource<'r> {
        IndexedDuplicateCandidateSource { index }
    }
}

impl DuplicateCandidateSource for IndexedDuplicateCandidateSource<'_> {
    fn shortlist(
        &self,
        request: &DuplicateScanRequest,
        cancel: &CancelToken,
    ) -> data::Result<DuplicateShortlist> {
        let exclude = |key: &str| DuplicateRules::is_excluded(key, request.skip_system_folders);
        self.index.duplicate_candidates(
            request.root_path.as_deref(),
            request.min_size_bytes,
            request.include_hidden,
            Some(&exclude),
            cancel,
        )
    }
}
