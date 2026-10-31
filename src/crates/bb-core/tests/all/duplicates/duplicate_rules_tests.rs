// Port of tests/BertBrowser.Core.Tests/DuplicateRulesTests.cs
//
// The judgements a duplicate scan makes before it shows or removes anything. All pure, so they are
// held still here rather than eyeballed in a window that takes minutes to fill.
use bb_core::FileTime;
use bb_core::duplicates::{
    DuplicateFile, DuplicateGroup, DuplicateRules, DuplicateScanAvailability, KeepStrategy,
};
use bb_paths::fold_upper;

fn stamp() -> FileTime {
    FileTime::from_ymd_hms(2024, 3, 14, 9, 26, 53)
}

fn file(path: &str, modified: Option<FileTime>, size: i64) -> DuplicateFile {
    DuplicateFile {
        display_path: path.to_string(),
        relative_dir_display: String::new(),
        name: bb_paths::file_name(path).to_string(),
        size_bytes: size,
        modified_utc: modified.unwrap_or_else(stamp),
        hidden: false,
        hardlink_paths: Vec::new(),
    }
}

fn group(files: Vec<DuplicateFile>) -> DuplicateGroup {
    DuplicateGroup {
        size_bytes: files[0].size_bytes,
        hash: "HASH".to_string(),
        files,
    }
}

/// `Environment.GetFolderPath(SpecialFolder.Windows)`, upper-cased.
fn windows_folder() -> String {
    fold_upper(&std::env::var("WINDIR").expect("WINDIR"))
}

/// `Environment.GetFolderPath(SpecialFolder.UserProfile)`, upper-cased.
fn profile_folder() -> String {
    fold_upper(&std::env::var("USERPROFILE").expect("USERPROFILE"))
}

// --- Classify ---

/// The one that matters. The FSCTL_ENUM_USN_DATA build path writes every row with
/// size_bytes = 0, so on such a volume every file collides with every other and the shortlist
/// means nothing at all — a scan that trusted it would try to hash the whole disk. Rows in scope
/// with not one real length is the signature, and it is not the same thing as a disk with no
/// duplicates on it. Make classify return Ready here and this goes red.
#[test]
fn rows_but_no_lengths_is_not_size_data_not_an_empty_result() {
    let verdict = DuplicateRules::classify(500, 0, false, true);

    assert_eq!(DuplicateScanAvailability::NoSizeData, verdict);
}

#[test]
fn real_lengths_in_scope_are_ready() {
    assert_eq!(
        DuplicateScanAvailability::Ready,
        DuplicateRules::classify(500, 500, false, true)
    );
}

/// A network share or a removable disk is filled in by the crawler rather than the MFT pass, so
/// the index service says "not indexed" about rows whose sizes are perfectly good. The evidence
/// has to beat the opinion, or the feature refuses to scan folders it can scan.
#[test]
fn real_lengths_beat_the_index_service_saying_no() {
    assert_eq!(
        DuplicateScanAvailability::Ready,
        DuplicateRules::classify(40, 40, false, false)
    );
}

#[test]
fn still_building_with_lengths_says_so() {
    assert_eq!(
        DuplicateScanAvailability::Building,
        DuplicateRules::classify(40, 40, true, false)
    );
}

/// A volume that has finished is Ready even while another one is still going.
#[test]
fn still_building_elsewhere_but_this_scope_is_done_is_ready() {
    assert_eq!(
        DuplicateScanAvailability::Ready,
        DuplicateRules::classify(40, 40, true, true)
    );
}

#[test]
fn nothing_indexed_and_nothing_in_scope_is_not_indexed() {
    assert_eq!(
        DuplicateScanAvailability::NotIndexed,
        DuplicateRules::classify(0, 0, false, false)
    );
}

/// An indexed folder that really is empty has been looked at and found to hold nothing, which is
/// an answer. Reporting NotIndexed there would send the user to a retry button that cannot change
/// anything.
#[test]
fn an_indexed_but_empty_scope_is_ready_not_unindexed() {
    assert_eq!(
        DuplicateScanAvailability::Ready,
        DuplicateRules::classify(0, 0, false, true)
    );
}

#[test]
fn nothing_in_scope_while_building_is_building() {
    assert_eq!(
        DuplicateScanAvailability::Building,
        DuplicateRules::classify(0, 0, true, false)
    );
}

// --- system subtrees ---

#[test]
fn inside_windows_is_a_system_subtree() {
    let windows = windows_folder();
    assert!(DuplicateRules::is_system_subtree(&format!(
        r"{windows}\WINSXS\SOMETHING.DLL"
    )));
}

/// The profile root is in the protected locations and deliberately not here. It is where a
/// person's duplicates actually live — downloads saved twice, photos imported twice — so skipping
/// it would leave this feature with nothing to find.
#[test]
fn the_profile_root_is_not_a_system_subtree() {
    let profile = profile_folder();
    assert!(!DuplicateRules::is_system_subtree(&format!(
        r"{profile}\DOWNLOADS\REPORT.PDF"
    )));
}

