//! What a disk-usage answer is made of, and the one distinction every type here exists to keep:
//! "this folder holds nothing" and "nobody has measured this folder" look identical in a size
//! column and mean opposite things. Spec: Core/Models/DiskUsageModels.cs.

use crate::data::SearchHit;

/// Whether a disk-usage answer can be trusted, and if not, why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskUsageAvailability {
    /// Real sizes, from a completed index.
    Ready,

    /// The volume is still being indexed. Whatever is shown is a floor, not a total.
    Building,

    /// The index covers this volume but carries no sizes for it. This is the
    /// `FSCTL_ENUM_USN_DATA` fallback shape: the volume indexer takes that path when the raw
    /// $MFT is unparseable, and it writes every row with `size_bytes = 0` and fills no
    /// dir_size_cache at all. Rows exist, so this is not `NotIndexed`, and every one of them is
    /// zero, so it must never be rendered as a list of empty files.
    NoSizeData,

    /// Nothing indexed here: a declined elevation prompt, a standard-user account, a non-NTFS
    /// volume, or an indexer that never ran.
    NotIndexed,

    /// The root asked about is not a usable directory path.
    NotAPath,
}

/// One row of a disk-usage breakdown: a direct child of the folder being examined.
///
/// `size_bytes` of `None` means *unknown*, and the distinction is the whole point of this type.
/// A directory with no dir_size_cache row has not been measured; rendering that as 0 claims it is
/// empty. Every consumer must show blank, never a number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskUsageNode {
    pub path_key: String,
    pub display_path: String,
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: Option<i64>,
    pub incomplete: bool,
    pub hidden: bool,
}

/// The composition of one directory: its children, largest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskUsageBreakdown {
    pub root_display_path: String,
    /// The folder's own cached total, or None when it has not been measured.
    pub total_bytes: Option<i64>,
    pub children: Vec<DiskUsageNode>,
    /// What `total_bytes` holds that the children do not explain. None whenever it cannot be
    /// computed honestly, which is most of the time on an unindexed volume.
    pub unaccounted_bytes: Option<i64>,
    /// How many children came back with no size. They are still listed, but they can be given
    /// no area.
    pub unknown_child_count: usize,
    pub availability: DiskUsageAvailability,
}

/// The biggest files under some root, largest first, with the standing of the data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LargestFilesOutcome {
    pub files: Vec<SearchHit>,
    pub availability: DiskUsageAvailability,
}
