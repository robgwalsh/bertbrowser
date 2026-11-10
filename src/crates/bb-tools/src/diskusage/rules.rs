//! The two judgements a disk-usage view has to make before it can draw anything, kept pure and
//! away from SQLite so they can be tested rather than eyeballed: *can these numbers be believed*,
//! and *how much of a folder do its children fail to explain*.
//!
//! Both answers exist to protect one invariant, which this whole feature is shaped around: an
//! unmeasured folder is unknown, never zero. A size column cannot tell those apart, so the
//! decision is made here, once, where a test can hold it still.
//! Spec: Core/Services/DiskUsage/DiskUsageRules.cs.

use super::{DiskUsageAvailability, DiskUsageNode};

pub struct DiskUsageRules;

impl DiskUsageRules {
    /// What standing the rows returned for `root_key` have.
    ///
    /// `largest_size_bytes` is the biggest size among the rows, which, since the query orders by
    /// size descending, is the first row's. Zero here is decisive: if the *largest* file in range
    /// is 0 bytes then every row in range is, and that is not a disk full of empty files, it is a
    /// volume indexed by the sizeless `FSCTL_ENUM_USN_DATA` fallback. `is_building` is whether any
    /// volume's initial enumeration is still running; `is_indexed` whether this root's volume
    /// reports a complete live index.
    pub fn classify(
        root_key: Option<&str>,
        row_count: usize,
        largest_size_bytes: i64,
        is_building: bool,
        is_indexed: bool,
    ) -> DiskUsageAvailability {
        // A None root is the deliberate "whole PC" scope, not a bad argument.
        if root_key == Some("") {
            return DiskUsageAvailability::NotAPath;
        }

        if row_count > 0 {
            // Rows but no bytes: the sizeless build path. Reporting Ready here is what would put
            // a screenful of "0 B" in front of the user, so it is checked before anything else.
            if largest_size_bytes <= 0 {
                return DiskUsageAvailability::NoSizeData;
            }
            return if is_building && !is_indexed {
                DiskUsageAvailability::Building
            } else {
                DiskUsageAvailability::Ready
            };
        }

        // No rows at all. Still building is a "come back in a moment"; anything else is genuinely
        // not indexed, and the caller offers the same retry the status bar does.
        if is_building {
            return DiskUsageAvailability::Building;
        }
        if is_indexed {
            DiskUsageAvailability::Ready
        } else {
            DiskUsageAvailability::NotIndexed
        }
    }

    /// What standing one folder's breakdown has.
    ///
    /// Deliberately *not* `classify`. That rule reads an all-zero result as a sizeless volume,
    /// which is sound for a top-N by size across a subtree but wrong here: a folder holding three
    /// empty files really is all zeros, and calling that "no size data" would be a lie about
    /// ordinary content. The evidence differs too: a breakdown's file sizes come from the
    /// enumeration and are always real, so only the *directory* totals depend on the index, and
    /// it is those this weighs. `directory_child_count` is how many of the children are
    /// directories; `measured_directory_count` how many of those had a dir_size_cache row.
    pub fn classify_breakdown(
        directory_child_count: usize,
        measured_directory_count: usize,
        is_building: bool,
        is_indexed: bool,
    ) -> DiskUsageAvailability {
        // Files only: the enumeration already carried real lengths, so this answer needs no
        // index at all and is complete on a volume nothing has ever looked at.
        if directory_child_count == 0 {
            return DiskUsageAvailability::Ready;
        }

        if measured_directory_count > 0 {
            return if is_building && !is_indexed {
                DiskUsageAvailability::Building
            } else {
                DiskUsageAvailability::Ready
            };
        }

        // Subfolders, and not one of them measured.
        if is_building {
            return DiskUsageAvailability::Building;
        }

        // A volume that claims to be indexed and still has no folder total anywhere is the
        // sizeless FSCTL_ENUM_USN_DATA build, which fills no dir_size_cache.
        if is_indexed {
            DiskUsageAvailability::NoSizeData
        } else {
            DiskUsageAvailability::NotIndexed
        }
    }

    /// How many of `total_bytes` the children do not account for: the folder's own loose files on
    /// a directory-only breakdown, or simply the part nobody measured.
    ///
    /// None whenever the honest answer is "cannot say": the parent was never measured, or any one
    /// child was not. A remainder computed from an incomplete sum is not a smaller number, it is
    /// a wrong one; treating an unknown child as zero would silently attribute its bytes to the
    /// parent's own files.
    pub fn unaccounted(total_bytes: Option<i64>, children: &[DiskUsageNode]) -> Option<i64> {
        let total = total_bytes?;

        let mut sum: i64 = 0;
        for child in children {
            sum += child.size_bytes?;
        }

        let remainder = total - sum;

        // A parent row that predates its children's (a stale dir_size_cache entry against a
        // folder that has since grown) makes this negative. There is no such thing as negative
        // space, so the answer is "cannot say" rather than a bar drawn backwards.
        if remainder < 0 { None } else { Some(remainder) }
    }
}
