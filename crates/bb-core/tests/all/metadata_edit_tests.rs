// Port of tests/BertBrowser.Core.Tests/MetadataEditTests.cs. The planner against a fake disk, and
// the executor and its undo record against a real one.
use std::collections::HashMap;
use std::fs::FileTimes;
use std::os::windows::fs::{FileTimesExt, MetadataExt};
use std::path::PathBuf;

use bb_core::FileTime;
use bb_core::attributes::{
    ARCHIVE, DIRECTORY, HIDDEN, NORMAL, READONLY, RECALL_ON_DATA_ACCESS, REPARSE_POINT,
};
use bb_core::cancel::CancelToken;
use bb_core::delete::{DeleteExecutor, FileSystemDeleteProbe};
use bb_core::fs::{DirectoryRemoval, EntryStamp, ReadOnlyFile};
use bb_core::metadata::{
    EDITED_MARKER, HeldVersion, MetadataCodecs, MetadataDocument, MetadataEdit,
    MetadataEditExecutor, MetadataEditOutcome, MetadataEditPlan, MetadataEditPlanner,
    MetadataField, MetadataProbe, MetadataRejection, REPLACED_MARKER,
};
use bb_core::rename::RenameExecutor;
use bb_core::time::TICKS_PER_HOUR;
use bb_core::transfer::TransferExecutor;
use bb_core::undo::{MetadataEditRecord, PlainUndoHost, UndoKind, UndoableRecord};
use bb_test_support::metadata_fixtures as fx;
use bb_test_support::unique_temp_path;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-metadata", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn p(&self, name: &str) -> String {
        self.root.join(name).to_string_lossy().into_owned()
    }

    fn jpeg(&self, name: &str) -> String {
        self.jpeg_of(name, fx::CAMERA_JPEG)
    }

    fn jpeg_of(&self, name: &str, fixture: &str) -> String {
        let path = self.p(name);
        std::fs::write(&path, fx::jpeg(fixture)).unwrap();
        path
    }

    fn mp3(&self, name: &str) -> String {
        let path = self.p(name);
        std::fs::write(
            &path,
            fx::mp3(&[("TIT2", "Song"), ("TPE1", "Artist")], false),
        )
        .unwrap();
        path
    }

    fn leftovers(&self) -> Vec<String> {
        std::fs::read_dir(&self.root)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".bertbrowser-"))
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    let mut permissions = meta.permissions();
                    if permissions.readonly() {
                        #[allow(clippy::permissions_set_readonly_false)]
                        permissions.set_readonly(false);
                        let _ = std::fs::set_permissions(entry.path(), permissions);
                    }
                }
            }
        }
        let _ = DirectoryRemoval::remove_tree(&self.root.to_string_lossy());
    }
}

fn edit(changes: &[(MetadataField, &str)]) -> MetadataEdit {
    MetadataEdit::of(changes)
}

fn read(path: &str) -> MetadataDocument {
    let mut stream = ReadOnlyFile::try_open(path, false).expect("open");
    MetadataCodecs::for_path(path)
        .expect("codec")
        .read(&mut stream)
        .expect("read")
}

fn plan_for(paths: &[&str], edit: &MetadataEdit) -> MetadataEditPlan {
    let paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
    MetadataEditPlanner::local().plan(&paths, edit, None)
}

fn execute(plan: &MetadataEditPlan) -> MetadataEditOutcome {
    MetadataEditExecutor::local().execute(plan, &CancelToken::none(), None, None)
}

fn attributes(path: &str) -> u32 {
    std::fs::metadata(path).unwrap().file_attributes()
}

fn set_attributes(path: &str, attributes: u32) {
    use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: a NUL-terminated path.
    assert_ne!(0, unsafe { SetFileAttributesW(wide.as_ptr(), attributes) });
}

fn creation_utc(path: &str) -> FileTime {
    FileTime(std::fs::metadata(path).unwrap().creation_time() as i64)
}

fn name_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

// ---- planner ----------------------------------------------------------------------------------

struct FakeProbe(HashMap<String, u32>);

impl MetadataProbe for FakeProbe {
    fn attributes_of(&self, path: &str) -> Option<u32> {
        self.0.get(path).copied()
    }

    fn length_of(&self, _path: &str) -> i64 {
        100
    }
}

