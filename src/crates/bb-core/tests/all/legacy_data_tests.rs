// Not a port: the native app's first-launch rule. The C# app's data (its database, settings file,
// themes and templates) is moved aside into a dated folder, never deleted, and only once.
use std::path::{Path, PathBuf};

use bb_core::FileTime;
use bb_core::settings::{DataDirEntry, LegacyData, LegacyMove, LegacyPlan};
use bb_test_support::unique_temp_path;

fn today() -> FileTime {
    FileTime::from_ymd(2026, 10, 23)
}

fn files(names: &[&str]) -> Vec<DataDirEntry> {
    names.iter().map(|n| DataDirEntry::file(n)).collect()
}

fn moved_names(plan: &LegacyPlan) -> Vec<&str> {
    plan.moves.iter().map(|m| m.from.as_str()).collect()
}

// --- the plan ---

#[test]
fn a_fresh_data_dir_has_nothing_to_move() {
    let plan = LegacyData::plan(&[], today(), false);

    assert!(plan.is_empty());
    assert_eq!(None, plan.folder);
}

#[test]
fn the_marker_means_nothing_is_ever_moved_again() {
    // Once the native app has run, bertbrowser.db is its own, and moving it would be losing data.
    let plan = LegacyData::plan(&files(&["settings.json", "bertbrowser.db"]), today(), true);

    assert!(plan.is_empty());
}

#[test]
fn the_c_sharp_files_are_moved_into_a_dated_folder() {
    let mut entries = files(&[
        "bertbrowser.db",
        "bertbrowser.db-wal",
        "bertbrowser.db-shm",
        "settings.json",
        "startup.jitprofile",
        "instance.pipe",
    ]);
    entries.push(DataDirEntry::dir("themes"));
    entries.push(DataDirEntry::dir("templates"));

    let plan = LegacyData::plan(&entries, today(), false);

    assert_eq!(Some("legacy-20261023".to_string()), plan.folder);
    assert_eq!(
        vec![
            "bertbrowser.db",
            "bertbrowser.db-wal",
            "bertbrowser.db-shm",
            "settings.json",
            "themes",
            "templates",
        ],
        moved_names(&plan)
    );
    assert_eq!(
        r"legacy-20261023\settings.json",
        plan.moves
            .iter()
            .find(|m| m.from == "settings.json")
            .unwrap()
            .to
    );
}

#[test]
fn a_file_named_like_a_folder_is_not_one() {
    // A themes *file* is not the C# themes folder, and a settings.json *folder* is not its file.
    let entries = vec![
        DataDirEntry::file("themes"),
        DataDirEntry::dir("settings.json"),
    ];

    assert!(LegacyData::plan(&entries, today(), false).is_empty());
    assert!(!LegacyData::is_legacy_name("themes", false));
    assert!(LegacyData::is_legacy_name("themes", true));
    assert!(!LegacyData::is_legacy_name("settings.json", true));
    assert!(LegacyData::is_legacy_name("settings.json", false));
}

#[test]
fn names_are_matched_the_way_windows_matches_them() {
    let plan = LegacyData::plan(
        &files(&["Settings.JSON", "BERTBROWSER.DB-journal"]),
        today(),
        false,
    );

    assert_eq!(
        vec!["Settings.JSON", "BERTBROWSER.DB-journal"],
        moved_names(&plan)
    );
}

#[test]
fn a_database_backup_with_a_longer_name_is_not_the_database() {
    // Only bertbrowser.db and its journal siblings; "bertbrowser.db.2026-01-01.bak" is also
    // "bertbrowser.db*" and goes too — it is the C# app's as well — but "bertbrowser.database"
    // is some other file.
    assert!(LegacyData::is_legacy_name(
        "bertbrowser.db.2026-01-01.bak",
        false
    ));
    assert!(!LegacyData::is_legacy_name("bertbrowser.database", false));
    assert!(!LegacyData::is_legacy_name("other.db", false));
}

#[test]
fn an_existing_dated_folder_is_never_reused() {
    // A second move on the same day (a reinstall, say) must not land on top of the first.
    let mut entries = files(&["settings.json"]);
    entries.push(DataDirEntry::dir("LEGACY-20261023"));
    entries.push(DataDirEntry::dir("legacy-20261023-2"));

    let plan = LegacyData::plan(&entries, today(), false);

    assert_eq!(Some("legacy-20261023-3".to_string()), plan.folder);
    assert_eq!(r"legacy-20261023-3\settings.json", plan.moves[0].to);
}

