// Port of tests/BertBrowser.Core.Tests/DiskUsageRulesTests.cs
use bb_core::diskusage::{DiskUsageAvailability, DiskUsageNode, DiskUsageRules};

fn node(name: &str, size: Option<i64>) -> DiskUsageNode {
    DiskUsageNode {
        path_key: format!(r"C:\ROOT\{}", name.to_uppercase()),
        display_path: format!(r"C:\Root\{name}"),
        name: name.to_string(),
        is_directory: true,
        size_bytes: size,
        incomplete: false,
        hidden: false,
    }
}

// --- Classify: the largest-files verdict ---

/// The one that matters. The FSCTL_ENUM_USN_DATA build path writes every row with size_bytes = 0
/// and fills no dir_size_cache, so a top-N by size comes back full of rows that are all zero. That
/// is a volume nobody measured, not a disk full of empty files — and since the query orders by
/// size descending, a zero in the *largest* slot proves it. Make `classify` return Ready here and
/// this goes red; that mutation is exactly the bug that puts a screenful of "0 B" in front of the
/// user.
#[test]
fn all_zero_sizes_is_not_size_data_not_an_empty_disk() {
    let verdict = DiskUsageRules::classify(Some(r"C:\"), 500, 0, false, true);

    assert_eq!(DiskUsageAvailability::NoSizeData, verdict);
}

/// The sizeless shape is decided before the building check, or a volume that is both mid-build
/// and sizeless would be reported as merely incomplete and drawn as zeros.
#[test]
fn all_zero_sizes_beats_building() {
    assert_eq!(
        DiskUsageAvailability::NoSizeData,
        DiskUsageRules::classify(Some(r"C:\"), 500, 0, true, false)
    );
}

#[test]
fn rows_with_real_sizes_are_ready() {
    assert_eq!(
        DiskUsageAvailability::Ready,
        DiskUsageRules::classify(Some(r"C:\"), 500, 4_096, false, true)
    );
}

/// Partial results while a volume is still being read are a floor, and have to say so rather than
/// presenting themselves as the answer.
#[test]
fn rows_while_still_building_are_building() {
    assert_eq!(
        DiskUsageAvailability::Building,
        DiskUsageRules::classify(Some(r"C:\"), 12, 4_096, true, false)
    );
}

#[test]
fn no_rows_while_building_is_building_not_empty() {
    assert_eq!(
        DiskUsageAvailability::Building,
        DiskUsageRules::classify(Some(r"C:\"), 0, 0, true, false)
    );
}

#[test]
fn no_rows_not_indexed_says_so_so_the_view_can_offer_a_retry() {
    assert_eq!(
        DiskUsageAvailability::NotIndexed,
        DiskUsageRules::classify(Some(r"C:\"), 0, 0, false, false)
    );
}

/// An indexed volume that genuinely holds no files is the one case where "nothing" is the true
/// answer.
#[test]
fn no_rows_but_indexed_is_a_genuinely_empty_result() {
    assert_eq!(
        DiskUsageAvailability::Ready,
        DiskUsageRules::classify(Some(r"C:\"), 0, 0, false, true)
    );
}

/// None is the deliberate whole-PC scope; only an empty string is a bad root.
#[test]
fn null_root_is_whole_pc_empty_root_is_not_a_path() {
    assert_eq!(
        DiskUsageAvailability::Ready,
        DiskUsageRules::classify(None, 5, 100, false, true)
    );
    assert_eq!(
        DiskUsageAvailability::NotAPath,
        DiskUsageRules::classify(Some(""), 5, 100, false, true)
    );
}

// --- ClassifyBreakdown: one folder's verdict, which weighs different evidence ---

/// A folder of empty files really is all zeros, and saying "no size data" about it would be a lie
/// about ordinary content. Point `classify`'s all-zero rule at a breakdown and this goes red.
#[test]
fn a_folder_of_empty_files_is_ready_not_no_size_data() {
    assert_eq!(
        DiskUsageAvailability::Ready,
        DiskUsageRules::classify_breakdown(0, 0, false, false)
    );
}

#[test]
fn measured_subfolders_are_ready() {
    assert_eq!(
        DiskUsageAvailability::Ready,
        DiskUsageRules::classify_breakdown(3, 3, false, true)
    );
}

#[test]
fn subfolders_none_measured_while_building_is_building() {
    assert_eq!(
        DiskUsageAvailability::Building,
        DiskUsageRules::classify_breakdown(3, 0, true, false)
    );
}

/// Indexed, has subfolders, and not one total anywhere: the sizeless build.
#[test]
fn subfolders_none_measured_but_indexed_is_no_size_data() {
    assert_eq!(
        DiskUsageAvailability::NoSizeData,
        DiskUsageRules::classify_breakdown(3, 0, false, true)
    );
}

#[test]
fn subfolders_none_measured_and_not_indexed_is_not_indexed() {
    assert_eq!(
        DiskUsageAvailability::NotIndexed,
        DiskUsageRules::classify_breakdown(3, 0, false, false)
    );
}

// --- Unaccounted: the arithmetic form of "never zero" ---

#[test]
fn unaccounted_is_the_remainder_when_everything_is_known() {
    let remainder =
        DiskUsageRules::unaccounted(Some(1_000), &[node("a", Some(600)), node("b", Some(300))]);

    assert_eq!(Some(100), remainder);
}

/// Treating an unknown child as zero would silently attribute its bytes to the parent's own loose
/// files — a wrong number, not a smaller one. Change the sum to `unwrap_or(0)` and this goes red.
#[test]
fn unaccounted_is_null_when_any_child_is_unknown() {
    assert_eq!(
        None,
        DiskUsageRules::unaccounted(Some(1_000), &[node("a", Some(600)), node("b", None)])
    );
}

#[test]
fn unaccounted_is_null_when_the_parent_total_is_unknown() {
    assert_eq!(
        None,
        DiskUsageRules::unaccounted(None, &[node("a", Some(600))])
    );
}

/// A dir_size_cache row that predates its children's makes the remainder negative. There is no
/// negative space, so the honest answer is "cannot say" rather than a bar drawn backwards.
#[test]
fn unaccounted_is_null_rather_than_negative_when_the_parent_row_is_stale() {
    assert_eq!(
        None,
        DiskUsageRules::unaccounted(Some(500), &[node("a", Some(600)), node("b", Some(300))])
    );
}

#[test]
fn unaccounted_is_the_whole_total_when_there_are_no_children() {
    assert_eq!(Some(1_000), DiskUsageRules::unaccounted(Some(1_000), &[]));
}