#[test]
fn the_planner_takes_what_it_can_and_says_why_not_for_the_rest() {
    let recall = RECALL_ON_DATA_ACCESS;
    let probe = FakeProbe(HashMap::from([
        (r"C:\p\a.jpg".to_string(), ARCHIVE),
        (r"C:\p\song.mp3".to_string(), NORMAL),
        (r"C:\p\folder.jpg".to_string(), DIRECTORY),
        (r"C:\p\link.jpg".to_string(), REPARSE_POINT),
        (r"C:\p\cloud.jpg".to_string(), ARCHIVE | recall),
        (r"C:\p\locked.jpg".to_string(), READONLY),
        (r"C:\p\notes.txt".to_string(), NORMAL),
    ]));
    let planner = MetadataEditPlanner::new(&probe);

    let paths: Vec<String> = [
        r"C:\p\a.jpg",
        r"C:\p\song.mp3",
        r"C:\p\folder.jpg",
        r"C:\p\link.jpg",
        r"C:\p\cloud.jpg",
        r"C:\p\locked.jpg",
        r"C:\p\notes.txt",
        r"C:\p\gone.jpg",
        r"C:\p\pack.zip\inner.jpg",
        r"C:\p\A.JPG",
    ]
    .iter()
    .map(|p| p.to_string())
    .collect();
    let plan = planner.plan(
        &paths,
        &edit(&[(MetadataField::Title, "T"), (MetadataField::Album, "A")]),
        None,
    );

    let planned: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(vec![r"C:\p\a.jpg", r"C:\p\song.mp3"], planned);
    assert_eq!(200, plan.rewrite_bytes());

    // Each file gets only the part of the edit its kind can hold.
    let first: Vec<MetadataField> = plan.files[0].edit.changes.keys().copied().collect();
    assert_eq!(vec![MetadataField::Title], first);
    let mut second: Vec<MetadataField> = plan.files[1].edit.changes.keys().copied().collect();
    second.sort();
    assert_eq!(vec![MetadataField::Title, MetadataField::Album], second);

    let why: HashMap<String, MetadataRejection> = plan
        .rejected
        .iter()
        .map(|r| (name_of(&r.path), r.reason))
        .collect();
    assert_eq!(MetadataRejection::IsFolder, why["folder.jpg"]);
    assert_eq!(MetadataRejection::Link, why["link.jpg"]);
    assert_eq!(MetadataRejection::CloudPlaceholder, why["cloud.jpg"]);
    assert_eq!(MetadataRejection::ReadOnly, why["locked.jpg"]);
    assert_eq!(MetadataRejection::Unsupported, why["notes.txt"]);
    assert_eq!(MetadataRejection::Missing, why["gone.jpg"]);
    assert_eq!(MetadataRejection::InsideArchive, why["inner.jpg"]);

    // Standing on an archive is not being inside one: a book is a zip, and is still a file.
    assert!(MetadataEditPlanner::refusal(r"C:\p\book.epub", Some(NORMAL), false).is_none());
    assert!(MetadataEditPlanner::refusal(r"C:\p\book.epub\cover.jpg", None, false).is_some());
    for rejected in &plan.rejected {
        assert!(!rejected.message.trim().is_empty(), "{}", rejected.path);
    }
}

#[test]
fn a_field_no_selected_file_can_hold_leaves_nothing_to_do() {
    let probe = FakeProbe(HashMap::from([(r"C:\p\a.jpg".to_string(), NORMAL)]));
    let planner = MetadataEditPlanner::new(&probe);
    let plan = planner.plan(
        &[r"C:\p\a.jpg".to_string()],
        &edit(&[(MetadataField::Album, "A")]),
        None,
    );

    assert!(!plan.has_work());
    assert_eq!(1, plan.rejected.len());
    assert_eq!(MetadataRejection::NothingApplies, plan.rejected[0].reason);
}

// ---- executor ---------------------------------------------------------------------------------

