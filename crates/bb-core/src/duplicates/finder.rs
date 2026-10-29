//! The facade over the duplicate scan. Spec: Core/Services/Duplicates/DuplicateFinder.cs.
//!
//! In C# this was the async wrapper that kept view models off blocking I/O; here it is a blocking
//! call the UI runs on a worker thread. It exists for one other reason too. `DuplicateScanner`
//! takes the two index facts as booleans so that every availability case can be tested by handing
//! it a pair; resolving them from the index service is this layer's job, and the whole-PC scope
//! asks `any_indexed` where a scoped one asks about its own volume — the same distinction the
//! disk-usage service draws.

use bb_paths::PathKey;

use super::{
    DuplicateCandidateSource, DuplicateProgressSink, DuplicateScanAvailability,
    DuplicateScanOutcome, DuplicateScanRequest, DuplicateScanner, FileHasher,
};
use crate::cancel::CancelToken;
use crate::data;

/// The slice of the MFT index service a duplicate scan asks.
// TODO(port): fold into `crate::mft::MftIndex` when Mft (M3) is ported and that trait gains
// `any_indexed`; until then the real service implements this alongside it.
pub trait DuplicateIndexFacts: Sync {
    /// Whether any volume's initial enumeration is still running.
    fn is_building(&self) -> bool;
    /// Whether any volume at all reports a complete live index.
    fn any_indexed(&self) -> bool;
    /// Whether this root's volume reports a complete live index.
    fn is_indexed(&self, path_key: &str) -> bool;
}

pub struct DuplicateFinder<'a> {
    candidates: &'a dyn DuplicateCandidateSource,
    hasher: &'a dyn FileHasher,
    index: &'a dyn DuplicateIndexFacts,
}

impl<'a> DuplicateFinder<'a> {
    pub fn new(
        candidates: &'a dyn DuplicateCandidateSource,
        hasher: &'a dyn FileHasher,
        index: &'a dyn DuplicateIndexFacts,
    ) -> DuplicateFinder<'a> {
        DuplicateFinder {
            candidates,
            hasher,
            index,
        }
    }

    /// Finds files that are byte-for-byte identical under `request.root_path`, or across every
    /// indexed volume when it is None. The shortlist is a full scan of the index and the passes
    /// after it read real files, so this takes seconds at best and minutes on a large disk. It is
    /// meant to be reached from an explicit "go and find these" gesture, never from typing, and
    /// the token is honoured throughout — a cancelled scan hands back what it had already
    /// confirmed rather than nothing.
    pub fn scan(
        &self,
        request: &DuplicateScanRequest,
        progress: DuplicateProgressSink<'_>,
        cancel: &CancelToken,
    ) -> data::Result<DuplicateScanOutcome> {
        let mut root_key: Option<String> = None;
        if let Some(path) = request.root_path.as_deref().filter(|p| !p.is_empty()) {
            match PathKey::canonicalize(path) {
                Ok(key) => root_key = Some(key),
                // Not a folder anything could be scanned under.
                Err(_) => {
                    return Ok(DuplicateScanOutcome::empty(
                        DuplicateScanAvailability::NotIndexed,
                    ));
                }
            }
        }

        let is_indexed = match &root_key {
            None => self.index.any_indexed(),
            Some(key) => self.index.is_indexed(key),
        };

        DuplicateScanner::new(self.candidates, self.hasher).scan(
            request,
            self.index.is_building(),
            is_indexed,
            cancel,
            progress,
        )
    }
}
