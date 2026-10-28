//! What a date change is asked for, what the planner answers, and what the executor reports.
//! Spec: Core/Services/Timestamps/TimestampModels.cs.

use crate::FileTime;

/// Where the new date comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimestampSource {
    /// One date and time, the same for every item.
    Value,
    /// Each item's own date, moved by an offset.
    Shift,
    /// Each picture's own date taken.
    DateTaken,
}

/// What to do to a selection's dates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimestampChange {
    pub modified: bool,
    pub created: bool,
    pub source: TimestampSource,
    /// Local wall-clock ticks, for `Value`.
    pub value: Option<FileTime>,
    /// Ticks, for `Shift`.
    pub offset: Option<i64>,
}

impl TimestampChange {
    pub fn value(modified: bool, created: bool, local: FileTime) -> TimestampChange {
        TimestampChange {
            modified,
            created,
            source: TimestampSource::Value,
            value: Some(local),
            offset: None,
        }
    }

    pub fn shift(modified: bool, created: bool, offset_ticks: i64) -> TimestampChange {
        TimestampChange {
            modified,
            created,
            source: TimestampSource::Shift,
            value: None,
            offset: Some(offset_ticks),
        }
    }

    pub fn date_taken(modified: bool, created: bool) -> TimestampChange {
        TimestampChange {
            modified,
            created,
            source: TimestampSource::DateTaken,
            value: None,
            offset: None,
        }
    }
}

/// What the planner is told about one entry on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimestampEntry {
    /// The entry's own Win32 attribute mask.
    pub attributes: u32,
    pub modified_utc: FileTime,
    pub created_utc: FileTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimestampRejection {
    Missing,
    /// An entry inside an archive has no dates of its own on disk.
    InsideArchive,
    /// Setting a link's date would set its target's.
    Link,
    NoDateTaken,
    /// The result is not a date Windows can store.
    OutOfRange,
    /// It already has exactly these dates.
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedTimestamp {
    pub path: String,
    pub reason: TimestampRejection,
    pub message: String,
}

/// One item and the dates it should end up with. A None is left alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedTimestamp {
    pub path: String,
    pub is_directory: bool,
    pub modified_utc: Option<FileTime>,
    pub created_utc: Option<FileTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TimestampPlan {
    pub items: Vec<PlannedTimestamp>,
    pub rejected: Vec<RejectedTimestamp>,
}

impl TimestampPlan {
    pub fn empty() -> TimestampPlan {
        TimestampPlan::default()
    }

    pub fn has_work(&self) -> bool {
        !self.items.is_empty()
    }
}

/// An item whose dates were changed: what they were and what they are, which is everything an
/// undo needs — there is no data to hold, only four numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StampedEntry {
    pub path: String,
    pub is_directory: bool,
    pub before_modified_utc: FileTime,
    pub before_created_utc: FileTime,
    pub after_modified_utc: FileTime,
    pub after_created_utc: FileTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedTimestamp {
    pub path: String,
    pub message: String,
    pub access_denied: bool,
}

impl FailedTimestamp {
    pub fn new(path: &str, message: String) -> FailedTimestamp {
        FailedTimestamp {
            path: path.to_string(),
            message,
            access_denied: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TimestampOutcome {
    pub stamped: Vec<StampedEntry>,
    pub failed: Vec<FailedTimestamp>,
}

impl TimestampOutcome {
    pub fn empty() -> TimestampOutcome {
        TimestampOutcome::default()
    }

    pub fn can_undo(&self) -> bool {
        !self.stamped.is_empty()
    }
}