#[test]
fn an_edit_lands_and_the_original_is_held_beside_it() {
    let x = Fixture::new();
    let photo = x.jpeg("photo.jpg");
    let song = x.mp3("song.mp3");
    let before = std::fs::read(&photo).unwrap();

    let outcome = execute(&plan_for(
        &[&photo, &song],
        &edit(&[(MetadataField::Title, "New")]),
    ));

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_eq!(2, outcome.edited.len());
    assert_eq!("New", read(&photo).get(MetadataField::Title));
    assert_eq!("New", read(&song).get(MetadataField::Title));
    assert_eq!("Artist", read(&song).get(MetadataField::Artist));

    let held = outcome.edited.iter().find(|e| e.path == photo).unwrap();
    assert!(held.held_path.contains(REPLACED_MARKER));
    assert!(held.held_path.ends_with(".jpg"));
    assert_eq!(before, std::fs::read(&held.held_path).unwrap());
    assert!(held.in_place.matches(&photo));
}

#[test]
fn undo_restores_the_exact_bytes_and_redo_brings_the_edit_back() {
    let x = Fixture::new();
    let photo = x.jpeg("photo.jpg");
    let original = std::fs::read(&photo).unwrap();
    let executor = MetadataEditExecutor::local();

    let outcome = execute(&plan_for(
        &[&photo],
        &edit(&[(MetadataField::Title, "New")]),
    ));
    let edited = std::fs::read(&photo).unwrap();

    let undone = executor.undo(&outcome.edited);
    assert!(undone.failed.is_empty(), "{:?}", undone.failed);
    assert_eq!(original, std::fs::read(&photo).unwrap());
    assert!(undone.swapped[0].held_path.contains(EDITED_MARKER));

    let redone = executor.redo(&undone.swapped);
    assert!(redone.failed.is_empty(), "{:?}", redone.failed);
    assert_eq!(edited, std::fs::read(&photo).unwrap());

    MetadataEditExecutor::commit_staging(&redone.swapped);
    assert!(x.leftovers().is_empty());
}

#[test]
fn the_swap_keeps_what_a_fresh_file_would_lose() {
    let x = Fixture::new();
    let photo = x.jpeg("photo.jpg");
    let created = FileTime::from_ymd_hms(2019, 1, 2, 3, 4, 5);
    let times = FileTimes::new().set_created(created.to_system_time());
    std::fs::OpenOptions::new()
        .write(true)
        .open(&photo)
        .unwrap()
        .set_times(times)
        .unwrap();
    let zone = format!("{photo}:Zone.Identifier");
    std::fs::write(&zone, "[ZoneTransfer]\r\nZoneId=3\r\n").unwrap();
    set_attributes(&photo, HIDDEN | ARCHIVE);

    let executor = MetadataEditExecutor::local();
    let outcome = execute(&plan_for(
        &[&photo],
        &edit(&[(MetadataField::Title, "New")]),
    ));

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_eq!(created, creation_utc(&photo));
    assert!(attributes(&photo) & HIDDEN != 0);
    assert_eq!(
        "[ZoneTransfer]\r\nZoneId=3\r\n",
        std::fs::read_to_string(&zone).unwrap()
    );

    // And the same again coming back.
    executor.undo(&outcome.edited);
    assert_eq!(created, creation_utc(&photo));
    assert!(attributes(&photo) & HIDDEN != 0);
    assert_eq!(
        "[ZoneTransfer]\r\nZoneId=3\r\n",
        std::fs::read_to_string(&zone).unwrap()
    );
}

#[test]
fn a_held_version_is_hidden_and_never_comes_back_hidden() {
    let x = Fixture::new();
    let photo = x.jpeg("photo.jpg");
    let executor = MetadataEditExecutor::local();

    let outcome = execute(&plan_for(
        &[&photo],
        &edit(&[(MetadataField::Title, "New")]),
    ));
    assert!(attributes(&outcome.edited[0].held_path) & HIDDEN != 0);
    assert!(attributes(&photo) & HIDDEN == 0);

    let undone = executor.undo(&outcome.edited);
    assert!(attributes(&undone.swapped[0].held_path) & HIDDEN != 0);
    assert!(attributes(&photo) & HIDDEN == 0);

    let redone = executor.redo(&undone.swapped);
    assert!(attributes(&photo) & HIDDEN == 0);

    MetadataEditExecutor::commit_staging(&redone.swapped);
    assert!(x.leftovers().is_empty());
}

