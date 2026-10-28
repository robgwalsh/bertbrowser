//! Works out what a plan costs in bytes, before it runs.
//! Spec: Core/Services/Transfer/TransferEstimator.cs.

use super::executor::TransferExecutor;
use super::models::{PlannedTransfer, TransferPlan, TransferVerb};

/// A directory's recursive footprint, as the size index already knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectorySize {
    pub bytes: i64,
    pub files: usize,
}

/// Where a byte total comes from. A trait rather than a walk: this app's whole premise is that
/// *nothing scans the filesystem to size a folder*; the MFT pass has already written a row for
/// every directory on the volume, so the answer is a lookup.
pub trait TransferSizeSource {
    /// A directory's recursive size, or None when the index has no row for it: a non-NTFS volume,
    /// or one still being indexed.
    fn directory(&self, path: &str) -> Option<DirectorySize>;

    /// A file's size, or None when it cannot be read.
    fn file(&self, path: &str) -> Option<i64>;
}

/// How much a plan will actually move, and whether that figure can be trusted. `complete` is false
/// when something in the plan had no size to hand: the figures are then a floor, not a total, and
/// must not be turned into a percentage or a time remaining.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferEstimate {
    pub bytes: i64,
    pub files: usize,
    pub complete: bool,
}

impl TransferEstimate {
    /// An empty plan, or one that is pure renaming: nothing to move, and we know it.
    pub const NOTHING: TransferEstimate = TransferEstimate {
        bytes: 0,
        files: 0,
        complete: true,
    };

    pub fn new(bytes: i64, files: usize, complete: bool) -> TransferEstimate {
        TransferEstimate {
            bytes,
            files,
            complete,
        }
    }

    /// True when there is a real total to draw a bar against.
    pub fn is_usable(&self) -> bool {
        self.complete && self.bytes > 0
    }
}

pub struct TransferEstimator;

impl TransferEstimator {
    pub fn estimate(plan: &TransferPlan, sizes: &dyn TransferSizeSource) -> TransferEstimate {
        let mut bytes = 0i64;
        let mut files = 0usize;
        let mut complete = true;

        for transfer in &plan.transfers {
            if !Self::moves_bytes(plan.verb, transfer) {
                continue;
            }
            if transfer.is_directory {
                match sizes.directory(&transfer.source_path) {
                    Some(size) => {
                        bytes += size.bytes;
                        files += size.files;
                    }
                    None => complete = false,
                }
            } else {
                // A size the plan already carries is preferred to asking disk for it again: an
                // expanded merge knows every file's length from the enumeration that found it,
                // and without this a merge of four thousand files would be four thousand stats
                // on the enqueue path, in front of the user.
                match transfer
                    .known_bytes
                    .or_else(|| sizes.file(&transfer.source_path))
                {
                    Some(size) => {
                        bytes += size;
                        files += 1;
                    }
                    None => complete = false,
                }
            }
        }

        TransferEstimate {
            bytes,
            files,
            complete,
        }
    }

    /// Whether this item costs anything to transfer at all. **A move within one volume is a
    /// rename**: it relocates 50 GB in the time it takes to update a directory entry, so counting
    /// those bytes would put a bar on screen that sits at 0% while eleven instant renames go past.
    /// Asks the executor's own `same_volume`, so the estimate and the work cannot disagree.
    pub fn moves_bytes(verb: TransferVerb, transfer: &PlannedTransfer) -> bool {
        verb == TransferVerb::Copy
            || !TransferExecutor::same_volume(&transfer.source_path, &transfer.destination_path)
    }
}
