//! The change timeline ("What changed"): the kinds, the policy, the pure rules, the query shape and
//! the recorder that buffers a volume's USN tail into the log. Spec: Core/Services/Changes/*.

mod recorder;
mod rules;

use std::collections::HashSet;
use std::time::Duration;

pub use recorder::ChangeRecorder;
pub use rules::ChangeLogRules;

use crate::FileTime;

/// What one journal record amounted to. Stored as its integer in `fs_change.kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum ChangeKind {
    Created = 1,
    Modified = 2,
    Deleted = 3,
    Renamed = 4,
}

impl ChangeKind {
    pub const ALL: [ChangeKind; 4] = [
        ChangeKind::Created,
        ChangeKind::Modified,
        ChangeKind::Deleted,
        ChangeKind::Renamed,
    ];

    pub fn from_i64(v: i64) -> ChangeKind {
        match v {
            1 => ChangeKind::Created,
            3 => ChangeKind::Deleted,
            4 => ChangeKind::Renamed,
            _ => ChangeKind::Modified,
        }
    }

    pub fn all() -> HashSet<ChangeKind> {
        Self::ALL.into_iter().collect()
    }
}

/// One change the USN tail resolved, on its way into `fs_change`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeEvent {
    /// `PathKey::canonicalize` of the path.
    pub path_key: String,
    /// The path as cased on disk. Stored because a deleted file has no ancestors left to rebuild it.
    pub display_path: String,
    pub is_directory: bool,
    /// Effective: the entry's own Hidden bit OR'd down from every ancestor.
    pub hidden: bool,
    pub kind: ChangeKind,
    /// For a rename, where it was.
    pub old_display_path: Option<String>,
    /// The record's own timestamp, not the time it was written down.
    pub utc: FileTime,
}

/// Whether the USN tail writes a change log at all, and how long rows live. **The default is off.**
/// Retention is one of a short menu of hours, because the same value crosses the pipe to the
/// elevated helper as the argument of one verb, and the helper accepts nothing it cannot validate
/// exhaustively. `0` is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ChangeLogPolicy {
    pub enabled: bool,
    pub retention: Duration,
}

impl ChangeLogPolicy {
    /// The choices the settings page offers, in hours.
    pub const RETENTION_OPTIONS: [i32; 4] = [1, 6, 24, 168];
    pub const DEFAULT_RETENTION_HOURS: i32 = 24;
    pub const OFF: ChangeLogPolicy = ChangeLogPolicy {
        enabled: false,
        retention: Duration::ZERO,
    };

    /// True for 0 (off) and for each of the retention options; nothing else.
    pub fn is_acceptable_hours(hours: i32) -> bool {
        hours == 0 || Self::RETENTION_OPTIONS.contains(&hours)
    }

    /// The wire and settings form: 0 is off, otherwise one of the menu's hours.
    pub fn from_hours(hours: i32) -> Option<ChangeLogPolicy> {
        if !Self::is_acceptable_hours(hours) {
            return None;
        }
        Some(if hours == 0 {
            Self::OFF
        } else {
            ChangeLogPolicy {
                enabled: true,
                retention: Duration::from_secs(hours as u64 * 3600),
            }
        })
    }

    pub fn to_hours(self) -> i32 {
        if self.enabled {
            (self.retention.as_secs() / 3600) as i32
        } else {
            0
        }
    }
}

/// How far back the timeline looks. `SinceMark` is the installer case: press "Mark now", run the
/// thing, and see only what happened after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeRange {
    Last15Minutes,
    LastHour,
    Last6Hours,
    Last24Hours,
    SinceMark,
}

/// Why the timeline cannot show anything, when it cannot. A feature that is switched off must not
/// look like a quiet disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeTimelineAvailability {
    Ready,
    /// The user has not turned recording on.
    RecordingOff,
    /// The volume's initial index is still building; recording starts when it completes.
    Building,
    /// The index helper declined, died, or found nothing to index.
    IndexerUnavailable,
    /// The folder asked about sits on a drive the helper does not cover.
    ScopeNotIndexed,
}

/// What the timeline asks the log for.
#[derive(Clone, Debug)]
pub struct ChangeQuery {
    /// The lower bound; there is no upper one, the log ends now.
    pub since_utc: FileTime,
    /// A canonical directory key to stay under, or None for every volume.
    pub scope_path_key: Option<String>,
    /// Which kinds to include. All four means no filter.
    pub kinds: HashSet<ChangeKind>,
    pub include_hidden: bool,
    /// The most rows wanted; one more is read to learn whether there were more.
    pub limit: usize,
}

/// One row of `fs_change`, read back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRow {
    pub id: i64,
    pub path_key: String,
    pub display_path: String,
    pub old_display_path: Option<String>,
    pub is_directory: bool,
    pub hidden: bool,
    pub kind: ChangeKind,
    pub first_utc: FileTime,
    pub last_utc: FileTime,
    pub count: i32,
}

/// The cheapest thing that changes whenever the log does. The largest id alone is not enough: a
/// coalesced write bumps an existing row and adds none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ChangeLogStamp {
    pub max_id: i64,
    pub last_utc: Option<FileTime>,
}