#[test]
fn one_bad_file_does_not_stop_the_others_and_is_left_exactly_as_it_was() {
    let x = Fixture::new();
    let good = x.jpeg("good.jpg");
    let bad = x.p("bad.jpg");
    std::fs::write(&bad, "this is not a picture at all").unwrap();

    let outcome = execute(&plan_for(
        &[&bad, &good],
        &edit(&[(MetadataField::Title, "New")]),
    ));

    assert_eq!(1, outcome.edited.len());
    assert_eq!(good, outcome.edited[0].path);
    assert_eq!(1, outcome.failed.len());
    assert_eq!(bad, outcome.failed[0].path);
    assert_eq!(
        "this is not a picture at all",
        std::fs::read_to_string(&bad).unwrap()
    );
    assert_eq!("New", read(&good).get(MetadataField::Title));

    // Only the good file's held original is beside it; the failed rewrite was cleaned up.
    assert_eq!(1, x.leftovers().len());
}

#[test]
fn the_executor_asks_disk_again_rather_than_trusting_the_plan() {
    let x = Fixture::new();
    let photo = x.jpeg("photo.jpg");
    let plan = plan_for(&[&photo], &edit(&[(MetadataField::Title, "New")]));

    set_attributes(&photo, READONLY);
    let outcome = execute(&plan);

    assert!(outcome.edited.is_empty());
    assert_eq!(1, outcome.failed.len());
    assert!(
        outcome.failed[0].message.contains("read-only"),
        "{}",
        outcome.failed[0].message
    );
    assert_eq!("Old title", read(&photo).get(MetadataField::Title));
    assert!(x.leftovers().is_empty());
}

#[test]
fn a_cancel_before_the_start_changes_nothing() {
    let x = Fixture::new();
    let photo = x.jpeg("photo.jpg");
    let cancelled = CancelToken::new();
    cancelled.cancel();

    let outcome = MetadataEditExecutor::local().execute(
        &plan_for(&[&photo], &edit(&[(MetadataField::Title, "New")])),
        &cancelled,
        None,
        None,
    );

    assert!(outcome.cancelled);
    assert!(!outcome.can_undo());
    assert_eq!("Old title", read(&photo).get(MetadataField::Title));
    assert!(x.leftovers().is_empty());
}

#[test]
fn undo_refuses_a_file_somebody_else_has_changed_since_and_keeps_the_held_copy() {
    let x = Fixture::new();
    let photo = x.jpeg("photo.jpg");
    let outcome = execute(&plan_for(
        &[&photo],
        &edit(&[(MetadataField::Title, "New")]),
    ));

    std::fs::write(&photo, fx::jpeg(fx::PLAIN_JPEG)).unwrap();
    let undone = MetadataEditExecutor::local().undo(&outcome.edited);

    assert!(undone.swapped.is_empty());
    assert_eq!(1, undone.failed.len());
    assert!(
        undone.failed[0].message.contains("has changed since"),
        "{}",
        undone.failed[0].message
    );
    assert_eq!(fx::jpeg(fx::PLAIN_JPEG), std::fs::read(&photo).unwrap());
    assert!(std::path::Path::new(&outcome.edited[0].held_path).exists());
}

#[test]
fn a_date_shift_moves_each_picture_from_its_own_date_and_skips_one_with_none() {
    let x = Fixture::new();
    let camera = x.jpeg("camera.jpg");
    let plain = x.jpeg_of("plain.jpg", fx::PLAIN_JPEG);
    let song = x.mp3("song.mp3");

    let plan = plan_for(
        &[&camera, &plain, &song],
        &MetadataEdit {
            shift_date_taken: Some(-5 * TICKS_PER_HOUR),
            ..MetadataEdit::none()
        },
    );

    // A song has no date taken; the planner drops it rather than the executor failing it.
    let planned: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(vec![camera.as_str(), plain.as_str()], planned);
    assert_eq!(1, plan.rejected.len());
    assert_eq!(MetadataRejection::NothingApplies, plan.rejected[0].reason);

    let outcome = execute(&plan);

    assert_eq!(1, outcome.edited.len());
    assert_eq!(camera, outcome.edited[0].path);
    assert_eq!(
        "2021-06-04 23:03:02",
        read(&camera).get(MetadataField::DateTaken)
    );
    assert_eq!(1, outcome.failed.len());
    assert!(
        outcome.failed[0].message.contains("no date taken"),
        "{}",
        outcome.failed[0].message
    );
}

