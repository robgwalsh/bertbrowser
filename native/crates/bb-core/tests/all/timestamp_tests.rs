// Port of tests/BertBrowser.Core.Tests/TimestampTests.cs. Setting a file's own dates: the planner
// against a fake disk, the rest against a real one.
use std::collections::HashMap;
use std::fs::FileTimes;
use std::os::windows::fs::{FileTimesExt, MetadataExt};
use std::path::PathBuf;

use bb_core::FileTime;
use bb_core::attributes::{ARCHIVE, DIRECTORY, REPARSE_POINT};
use bb_core::fs::DirectoryRemoval;
use bb_core::time::TICKS_PER_HOUR;
use bb_core::timestamps::{
    TimestampChange, TimestampEntry, TimestampExecutor, TimestampPlanner, TimestampProbe,
    TimestampRejection,
};
use bb_paths::fold_upper;
use bb_test_support::unique_temp_path;

/// 2020-05-06 12:00:00 UTC.
fn noon() -> FileTime {
    FileTime::from_ymd_hms(2020, 5, 6, 12, 0, 0)
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-dates", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn p(&self, name: &str) -> String {
        self.root.join(name).to_string_lossy().into_owned()
    }

    /// A file stamped noon, both dates.
    fn file(&self, name: &str) -> String {
        let path = self.p(name);
        std::fs::write(&path, name).unwrap();
        set_times(&path, noon(), noon());
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = DirectoryRemoval::remove_tree(&self.root.to_string_lossy());
    }
}

fn set_times(path: &str, modified_utc: FileTime, created_utc: FileTime) {
    let times = FileTimes::new()
        .set_modified(modified_utc.to_system_time())
        .set_created(created_utc.to_system_time());
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(times)
        .unwrap();
}

fn last_write_utc(path: &str) -> FileTime {
    FileTime(std::fs::metadata(path).unwrap().last_write_time() as i64)
}

fn creation_utc(path: &str) -> FileTime {
    FileTime(std::fs::metadata(path).unwrap().creation_time() as i64)
}

#[derive(Default)]
struct FakeProbe {
    entries: HashMap<String, TimestampEntry>,
    taken: HashMap<String, FileTime>,
}

impl FakeProbe {
    fn entry(&mut self, path: &str, attributes: u32, modified: FileTime, created: FileTime) {
        self.entries.insert(
            fold_upper(path),
            TimestampEntry {
                attributes,
                modified_utc: modified,
                created_utc: created,
            },
        );
    }
}

impl TimestampProbe for FakeProbe {
    fn find(&self, path: &str) -> Option<TimestampEntry> {
        self.entries.get(&fold_upper(path)).copied()
    }

    fn date_taken_of(&self, path: &str) -> Option<FileTime> {
        self.taken.get(&fold_upper(path)).copied()
    }
}

fn disk() -> FakeProbe {
    let mut probe = FakeProbe::default();
    probe.entry(r"C:\p\a.jpg", ARCHIVE, noon(), noon().add_days(-1));
    probe.entry(r"C:\p\folder", DIRECTORY, noon(), noon());
    probe.entry(r"C:\p\link", REPARSE_POINT, noon(), noon());
    probe.taken.insert(
        fold_upper(r"C:\p\a.jpg"),
        FileTime::from_ymd_hms(2021, 6, 5, 4, 3, 2),
    );
    probe
}

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ---- planner --------------------------------------------------------------------------------

#[test]
fn a_value_is_local_time_and_only_the_ticked_dates_are_planned() {
    let local = FileTime::from_ymd_hms(2022, 2, 3, 4, 5, 6);
    let probe = disk();
    let plan = TimestampPlanner::new(&probe).plan(
        &paths(&[r"C:\p\a.jpg", r"C:\p\folder"]),
        &TimestampChange::value(true, false, local),
    );

    assert_eq!(2, plan.items.len());
    for item in &plan.items {
        assert_eq!(Some(local.local_to_utc()), item.modified_utc);
        assert_eq!(None, item.created_utc);
    }
    assert!(plan.items[1].is_directory);
}

#[test]
fn a_shift_moves_each_item_from_its_own_dates() {
    let probe = disk();
    let plan = TimestampPlanner::new(&probe).plan(
        &paths(&[r"C:\p\a.jpg"]),
        &TimestampChange::shift(true, true, -2 * TICKS_PER_HOUR),
    );

    assert_eq!(1, plan.items.len());
    let item = &plan.items[0];
    assert_eq!(Some(noon().add_hours(-2)), item.modified_utc);
    assert_eq!(Some(noon().add_days(-1).add_hours(-2)), item.created_utc);
}

