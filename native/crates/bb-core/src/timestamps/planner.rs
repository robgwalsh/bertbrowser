//! Works out the dates each selected item should end up with. Pure: every question about disk goes
//! through `TimestampProbe`. Spec: Core/Services/Timestamps/TimestampPlanner.cs.

use std::collections::HashSet;
use std::os::windows::fs::MetadataExt;

use bb_paths::{file_name, fold_upper};

use super::{
    PlannedTimestamp, RejectedTimestamp, TimestampChange, TimestampEntry, TimestampPlan,
    TimestampRejection, TimestampSource,
};
use crate::FileTime;
use crate::archives::ArchivePath;
use crate::attributes::{DIRECTORY, REPARSE_POINT};
use crate::rename::paths::directory_name;
use crate::time::TICKS_PER_DAY;

/// What the planner needs to know about disk, so it can be tested without one.
pub trait TimestampProbe {
    fn find(&self, path: &str) -> Option<TimestampEntry>;

    /// The picture's own date taken as local wall-clock ticks, or None when it has none.
    fn date_taken_of(&self, path: &str) -> Option<FileTime>;
}

/// The real one, over the local disk.
pub struct FileSystemTimestampProbe;

impl TimestampProbe for FileSystemTimestampProbe {
    fn find(&self, path: &str) -> Option<TimestampEntry> {
        // The entry's own attributes and dates, as `FileSystemInfo` read them from find data.
        let meta = std::fs::symlink_metadata(path).ok()?;
        Some(TimestampEntry {
            attributes: meta.file_attributes(),
            modified_utc: FileTime(meta.last_write_time() as i64),
            created_utc: FileTime(meta.creation_time() as i64),
        })
    }

    fn date_taken_of(&self, _path: &str) -> Option<FileTime> {
        // TODO(port): extended when Metadata is ported — reads the picture's `DateTaken` through
        // the codec for its type (`MetadataCodecs::for_path(path)?.read(ReadOnlyFile::try_open(…))`)
        // and parses it with `MetadataFields::try_parse_date`. Until then no picture has one.
        None
    }
}

pub static LOCAL_PROBE: FileSystemTimestampProbe = FileSystemTimestampProbe;

/// The earliest date NTFS can store, with a day's margin for time zones: 1601-01-02 UTC.
const EARLIEST: FileTime = FileTime(TICKS_PER_DAY);

pub struct TimestampPlanner<'a> {
    probe: &'a dyn TimestampProbe,
}

impl<'a> TimestampPlanner<'a> {
    pub fn new(probe: &'a dyn TimestampProbe) -> TimestampPlanner<'a> {
        TimestampPlanner { probe }
    }

    /// A planner over the real disk.
    pub fn local() -> TimestampPlanner<'static> {
        TimestampPlanner {
            probe: &LOCAL_PROBE,
        }
    }

    pub fn plan(&self, paths: &[String], change: &TimestampChange) -> TimestampPlan {
        if !change.modified && !change.created {
            return TimestampPlan::empty();
        }

        let mut items = Vec::new();
        let mut rejected = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for path in paths {
            if !seen.insert(fold_upper(path)) {
                continue;
            }
            let leaf = file_name(path);
            let name = if leaf.is_empty() { path.as_str() } else { leaf };
            let mut no = |reason: TimestampRejection, message: String| {
                rejected.push(RejectedTimestamp {
                    path: path.clone(),
                    reason,
                    message,
                });
            };

            if is_inside_archive(path) {
                no(
                    TimestampRejection::InsideArchive,
                    format!("{name} is inside an archive."),
                );
                continue;
            }

            let Some(entry) = self.probe.find(path) else {
                no(
                    TimestampRejection::Missing,
                    format!("{name} is no longer there."),
                );
                continue;
            };

            if entry.attributes & REPARSE_POINT != 0 {
                no(
                    TimestampRejection::Link,
                    format!("{name} is a link, and its date is its target's."),
                );
                continue;
            }

            let (modified, created): (Option<FileTime>, Option<FileTime>) = match change.source {
                TimestampSource::Shift => {
                    // A date past either end of what a date can hold is no date at all.
                    let offset = change.offset.unwrap_or(0);
                    match (
                        entry.modified_utc.0.checked_add(offset),
                        entry.created_utc.0.checked_add(offset),
                    ) {
                        (Some(m), Some(c)) => (Some(FileTime(m)), Some(FileTime(c))),
                        _ => {
                            no(
                                TimestampRejection::OutOfRange,
                                format!("That is not a date {name} can be given."),
                            );
                            continue;
                        }
                    }
                }
                TimestampSource::DateTaken => {
                    let Some(taken) = self.probe.date_taken_of(path) else {
                        no(
                            TimestampRejection::NoDateTaken,
                            format!("{name} has no date taken."),
                        );
                        continue;
                    };
                    let utc = taken.local_to_utc();
                    (Some(utc), Some(utc))
                }
                TimestampSource::Value => {
                    let Some(value) = change.value else {
                        return TimestampPlan::empty();
                    };
                    let utc = value.local_to_utc();
                    (Some(utc), Some(utc))
                }
            };

            let modified = modified.filter(|_| change.modified);
            let created = created.filter(|_| change.created);

            if modified.is_some_and(|m| m < EARLIEST) || created.is_some_and(|c| c < EARLIEST) {
                no(
                    TimestampRejection::OutOfRange,
                    format!("That is not a date {name} can be given."),
                );
                continue;
            }

            if modified.is_none_or(|m| m == entry.modified_utc)
                && created.is_none_or(|c| c == entry.created_utc)
            {
                no(
                    TimestampRejection::Unchanged,
                    format!("{name} already has that date."),
                );
                continue;
            }

            items.push(PlannedTimestamp {
                path: path.clone(),
                is_directory: entry.attributes & DIRECTORY != 0,
                modified_utc: modified,
                created_utc: created,
            });
        }

        TimestampPlan { items, rejected }
    }
}

/// `MetadataEditPlanner.IsInsideArchive`: asks a path's **folder**, never its own name, because
/// an EPUB is an archive and standing on one is not being in it.
/// TODO(port): extended when Metadata is ported — belongs on its planner.
pub fn is_inside_archive(path: &str) -> bool {
    directory_name(path).is_some_and(|dir| ArchivePath::looks_virtual(&dir))
}