#[test]
fn removing_a_location_is_undoable_like_any_other_edit() {
    let x = Fixture::new();
    let photo = x.jpeg("photo.jpg");
    let outcome = execute(&plan_for(
        &[&photo],
        &MetadataEdit {
            remove_location: true,
            ..MetadataEdit::none()
        },
    ));

    assert!(!read(&photo).has_location);
    MetadataEditExecutor::local().undo(&outcome.edited);
    assert!(read(&photo).has_location);
}

#[test]
fn an_edit_that_changes_nothing_is_neither_a_change_nor_a_failure() {
    let x = Fixture::new();
    let plain = x.jpeg_of("plain.jpg", fx::PLAIN_JPEG);
    let camera = x.jpeg("camera.jpg");
    let before = EntryStamp::of(&plain).unwrap();

    let outcome = execute(&plan_for(
        &[&plain, &camera],
        &MetadataEdit {
            remove_location: true,
            ..MetadataEdit::none()
        },
    ));

    assert_eq!(1, outcome.edited.len());
    assert_eq!(camera, outcome.edited[0].path);
    assert_eq!(vec![plain.clone()], outcome.unchanged);
    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert!(before.matches(&plain));

    // Writing a value a file already holds is the same non-event - the second time. The first
    // is a real change: the camera wrote the title in one of its two tags and this fills in
    // the other.
    let first = execute(&plan_for(
        &[&camera],
        &edit(&[(MetadataField::Title, "Old title")]),
    ));
    assert!(first.can_undo());

    let again = execute(&plan_for(
        &[&camera],
        &edit(&[(MetadataField::Title, "Old title")]),
    ));
    assert!(!again.can_undo());
    assert_eq!(vec![camera.clone()], again.unchanged);
    assert_eq!(2, x.leftovers().len());
}

#[test]
fn committing_erases_only_files_named_the_way_this_names_them() {
    let x = Fixture::new();
    let photo = x.jpeg("photo.jpg");
    MetadataEditExecutor::commit_staging(&[HeldVersion {
        path: photo.clone(),
        held_path: photo.clone(),
        in_place: EntryStamp::of(&photo).unwrap(),
    }]);

    assert!(std::path::Path::new(&photo).exists());
}

// ---- undo record -----------------------------------------------------------------------------

#[test]
fn the_record_steps_back_and_forth_and_retiring_it_erases_what_it_held() {
    let x = Fixture::new();
    let transfers = TransferExecutor::default();
    let deletes = DeleteExecutor::new(&FileSystemDeleteProbe)
        .protecting(&[])
        .staging_root(&x.root.to_string_lossy());
    let renames = RenameExecutor;
    let host = PlainUndoHost::without_archives(&transfers, &deletes, &renames);

    let photo = x.jpeg("photo.jpg");
    let song = x.mp3("song.mp3");
    let record = MetadataEditRecord::for_outcome(execute(&plan_for(
        &[&photo, &song],
        &edit(&[(MetadataField::Title, "New")]),
    )))
    .unwrap();

    assert_eq!(UndoKind::MetadataEdit, record.kind());
    assert!(record.is_applied());
    assert_eq!(2, record.item_count());
    assert_eq!(2, record.held().len());
    assert_eq!("Edit metadata of 2 files", record.description());

    let undone = record.step(&host);
    assert!(undone.report.clean(), "{:?}", undone.report.failures);
    assert!(!undone.next.is_applied());
    assert_eq!("Old title", read(&photo).get(MetadataField::Title));
    assert_eq!("Song", read(&song).get(MetadataField::Title));
    assert_eq!(
        vec![x.root.to_string_lossy().into_owned()],
        undone.refresh.directories
    );

    let redone = undone.next.step(&host);
    assert!(redone.next.is_applied());
    assert_eq!("New", read(&photo).get(MetadataField::Title));

    redone.next.retire();
    assert!(x.leftovers().is_empty(), "{:?}", x.leftovers());
}

#[test]
fn an_edit_that_changed_nothing_is_not_a_history_entry() {
    assert!(MetadataEditRecord::for_outcome(MetadataEditOutcome::empty()).is_none());
}