#[test]
fn the_marker_file_itself_is_never_moved() {
    let plan = LegacyData::plan(&files(&[LegacyData::MARKER_FILE_NAME]), today(), false);

    assert!(plan.is_empty());
}

// --- the executor ---

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let p = unique_temp_path("bertbrowser-legacy", "");
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn file(&self, name: &str, content: &str) {
        std::fs::write(self.0.join(name), content).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn count_entries(dir: &Path) -> usize {
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        n += 1;
        if e.file_type().unwrap().is_dir() {
            n += count_entries(&e.path());
        }
    }
    n
}

#[test]
fn executing_moves_everything_aside_and_deletes_nothing() {
    let dir = TempDir::new();
    dir.file("bertbrowser.db", "db");
    dir.file("bertbrowser.db-wal", "wal");
    dir.file("settings.json", "{}");
    dir.file("startup.jitprofile", "jit");
    std::fs::create_dir(dir.path().join("themes")).unwrap();
    dir.file(r"themes\mine.json", "theme");
    let before = count_entries(dir.path());

    let outcome = LegacyData::migrate(dir.path(), today()).unwrap();

    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(4, outcome.moved.len());
    let legacy = dir.path().join("legacy-20261023");
    assert_eq!(
        "db",
        std::fs::read_to_string(legacy.join("bertbrowser.db")).unwrap()
    );
    assert_eq!(
        "{}",
        std::fs::read_to_string(legacy.join("settings.json")).unwrap()
    );
    assert_eq!(
        "theme",
        std::fs::read_to_string(legacy.join("themes").join("mine.json")).unwrap()
    );
    assert!(!dir.path().join("settings.json").exists());
    assert!(
        dir.path().join("startup.jitprofile").is_file(),
        "not ours to move"
    );
    assert!(dir.path().join(LegacyData::MARKER_FILE_NAME).is_file());
    // Everything that was there still is, plus the folder and the marker.
    assert_eq!(before + 2, count_entries(dir.path()));
}

#[test]
fn a_second_launch_moves_nothing_even_with_new_files_present() {
    let dir = TempDir::new();
    dir.file("settings.json", "old");
    LegacyData::migrate(dir.path(), today()).unwrap();
    dir.file("settings.json", "native");
    dir.file("bertbrowser.db", "native db");

    let outcome = LegacyData::migrate(dir.path(), today()).unwrap();

    assert!(outcome.moved.is_empty());
    assert_eq!(
        "native",
        std::fs::read_to_string(dir.path().join("settings.json")).unwrap()
    );
}

#[test]
fn a_destination_already_present_fails_that_item_only() {
    let dir = TempDir::new();
    dir.file("settings.json", "mine");
    dir.file("bertbrowser.db", "db");
    std::fs::create_dir(dir.path().join("legacy-x")).unwrap();
    dir.file(r"legacy-x\settings.json", "somebody else's");
    let plan = LegacyPlan {
        folder: Some("legacy-x".into()),
        moves: vec![
            LegacyMove::new("settings.json", r"legacy-x\settings.json"),
            LegacyMove::new("bertbrowser.db", r"legacy-x\bertbrowser.db"),
        ],
    };

    let outcome = LegacyData::execute(dir.path(), &plan);

    assert_eq!(1, outcome.failures.len());
    assert_eq!("settings.json", outcome.failures[0].from);
    assert_eq!(1, outcome.moved.len());
    assert_eq!(
        "mine",
        std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        "never overwritten, never deleted"
    );
    assert_eq!(
        "db",
        std::fs::read_to_string(dir.path().join("legacy-x").join("bertbrowser.db")).unwrap()
    );
    assert!(dir.path().join(LegacyData::MARKER_FILE_NAME).is_file());
}

#[test]
fn a_data_dir_that_does_not_exist_yet_is_created_and_marked() {
    let dir = TempDir::new();
    let data = dir.path().join("fresh");

    let outcome = LegacyData::migrate(&data, today()).unwrap();

    assert!(outcome.moved.is_empty());
    assert!(data.join(LegacyData::MARKER_FILE_NAME).is_file());
}

#[test]
fn the_folder_name_is_the_local_date() {
    assert_eq!(
        "legacy-20260105",
        LegacyData::folder_name(FileTime::from_ymd(2026, 1, 5), |_| false)
    );
    assert_eq!(
        "legacy-20260105-2",
        LegacyData::folder_name(FileTime::from_ymd(2026, 1, 5), |n| n == "legacy-20260105")
    );
}
