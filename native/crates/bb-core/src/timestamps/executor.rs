//! Sets the modified and created dates of files and folders, and puts them back.
//! Spec: Core/Services/Timestamps/TimestampExecutor.cs.
//!
//! Nothing is staged: a date is two numbers, and the outcome carries the old ones. Undo and redo
//! are one operation, `revert`, which sets an entry back to its *before* and returns it with the
//! two swapped. One item's failure never affects the others, and a revert leaves alone anything
//! whose dates have moved since — something else has written to it, and its new date is true.

use std::fs::{FileTimes, OpenOptions};
use std::io;
use std::os::windows::fs::{FileTimesExt, OpenOptionsExt};

use bb_paths::file_name;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
};

use super::planner::LOCAL_PROBE;
use super::{FailedTimestamp, StampedEntry, TimestampOutcome, TimestampPlan, TimestampProbe};
use crate::FileTime;
use crate::attributes::{DIRECTORY, REPARSE_POINT};
use crate::fs::AccessDenied;

pub struct TimestampExecutor<'a> {
    probe: &'a dyn TimestampProbe,
}

impl<'a> TimestampExecutor<'a> {
    pub fn new(probe: &'a dyn TimestampProbe) -> TimestampExecutor<'a> {
        TimestampExecutor { probe }
    }

    /// An executor over the real disk.
    pub fn local() -> TimestampExecutor<'static> {
        TimestampExecutor {
            probe: &LOCAL_PROBE,
        }
    }

    pub fn execute(&self, plan: &TimestampPlan) -> TimestampOutcome {
        if !plan.has_work() {
            return TimestampOutcome::empty();
        }

        let mut stamped = Vec::new();
        let mut failed = Vec::new();

        for item in &plan.items {
            let name = file_name(&item.path);

            // The planner saw an entry here; disk is the authority on whether it still is one.
            let Some(before) = self.probe.find(&item.path) else {
                failed.push(FailedTimestamp::new(
                    &item.path,
                    format!("{name} is no longer there."),
                ));
                continue;
            };

            if before.attributes & REPARSE_POINT != 0 {
                failed.push(FailedTimestamp::new(
                    &item.path,
                    format!("{name} is a link, and its date is its target's."),
                ));
                continue;
            }

            let directory = before.attributes & DIRECTORY != 0;
            match set(&item.path, item.modified_utc, item.created_utc) {
                Ok(()) => {
                    let after = self.probe.find(&item.path).unwrap_or(before);
                    stamped.push(StampedEntry {
                        path: item.path.clone(),
                        is_directory: directory,
                        before_modified_utc: before.modified_utc,
                        before_created_utc: before.created_utc,
                        after_modified_utc: after.modified_utc,
                        after_created_utc: after.created_utc,
                    });
                }
                Err(e) => failed.push(FailedTimestamp {
                    path: item.path.clone(),
                    message: format!("The date of {name} could not be changed: {e}"),
                    access_denied: AccessDenied::caused(Some(&e)),
                }),
            }
        }

        TimestampOutcome { stamped, failed }
    }

    /// Sets each entry back to the dates it had, and returns the ones that moved with their
    /// before and after exchanged — so reverting the result is the redo.
    pub fn revert(&self, stamped: &[StampedEntry]) -> TimestampOutcome {
        let mut reverted = Vec::new();
        let mut failed = Vec::new();

        for entry in stamped {
            let name = file_name(&entry.path);

            let Some(now) = self.probe.find(&entry.path) else {
                failed.push(FailedTimestamp::new(
                    &entry.path,
                    format!("{name} is no longer there."),
                ));
                continue;
            };

            if now.modified_utc != entry.after_modified_utc
                || now.created_utc != entry.after_created_utc
            {
                failed.push(FailedTimestamp::new(
                    &entry.path,
                    format!("{name} has changed since, so its date was left as it is."),
                ));
                continue;
            }

            match set(
                &entry.path,
                Some(entry.before_modified_utc),
                Some(entry.before_created_utc),
            ) {
                Ok(()) => {
                    let after = self.probe.find(&entry.path).unwrap_or(now);
                    reverted.push(StampedEntry {
                        path: entry.path.clone(),
                        is_directory: entry.is_directory,
                        before_modified_utc: now.modified_utc,
                        before_created_utc: now.created_utc,
                        after_modified_utc: after.modified_utc,
                        after_created_utc: after.created_utc,
                    });
                }
                Err(e) => failed.push(FailedTimestamp {
                    path: entry.path.clone(),
                    message: format!("The date of {name} could not be put back: {e}"),
                    access_denied: AccessDenied::caused(Some(&e)),
                }),
            }
        }

        TimestampOutcome {
            stamped: reverted,
            failed,
        }
    }
}

/// `SetFileTime` on a handle opened for attribute writes only, with backup semantics so a folder
/// opens too and the reparse-point flag so the entry itself is what is stamped (the executor has
/// already refused a link). Created first: on some file systems setting it nudges the last-write
/// time.
fn set(
    path: &str,
    modified_utc: Option<FileTime>,
    created_utc: Option<FileTime>,
) -> io::Result<()> {
    let file = OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if let Some(created) = created_utc {
        file.set_times(FileTimes::new().set_created(created.to_system_time()))?;
    }
    if let Some(modified) = modified_utc {
        file.set_times(FileTimes::new().set_modified(modified.to_system_time()))?;
    }
    Ok(())
}
