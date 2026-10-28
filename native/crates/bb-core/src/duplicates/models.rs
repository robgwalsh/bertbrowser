//! What a duplicate scan is asked for and what it answers. Spec: Core/Models/DuplicateModels.cs.

use crate::FileTime;

pub use crate::data::DuplicateShortlist;

/// What standing a duplicate scan's answer has.
///
/// Deliberately *not* the disk-usage availability: it weighs different evidence. That rule reads
/// an all-zero result as a sizeless volume, which it can do because it orders by size and so knows
/// the largest row in range. A duplicate scan applies a size *floor* before it looks at anything,
/// so an empty result tells it nothing at all — it has to ask the index directly whether it holds
/// believable sizes. The state that matters is `NoSizeData`: the `FSCTL_ENUM_USN_DATA` fallback
/// build writes every row with `size_bytes = 0`, and grouping by size on such a volume would put
/// the entire disk in one candidate group. Reporting "no duplicates" there would be a lie, and
/// reporting the group would be worse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateScanAvailability {
    /// The index holds real sizes for this scope; the answer can be believed.
    Ready,
    /// A volume's initial enumeration is still running, so the answer is a floor.
    Building,
    /// Nothing has indexed this scope. There is no shortlist to draw from.
    NotIndexed,
    /// Indexed by name only — every size is zero, so sizes cannot shortlist anything.
    NoSizeData,
}

/// Which pass of the scan is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DuplicateScanPhase {
    /// Reading the index for files that share a byte length. Touches no file.
    Shortlisting,
    /// Hashing the first `HEAD_SAMPLE_BYTES` of each candidate.
    Sampling,
    /// Hashing the survivors in full.
    Hashing,
}

/// What to scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateScanRequest {
    /// The folder to look under, or None for every indexed volume.
    pub root_path: Option<String>,
    /// The floor below which files are not considered. Not cosmetic: it is what bounds the
    /// shortlist's memory and the hashing passes' read volume, and duplicate 400-byte files are
    /// not what anyone opens this for.
    pub min_size_bytes: i64,
    pub include_hidden: bool,
    /// Leave out Windows, Program Files and the rest. On by default: duplicates there are not the
    /// user's to remove, and `WinSxS` alone would dominate every result.
    pub skip_system_folders: bool,
}

impl DuplicateScanRequest {
    /// One megabyte.
    pub const DEFAULT_MIN_SIZE_BYTES: i64 = 1024 * 1024;
}

impl Default for DuplicateScanRequest {
    fn default() -> Self {
        DuplicateScanRequest {
            root_path: None,
            min_size_bytes: Self::DEFAULT_MIN_SIZE_BYTES,
            include_hidden: false,
            skip_system_folders: true,
        }
    }
}

/// One file in a duplicate group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateFile {
    pub display_path: String,
    pub relative_dir_display: String,
    pub name: String,
    pub size_bytes: i64,
    pub modified_utc: FileTime,
    pub hidden: bool,
    /// The other names this same file on disk answers to, when it has more than one. Empty for
    /// the overwhelming majority. These are *not* duplicates — deleting one frees nothing — so
    /// they travel with the entry rather than appearing beside it.
    pub hardlink_paths: Vec<String>,
}

/// Files that are byte-for-byte the same thing, in different places.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub size_bytes: i64,
    pub hash: String,
    pub files: Vec<DuplicateFile>,
}

impl DuplicateGroup {
    /// What keeping every copy costs: one copy is wanted, the rest are the waste.
    pub fn wasted_bytes(&self) -> i64 {
        self.size_bytes * (self.files.len() as i64 - 1)
    }
}

/// Progress while a scan runs. `bytes_total` is zero during `Shortlisting` and the bar must be
/// indeterminate there: counting the rows first would cost a second full scan of the index, and a
/// determinate bar pinned at 0% reads as a stall. It becomes exact the moment the shortlist is in,
/// which is what the two hashing phases show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateScanProgress {
    pub phase: DuplicateScanPhase,
    pub done: usize,
    pub total: usize,
    pub current_name: String,
    pub bytes_done: i64,
    pub bytes_total: i64,
}

/// What a scan found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateScanOutcome {
    pub groups: Vec<DuplicateGroup>,
    pub availability: DuplicateScanAvailability,
    /// True when the user stopped it. Without this a cancelled scan is indistinguishable from one
    /// that genuinely found nothing.
    pub cancelled: bool,
    /// True when at least one candidate could not be read. The groups are then a floor, not the
    /// answer, which the view says rather than quietly under-reporting.
    pub incomplete: bool,
    pub files_hashed: usize,
    pub bytes_hashed: i64,
}

impl DuplicateScanOutcome {
    pub fn wasted_bytes(&self) -> i64 {
        self.groups.iter().map(DuplicateGroup::wasted_bytes).sum()
    }

    pub fn has_results(&self) -> bool {
        !self.groups.is_empty()
    }

    pub fn empty(availability: DuplicateScanAvailability) -> DuplicateScanOutcome {
        DuplicateScanOutcome {
            groups: Vec::new(),
            availability,
            cancelled: false,
            incomplete: false,
            files_hashed: 0,
            bytes_hashed: 0,
        }
    }
}
