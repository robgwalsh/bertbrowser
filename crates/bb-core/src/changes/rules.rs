//! The decisions behind the change timeline, kept pure so they are tested without a volume, a
//! database or a window. Spec: Core/Services/Changes/ChangeLogRules.cs.

use std::time::Duration;

use bb_paths::PathKey;

use super::{ChangeKind, ChangeLogPolicy, ChangeRange, ChangeTimelineAvailability};
use crate::FileTime;
use crate::archives::ArchivePath;
use crate::mft::usn_reason;

pub struct ChangeLogRules;

impl ChangeLogRules {
    /// Repeated writes to one file inside this window fold into one row with a count.
    pub const COALESCE_WINDOW: Duration = Duration::from_secs(60);
    /// The hard cap, whatever the retention: a runaway build must not fill the disk.
    pub const MAX_ROWS: i64 = 500_000;
    /// How often the writer prunes. Wall-clock, not per-batch.
    pub const PRUNE_INTERVAL: Duration = Duration::from_secs(60);
    /// The most rows a view asks for. Past this it says "showing the newest N".
    pub const QUERY_LIMIT: usize = 2_000;

    /// What a CLOSE record amounted to. The precedence is the indexer's own: a delete beats
    /// everything, then a rename, then a create. Without an old name a new name is a move-in from
    /// somewhere the map could not resolve, which the indexer treats as a fresh entry.
    pub fn classify(reason: u32, had_old_name: bool) -> ChangeKind {
        if reason & usn_reason::FILE_DELETE != 0 {
            return ChangeKind::Deleted;
        }
        if reason & usn_reason::RENAME_NEW_NAME != 0 {
            return if had_old_name {
                ChangeKind::Renamed
            } else {
                ChangeKind::Created
            };
        }
        if reason & usn_reason::FILE_CREATE != 0 {
            return ChangeKind::Created;
        }
        ChangeKind::Modified
    }

    /// True for the data directory and everything under it. Every write to the database is itself a
    /// filesystem change, so without this the log would record its own growth for ever.
    pub fn is_excluded(path_key: &str, excluded_root_key: &str) -> bool {
        path_key == excluded_root_key || PathKey::is_under_key(path_key, excluded_root_key)
    }

    /// The lower bound of a range, never earlier than the policy's retention: a row the writer has
    /// not yet pruned must not outlive the promise the settings page made. None when "since the
    /// mark" has no mark yet.
    pub fn since_utc_within(
        range: ChangeRange,
        now_utc: FileTime,
        mark_utc: Option<FileTime>,
        policy: ChangeLogPolicy,
    ) -> Option<FileTime> {
        let since = Self::since_utc(range, now_utc, mark_utc)?;
        if !policy.enabled {
            return Some(since);
        }
        let floor = now_utc - policy.retention;
        Some(if since < floor { floor } else { since })
    }

    /// The lower bound of a range, or None when "since the mark" has no mark yet.
    pub fn since_utc(
        range: ChangeRange,
        now_utc: FileTime,
        mark_utc: Option<FileTime>,
    ) -> Option<FileTime> {
        match range {
            ChangeRange::Last15Minutes => Some(now_utc.add_minutes(-15)),
            ChangeRange::LastHour => Some(now_utc.add_hours(-1)),
            ChangeRange::Last6Hours => Some(now_utc.add_hours(-6)),
            ChangeRange::Last24Hours => Some(now_utc.add_hours(-24)),
            ChangeRange::SinceMark => mark_utc,
        }
    }

    /// Which thing is missing, in the order the user can do something about it: turn recording on,
    /// then get the helper running, then wait for the build.
    pub fn availability(
        recording_on: bool,
        scoped: bool,
        any_indexed: bool,
        scope_indexed: bool,
        is_building: bool,
        indexer_running: bool,
    ) -> ChangeTimelineAvailability {
        if !recording_on {
            return ChangeTimelineAvailability::RecordingOff;
        }
        if !indexer_running {
            return ChangeTimelineAvailability::IndexerUnavailable;
        }
        let covered = if scoped { scope_indexed } else { any_indexed };
        if covered {
            return ChangeTimelineAvailability::Ready;
        }
        if is_building {
            return ChangeTimelineAvailability::Building;
        }
        if scoped {
            ChangeTimelineAvailability::ScopeNotIndexed
        } else {
            ChangeTimelineAvailability::IndexerUnavailable
        }
    }

    /// The folder a timeline opened on `path` should watch: the path itself, or for somewhere inside
    /// an archive the archive's own folder. None for no path, meaning every drive.
    pub fn scope_for(path: Option<&str>, is_archive_file: impl Fn(&str) -> bool) -> Option<String> {
        let path = path.filter(|p| !p.is_empty())?;
        match ArchivePath::parse(path, is_archive_file) {
            Some(inside) => Some(bb_paths::split_last(&inside.archive_file).0.to_string()),
            None => Some(path.to_string()),
        }
    }

    /// Why the list is empty, said with the range and scope in it so it reads as a fact rather than
    /// a failure.
    pub fn empty_message(range: ChangeRange, scoped: bool) -> String {
        format!(
            "Nothing changed {} {}.",
            if scoped { "here" } else { "on this PC" },
            Self::range_phrase(range)
        )
    }

    /// "in the last hour", "since the mark": the range as a phrase.
    pub fn range_phrase(range: ChangeRange) -> &'static str {
        match range {
            ChangeRange::Last15Minutes => "in the last 15 minutes",
            ChangeRange::LastHour => "in the last hour",
            ChangeRange::Last6Hours => "in the last 6 hours",
            ChangeRange::Last24Hours => "in the last 24 hours",
            ChangeRange::SinceMark => "since the mark",
        }
    }
}
