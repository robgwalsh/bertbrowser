// Port of tests/BertBrowser.Core.Tests/ChangeRecorderTests.cs.
use std::sync::{Arc, Mutex};

use bb_core::FileTime;
use bb_core::changes::{ChangeEvent, ChangeKind, ChangeLogPolicy, ChangeRecorder};
use bb_core::data::ChangeLogRepository;
use bb_paths::PathKey;
use bb_test_support::TempDb;

const DATA_DIR: &str = r"C:\Users\Rob\.bertbrowser";

fn t0() -> FileTime {
    FileTime::from_ymd_hms(2026, 9, 2, 12, 0, 0)
}

struct Fixture {
    t: TempDb,
    repo: Arc<ChangeLogRepository>,
    policy: Arc<Mutex<ChangeLogPolicy>>,
    now: Arc<Mutex<FileTime>>,
}

impl Fixture {
    fn new() -> Fixture {
        let t = TempDb::new();
        let repo = Arc::new(ChangeLogRepository::new(t.db()));
        Fixture {
            t,
            repo,
            policy: Arc::new(Mutex::new(ChangeLogPolicy::from_hours(24).unwrap())),
            now: Arc::new(Mutex::new(t0())),
        }
    }

    fn recorder(&self) -> ChangeRecorder {
        self.recorder_for(self.repo.clone())
    }

    fn recorder_for(&self, repo: Arc<ChangeLogRepository>) -> ChangeRecorder {
        let policy = self.policy.clone();
        let now = self.now.clone();
        ChangeRecorder::new(
            repo,
            PathKey::canonicalize(DATA_DIR).unwrap(),
            move || *policy.lock().unwrap(),
            move || *now.lock().unwrap(),
        )
    }

    fn set_policy(&self, p: ChangeLogPolicy) {
        *self.policy.lock().unwrap() = p;
    }

    fn set_now(&self, n: FileTime) {
        *self.now.lock().unwrap() = n;
    }

    fn rows(&self) -> i64 {
        self.repo.count().unwrap()
    }

    /// Size and write time of the database and its WAL: what any write would move.
    fn snapshot(
        &self,
    ) -> (
        u64,
        Option<std::time::SystemTime>,
        i64,
        Option<std::time::SystemTime>,
    ) {
        let db = std::fs::metadata(&self.t.path).unwrap();
        let mut wal = self.t.path.as_os_str().to_owned();
        wal.push("-wal");
        match std::fs::metadata(&wal) {
            Ok(w) => (
                db.len(),
                db.modified().ok(),
                w.len() as i64,
                w.modified().ok(),
            ),
            Err(_) => (db.len(), db.modified().ok(), -1, None),
        }
    }
}

fn ev(display_path: &str, utc: Option<FileTime>) -> ChangeEvent {
    ChangeEvent {
        path_key: PathKey::canonicalize(display_path).unwrap(),
        display_path: display_path.to_string(),
        is_directory: false,
        hidden: false,
        kind: ChangeKind::Modified,
        old_display_path: None,
        utc: utc.unwrap_or_else(t0),
    }
}

#[test]
fn off_writes_nothing() {
    let f = Fixture::new();
    f.set_policy(ChangeLogPolicy::OFF);
    let mut recorder = f.recorder();

    recorder.add(ev(r"C:\Work\notes.txt", None));
    recorder.flush();

    assert_eq!(0, f.rows());
}

#[test]
fn on_writes_the_batch_on_flush() {
    let f = Fixture::new();
    let mut recorder = f.recorder();

    recorder.add(ev(r"C:\Work\a.txt", None));
    recorder.add(ev(r"C:\Work\b.txt", None));
    assert_eq!(0, f.rows());

    recorder.flush();

    assert_eq!(2, f.rows());
}

