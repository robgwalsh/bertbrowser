//! The shapes the undo history is made of: what kind of operation an entry is, how a step went,
//! what the app is holding for it, and what a step leaves stale on screen.
//! Spec: Core/Services/UndoHistory/UndoModels.cs.

use std::ops::Add;

use bb_paths::fold_upper;

use super::record::UndoableRecord;
use crate::delete::DeletedItem;
use crate::rename::CompletedRename;

/// The eight kinds of operation the history can take back and do again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UndoKind {
    Rename,
    Move,
    /// A merged paste: the one copy whose writes are worth reversing, because it may have replaced
    /// files. A plain copy only adds, and never enters the history.
    Paste,
    Delete,
    ArchiveEdit,
    Sync,
    /// Tags and details written into files: a picture's EXIF, a song's ID3.
    MetadataEdit,
    /// A file's or folder's modified and created dates.
    Timestamps,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UndoDirection {
    Undo,
    Redo,
}

/// How a step went, for the badge on its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StepVerdict {
    /// Everything in it moved.
    Clean,
    /// Some of it moved; what did not is reported, and drops out of the entry.
    Partial,
    /// Nothing moved, so the entry stayed where it was.
    Failed,
}

/// How much this app is holding on the user's behalf for one entry: files a Replace displaced,
/// items deleted into the holding folder, an archive's other version. `complete` is false when
/// part of it could not be measured, so `bytes` is a floor, counted as a floor against the budget
/// and never as zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeldSize {
    pub bytes: i64,
    pub complete: bool,
}

impl HeldSize {
    pub const NONE: HeldSize = HeldSize {
        bytes: 0,
        complete: true,
    };

    pub fn new(bytes: i64, complete: bool) -> HeldSize {
        HeldSize { bytes, complete }
    }
}

impl Default for HeldSize {
    fn default() -> Self {
        HeldSize::NONE
    }
}

impl Add for HeldSize {
    type Output = HeldSize;

    fn add(self, other: HeldSize) -> HeldSize {
        HeldSize {
            bytes: self.bytes + other.bytes,
            complete: self.complete && other.complete,
        }
    }
}

/// One thing an entry holds: where it is parked, where it came from, and whether it is a folder.
/// The original path is what the size index knows it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldEntry {
    pub held_path: String,
    pub original_path: String,
    pub is_directory: bool,
}

impl HeldEntry {
    pub fn new(held_path: &str, original_path: &str, is_directory: bool) -> HeldEntry {
        HeldEntry {
            held_path: held_path.to_string(),
            original_path: original_path.to_string(),
            is_directory,
        }
    }
}

/// What one step did: how many items moved, and why the rest did not. `note` is anything else
/// worth saying, such as an elevated retry that was declined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepReport {
    pub succeeded: usize,
    pub failures: Vec<String>,
    pub note: String,
}

impl StepReport {
    pub fn new(succeeded: usize, failures: Vec<String>) -> StepReport {
        StepReport {
            succeeded,
            failures,
            note: String::new(),
        }
    }

    pub fn with_note(succeeded: usize, failures: Vec<String>, note: &str) -> StepReport {
        StepReport {
            succeeded,
            failures,
            note: note.to_string(),
        }
    }

    pub fn clean(&self) -> bool {
        self.failures.is_empty()
    }

    pub fn nothing_happened(&self) -> bool {
        self.succeeded == 0
    }

    pub fn verdict(&self) -> StepVerdict {
        if self.nothing_happened() {
            StepVerdict::Failed
        } else if self.clean() {
            StepVerdict::Clean
        } else {
            StepVerdict::Partial
        }
    }
}

/// One row of an entry's detail: an item and where it went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoItemDetail {
    pub from: String,
    pub to: String,
    pub is_directory: bool,
}

impl UndoItemDetail {
    pub fn new(from: &str, to: &str, is_directory: bool) -> UndoItemDetail {
        UndoItemDetail {
            from: from.to_string(),
            to: to.to_string(),
            is_directory,
        }
    }
}

/// What a step leaves stale on screen, for the shell to reload. Core decides what changed; the
/// app decides how to show it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct UndoRefresh {
    /// Folders whose listings changed.
    pub directories: Vec<String>,
    /// Archives whose insides changed; a tab may be standing in one.
    pub containers: Vec<String>,
    /// Items that just went away, so a tab inside a folder among them can step out of it before
    /// it is reloaded onto a missing path.
    pub vacated: Vec<DeletedItem>,
    /// Renames that just happened, for the shell's own rename refresh.
    pub renamed: Vec<CompletedRename>,
    /// A folder comparison's verdicts no longer describe the disk.
    pub rescan_compare: bool,
}

impl UndoRefresh {
    /// The given folders, blanks dropped and duplicates folded case-insensitively, keeping the
    /// first spelling seen.
    pub fn of(directories: impl IntoIterator<Item = Option<String>>) -> UndoRefresh {
        let mut seen = std::collections::HashSet::new();
        let mut distinct = Vec::new();
        for directory in directories.into_iter().flatten() {
            if directory.is_empty() {
                continue;
            }
            if seen.insert(fold_upper(&directory)) {
                distinct.push(directory);
            }
        }
        UndoRefresh {
            directories: distinct,
            ..UndoRefresh::default()
        }
    }
}

/// A step's result: the record for the other side, what happened, and what to reload.
pub struct StepResult {
    pub next: Box<dyn UndoableRecord>,
    pub report: StepReport,
    pub refresh: UndoRefresh,
}

impl StepResult {
    pub fn new(
        next: Box<dyn UndoableRecord>,
        report: StepReport,
        refresh: UndoRefresh,
    ) -> StepResult {
        StepResult {
            next,
            report,
            refresh,
        }
    }
}
