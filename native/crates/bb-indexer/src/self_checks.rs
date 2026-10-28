//! The reasons an unsupervised helper stops on its own. Spec: src/BertBrowser.Indexer/SelfChecks.cs.
//!
//! This process used to end when the app that launched it did. Now it outlives every app, so it
//! has to notice for itself when it has been left behind, and it has to notice *while idle*,
//! because the dangerous case is precisely a helper with no app connected. That is why this is a
//! timer and not a check between sessions. All three conditions are the same shape: something
//! about the installation this helper belongs to has changed underneath it, and carrying on would
//! mean an elevated process of an old build writing to a database of a new one.

use std::path::Path;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use bb_core::FileTime;
use bb_core::cancel::CancelToken;
use bb_core::data::Db;

/// How often to look for a reason to stop.
const INTERVAL: Duration = Duration::from_secs(30);

/// A reading of everything the checks look at. Each field is `None` when it could not be read:
/// only a reading that actually came back counts, since a busy or locked database (or a file
/// momentarily unreadable) is not a reason to stop indexing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub schema: Option<i64>,
    /// None when this executable's path is unknown; `Some(None)` when the file is gone.
    pub own_stamp: Option<Option<FileTime>>,
    pub app_exists: bool,
}

/// The decision, apart from the files and database it is made about.
pub fn should_stop(at_start: Reading, now: Reading) -> bool {
    // The app updated and migrated while this helper was idle. The app migrates before it ever
    // looks for an index helper, so there is a real window, and with nothing connected, an
    // unbounded one.
    if let (Some(before), Some(after)) = (at_start.schema, now.schema)
        && after != before
    {
        return true;
    }
    // Velopack replaces the install directory wholesale; an uninstall removes it.
    if let (Some(start_stamp), Some(now_stamp)) = (at_start.own_stamp, now.own_stamp)
        && (now_stamp.is_none() || now_stamp != start_stamp)
    {
        return true;
    }
    !now.app_exists
}

pub struct SelfChecks {
    stop: CancelToken,
    thread: Option<JoinHandle<()>>,
}

impl Drop for SelfChecks {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts the timer; cancels `lifetime` when the helper should stop. Dropping the result stops the
/// timer.
pub fn start(db: Arc<Db>, lifetime: CancelToken) -> SelfChecks {
    let own_path = std::env::current_exe().ok();
    let app_path = own_path
        .as_ref()
        .and_then(|p| p.parent())
        .map(|dir| dir.join("BertBrowser.exe"));
    let at_start = read(&db, own_path.as_deref(), app_path.as_deref());
    let stop = CancelToken::new();
    let ticker = stop.clone();
    let thread = std::thread::Builder::new()
        .name("bertbrowser self checks".to_string())
        .spawn(move || {
            while !ticker.is_cancelled() {
                let mut slept = Duration::ZERO;
                while slept < INTERVAL && !ticker.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(250));
                    slept += Duration::from_millis(250);
                }
                if ticker.is_cancelled() {
                    return;
                }
                let now = read(&db, own_path.as_deref(), app_path.as_deref());
                if should_stop(at_start, now) {
                    lifetime.cancel();
                    return;
                }
            }
        })
        .ok();
    SelfChecks { stop, thread }
}

fn read(db: &Db, own_path: Option<&Path>, app_path: Option<&Path>) -> Reading {
    Reading {
        schema: db.schema_version().ok(),
        own_stamp: own_path.map(last_write_of),
        app_exists: app_path.is_some_and(Path::is_file),
    }
}

/// The write time, or None when the file is gone or cannot be read just now.
fn last_write_of(path: &Path) -> Option<FileTime> {
    use std::os::windows::fs::MetadataExt;
    std::fs::metadata(path)
        .ok()
        .map(|m| FileTime(m.last_write_time() as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(schema: i64, stamp: i64) -> Reading {
        Reading {
            schema: Some(schema),
            own_stamp: Some(Some(FileTime(stamp))),
            app_exists: true,
        }
    }

    #[test]
    fn nothing_changed_keeps_running() {
        assert!(!should_stop(reading(5, 100), reading(5, 100)));
    }

    #[test]
    fn a_migrated_database_stops_it() {
        assert!(should_stop(reading(5, 100), reading(6, 100)));
    }

    #[test]
    fn a_replaced_or_removed_executable_stops_it() {
        assert!(should_stop(reading(5, 100), reading(5, 101)));
        let mut gone = reading(5, 100);
        gone.own_stamp = Some(None);
        assert!(should_stop(reading(5, 100), gone));
    }

    #[test]
    fn a_missing_app_stops_it() {
        let mut no_app = reading(5, 100);
        no_app.app_exists = false;
        assert!(should_stop(reading(5, 100), no_app));
    }

    /// A busy or locked database is not a reason to stop indexing.
    #[test]
    fn an_unreadable_reading_does_not_count() {
        let mut busy = reading(5, 100);
        busy.schema = None;
        assert!(!should_stop(reading(5, 100), busy));
        let mut unknown_exe = reading(5, 100);
        unknown_exe.own_stamp = None;
        assert!(!should_stop(reading(5, 100), unknown_exe));
    }
}