#[test]
fn date_taken_is_per_picture_and_anything_without_one_is_left_out_by_name() {
    let probe = disk();
    let plan = TimestampPlanner::new(&probe).plan(
        &paths(&[r"C:\p\a.jpg", r"C:\p\folder"]),
        &TimestampChange::date_taken(true, false),
    );

    let taken = FileTime::from_ymd_hms(2021, 6, 5, 4, 3, 2).local_to_utc();
    assert_eq!(1, plan.items.len());
    assert_eq!(Some(taken), plan.items[0].modified_utc);
    assert_eq!(1, plan.rejected.len());
    assert_eq!(TimestampRejection::NoDateTaken, plan.rejected[0].reason);
}

#[test]
fn what_cannot_be_dated_is_refused_with_a_reason() {
    let change = TimestampChange::value(true, false, FileTime::from_ymd(2022, 1, 1));
    let probe = disk();
    let plan = TimestampPlanner::new(&probe).plan(
        &paths(&[r"C:\p\link", r"C:\p\gone.txt", r"C:\p\pack.zip\inner.txt"]),
        &change,
    );

    assert!(!plan.has_work());
    assert_eq!(
        vec![
            TimestampRejection::Link,
            TimestampRejection::Missing,
            TimestampRejection::InsideArchive
        ],
        plan.rejected.iter().map(|r| r.reason).collect::<Vec<_>>()
    );
}

#[test]
fn a_date_windows_cannot_store_and_a_date_already_there_are_both_left_out() {
    let probe = disk();
    let planner = TimestampPlanner::new(&probe);

    let ancient = planner.plan(
        &paths(&[r"C:\p\a.jpg"]),
        &TimestampChange::value(true, false, FileTime::from_ymd(1500, 1, 1)),
    );
    assert_eq!(1, ancient.rejected.len());
    assert_eq!(TimestampRejection::OutOfRange, ancient.rejected[0].reason);

    let same = planner.plan(
        &paths(&[r"C:\p\a.jpg"]),
        &TimestampChange::value(true, false, noon().to_local()),
    );
    assert_eq!(1, same.rejected.len());
    assert_eq!(TimestampRejection::Unchanged, same.rejected[0].reason);

    assert!(
        !planner
            .plan(
                &paths(&[r"C:\p\a.jpg"]),
                &TimestampChange::date_taken(false, false)
            )
            .has_work()
    );
}

// ---- executor -------------------------------------------------------------------------------

#[test]
fn dates_are_set_on_files_and_folders_and_reverting_is_its_own_redo() {
    let x = Fixture::new();
    let file = x.file("a.txt");
    let folder = x.p("sub");
    std::fs::create_dir(&folder).unwrap();
    let folder_created = creation_utc(&folder);

    let when = FileTime::from_ymd_hms(2019, 3, 4, 5, 6, 7);
    let when_utc = when.local_to_utc();

    let planner = TimestampPlanner::local();
    let executor = TimestampExecutor::local();
    let outcome = executor.execute(&planner.plan(
        &[file.clone(), folder.clone()],
        &TimestampChange::value(true, false, when),
    ));

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_eq!(when_utc, last_write_utc(&file));
    assert_eq!(when_utc, last_write_utc(&folder));
    assert_eq!(noon(), creation_utc(&file));
    assert_eq!(folder_created, creation_utc(&folder));
    assert_eq!("a.txt", std::fs::read_to_string(&file).unwrap());

    let undone = executor.revert(&outcome.stamped);
    assert!(undone.failed.is_empty(), "{:?}", undone.failed);
    assert_eq!(noon(), last_write_utc(&file));

    let redone = executor.revert(&undone.stamped);
    assert!(redone.failed.is_empty(), "{:?}", redone.failed);
    assert_eq!(when_utc, last_write_utc(&file));
}

#[test]
fn a_revert_leaves_alone_a_file_something_else_has_written_since() {
    let x = Fixture::new();
    let file = x.file("a.txt");
    let executor = TimestampExecutor::local();
    let outcome = executor.execute(&TimestampPlanner::local().plan(
        std::slice::from_ref(&file),
        &TimestampChange::shift(true, false, 24 * TICKS_PER_HOUR),
    ));

    let later = noon().add_days(40);
    set_times(&file, later, creation_utc(&file));

    let undone = executor.revert(&outcome.stamped);
    assert!(undone.stamped.is_empty());
    assert_eq!(1, undone.failed.len());
    assert!(undone.failed[0].message.contains("has changed since"));
    assert_eq!(later, last_write_utc(&file));
}

#[test]
fn one_missing_file_does_not_stop_the_others() {
    let x = Fixture::new();
    let kept = x.file("kept.txt");
    let gone = x.file("gone.txt");
    let plan = TimestampPlanner::local().plan(
        &[gone.clone(), kept.clone()],
        &TimestampChange::shift(true, false, TICKS_PER_HOUR),
    );

    std::fs::remove_file(&gone).unwrap();
    let outcome = TimestampExecutor::local().execute(&plan);

    assert_eq!(1, outcome.stamped.len());
    assert_eq!(kept, outcome.stamped[0].path);
    assert_eq!(1, outcome.failed.len());
    assert_eq!(gone, outcome.failed[0].path);
    assert_eq!(noon().add_hours(1), last_write_utc(&kept));
}