#[test]
fn turning_off_wipes_the_table_once() {
    let f = Fixture::new();
    let mut recorder = f.recorder();
    recorder.add(ev(r"C:\Work\a.txt", None));
    recorder.flush();
    assert_eq!(1, f.rows());

    // The verb arrives between two polls; whatever was buffered meanwhile must not land.
    recorder.add(ev(r"C:\Work\b.txt", None));
    f.set_policy(ChangeLogPolicy::OFF);
    recorder.flush();
    assert_eq!(0, f.rows());

    // Once. A later flush while off does not keep wiping.
    f.repo.record(&[ev(r"C:\Elsewhere\c.txt", None)]).unwrap();
    recorder.flush();
    assert_eq!(1, f.rows());
}

#[test]
fn on_then_off_before_any_flush_lands_nothing_and_wipes_nothing() {
    // The helper came up, was told "on", buffered a batch, and was told "off" before its first
    // poll wrote anything. Nothing was ever written by this recorder, so nothing of its own to wipe.
    let f = Fixture::new();
    let mut recorder = f.recorder();
    recorder.add(ev(r"C:\Work\a.txt", None));
    f.repo
        .record(&[ev(r"C:\Elsewhere\seeded.txt", None)])
        .unwrap();

    f.set_policy(ChangeLogPolicy::OFF);
    recorder.flush();

    assert_eq!(1, f.rows());
}

#[test]
fn data_directory_is_never_recorded() {
    let f = Fixture::new();
    let mut recorder = f.recorder();

    recorder.add(ev(&format!(r"{DATA_DIR}\bertbrowser.db-wal"), None));
    recorder.add(ev(&format!(r"{DATA_DIR}\settings.json"), None));
    recorder.add(ev(r"C:\Users\Rob\notes.txt", None));
    recorder.flush();

    assert_eq!(1, f.rows());
}

#[test]
fn empty_flush_leaves_the_database_file_untouched() {
    let f = Fixture::new();
    let mut recorder = f.recorder();
    recorder.add(ev(r"C:\Work\a.txt", None));
    recorder.flush();
    f.t.db.clear_pool();
    let before = f.snapshot();

    f.set_now(t0().add_minutes(5)); // a prune would be due, if anything ran at all
    recorder.flush();
    recorder.flush();

    assert_eq!(before, f.snapshot());
}

#[test]
fn prune_runs_with_the_first_batch_and_then_once_per_interval() {
    let f = Fixture::new();
    f.set_policy(ChangeLogPolicy::from_hours(1).unwrap());
    let mut recorder = f.recorder();
    f.repo
        .record(&[ev(r"C:\Old\one.txt", Some(t0().add_hours(-30)))])
        .unwrap();

    recorder.add(ev(r"C:\Work\a.txt", Some(t0())));
    recorder.flush();
    assert_eq!(1, f.rows()); // the 30-hour-old row went with the first flush

    f.repo
        .record(&[ev(r"C:\Old\two.txt", Some(t0().add_hours(-30)))])
        .unwrap();
    f.set_now(t0().add_secs(30));
    recorder.add(ev(r"C:\Work\b.txt", Some(t0().add_secs(30))));
    recorder.flush();
    assert_eq!(3, f.rows()); // inside the interval: nothing pruned

    f.set_now(t0().add_secs(61));
    recorder.add(ev(r"C:\Work\c.txt", Some(t0().add_secs(61))));
    recorder.flush();
    assert_eq!(3, f.rows()); // a, b, c: the stale row went on the interval
}

#[test]
fn a_repository_failure_disables_the_recorder_and_never_throws() {
    // A database the app never migrated: no fs_change table. The tail must survive this.
    let f = Fixture::new();
    let bare = TempDb::bare();
    let mut recorder = f.recorder_for(Arc::new(ChangeLogRepository::new(bare.db())));

    recorder.add(ev(r"C:\Work\a.txt", None));
    recorder.flush();

    assert!(recorder.is_disabled());
    recorder.add(ev(r"C:\Work\b.txt", None));
    recorder.flush(); // still nothing
}