/// Held paths go whether or not system folders are being skipped: those files have been deleted
/// as far as the user is concerned, and offering to delete one again reads as a delete that
/// silently failed.
#[test]
fn a_held_path_is_always_excluded() {
    for skip_system_folders in [true, false] {
        assert!(
            DuplicateRules::is_excluded(
                r"C:\.BERTBROWSER-TRASH\DELETE-ABC\REPORT.PDF",
                skip_system_folders
            ),
            "skip_system_folders = {skip_system_folders}"
        );
    }
}

#[test]
fn system_folders_are_only_excluded_when_asked() {
    let windows = windows_folder();
    let path = format!(r"{windows}\WINSXS\SOMETHING.DLL");

    assert!(DuplicateRules::is_excluded(&path, true));
    assert!(!DuplicateRules::is_excluded(&path, false));
}

// --- CanRemove ---

/// The guard the whole destructive half rests on. The point of the feature is to reclaim what a
/// redundant copy costs, so one copy always stays; a batch that took all of them would destroy
/// the only remaining instance of a file the user was just told they had several of.
#[test]
fn every_copy_ticked_is_refused() {
    assert!(!DuplicateRules::can_remove(3, 3));
}

#[test]
fn all_but_one_ticked_is_allowed() {
    assert!(DuplicateRules::can_remove(3, 2));
}

#[test]
fn nothing_ticked_is_not_work() {
    assert!(!DuplicateRules::can_remove(3, 0));
}

// --- ChooseKeeper ---

#[test]
fn newest_keeps_the_latest_modified() {
    let group = group(vec![
        file(
            r"C:\a\one.bin",
            Some(FileTime::from_ymd_hms(2020, 1, 1, 0, 0, 0)),
            4096,
        ),
        file(
            r"C:\b\two.bin",
            Some(FileTime::from_ymd_hms(2024, 6, 1, 0, 0, 0)),
            4096,
        ),
        file(
            r"C:\c\three.bin",
            Some(FileTime::from_ymd_hms(2022, 1, 1, 0, 0, 0)),
            4096,
        ),
    ]);

    assert_eq!(
        1,
        DuplicateRules::choose_keeper(&group, KeepStrategy::Newest)
    );
}

#[test]
fn oldest_keeps_the_earliest_modified() {
    let group = group(vec![
        file(
            r"C:\a\one.bin",
            Some(FileTime::from_ymd_hms(2020, 1, 1, 0, 0, 0)),
            4096,
        ),
        file(
            r"C:\b\two.bin",
            Some(FileTime::from_ymd_hms(2024, 6, 1, 0, 0, 0)),
            4096,
        ),
    ]);

    assert_eq!(
        0,
        DuplicateRules::choose_keeper(&group, KeepStrategy::Oldest)
    );
}

#[test]
fn shallowest_keeps_the_shortest_path() {
    let group = group(vec![
        file(r"C:\archive\deep\deeper\one.bin", None, 4096),
        file(r"C:\one.bin", None, 4096),
    ]);

    assert_eq!(
        1,
        DuplicateRules::choose_keeper(&group, KeepStrategy::Shallowest)
    );
}

/// Several copies written by one unzip share a timestamp to the tick, which is exactly the case
/// this feature is most often pointed at. Without a tiebreak the keeper would depend on the order
/// the rows happened to arrive in, and pressing the button twice could tick a different set — an
/// auto-selection nobody could trust before a delete.
#[test]
fn identical_timestamps_tiebreak_on_path_so_the_answer_is_stable() {
    let stamp = stamp();
    let forwards = group(vec![
        file(r"C:\b\x.bin", Some(stamp), 4096),
        file(r"C:\a\x.bin", Some(stamp), 4096),
    ]);
    let backwards = group(vec![
        file(r"C:\a\x.bin", Some(stamp), 4096),
        file(r"C:\b\x.bin", Some(stamp), 4096),
    ]);

    assert_eq!(
        forwards.files[DuplicateRules::choose_keeper(&forwards, KeepStrategy::Newest)].display_path,
        backwards.files[DuplicateRules::choose_keeper(&backwards, KeepStrategy::Newest)]
            .display_path
    );
}

// --- WastedBytes ---

/// One copy is wanted; the rest are the waste. Counting all of them would tell the user they
/// could reclaim space that keeping the file at all costs.
#[test]
fn wasted_bytes_excludes_the_copy_being_kept() {
    let group = group(vec![
        file(r"C:\a\x.bin", None, 1000),
        file(r"C:\b\x.bin", None, 1000),
        file(r"C:\c\x.bin", None, 1000),
    ]);

    assert_eq!(2000, group.wasted_bytes());
}
