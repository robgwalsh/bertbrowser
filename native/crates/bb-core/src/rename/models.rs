//! What a rename is asked about, what the planner answers, and what the executor reports.
//! Spec: Core/Services/Rename/RenameModels.cs.

use std::collections::HashMap;

use bb_paths::file_name;

use super::MetadataField;
use crate::FileTime;

/// One item the user asked to rename.
///
/// `modified` is when the item was last written, in **local** wall-clock ticks, or None when that
/// is not known: what `{modified}` puts in a name. The date rides on the source rather than being
/// read from disk by the naming rule, which is what keeps `RenamePattern::apply` pure and its tests
/// free of the clock. Local rather than UTC because the file list's own Modified column is local,
/// and a date-stamped rename that disagreed with the column the user was reading would look like a
/// bug. None is a real answer: search results arrive without a timestamp until they are hydrated,
/// and a name is refused rather than stamped 0001-01-01.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameSource {
    /// Full path of the item as it stands now.
    pub path: String,
    /// True for a folder — a folder has no extension to preserve, so "My.Project" renamed in a
    /// batch must not have ".Project" treated as one.
    pub is_directory: bool,
    pub modified: Option<FileTime>,
    /// What the file's own metadata says — its artist, its date taken — for the tag tokens. None
    /// when nobody has read it, which is the usual case: reading costs a file open per item, so
    /// the dialog asks only once a template actually names a tag. Empty when it was read and held
    /// nothing. Rides on the source for the reason `modified` does.
    pub tags: Option<HashMap<MetadataField, String>>,
}

impl RenameSource {
    pub fn new(path: &str, is_directory: bool, modified: Option<FileTime>) -> RenameSource {
        RenameSource {
            path: path.to_string(),
            is_directory,
            modified,
            tags: None,
        }
    }

    pub fn file(path: &str) -> RenameSource {
        RenameSource::new(path, false, None)
    }

    pub fn dir(path: &str) -> RenameSource {
        RenameSource::new(path, true, None)
    }

    pub fn name(&self) -> &str {
        file_name(&self.path)
    }
}

/// Why the planner refused to rename one item. Every one of these is worth telling the user about
/// before anything is written — a rename that silently skips items is worse than one that refuses
/// to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameRejection {
    /// The item disappeared between selection and rename.
    SourceMissing,
    /// Drive and volume roots have no name to change.
    SourceIsRoot,
    /// The name the pattern produced is not a legal Windows file name.
    InvalidName,
    /// The name is already taken by something that isn't part of this batch, or two items in the
    /// batch would end up with the same name.
    NameTaken,
    /// A folder above this item is being renamed too, which moves this item's path out from
    /// under it. Reachable from a flattened search result, where a selection can hold both a
    /// folder and something inside it.
    InsideARenamedFolder,
    /// The rule could not produce a name — an expression that will not compile, a template with a
    /// token nobody recognises, a date format that is not one, or an item with no date for the
    /// `{modified}` the template asked for. When the rule is unusable outright this is nobody's
    /// fault in particular, so it carries an empty `source_path` and belongs in the dialog's
    /// banner rather than against a row; when one item alone fell over, it names that item.
    InvalidRule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedRename {
    pub source_path: String,
    pub reason: RenameRejection,
    /// User-facing explanation.
    pub message: String,
}

impl RejectedRename {
    pub fn new(source_path: &str, reason: RenameRejection, message: String) -> RejectedRename {
        RejectedRename {
            source_path: source_path.to_string(),
            reason,
            message,
        }
    }
}

/// One item the planner accepted, with the path it would end up at. Always in the same folder: a
/// rename never relocates anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedRename {
    pub source_path: String,
    pub target_path: String,
    pub is_directory: bool,
}

impl PlannedRename {
    pub fn new(source_path: &str, target_path: &str, is_directory: bool) -> PlannedRename {
        PlannedRename {
            source_path: source_path.to_string(),
            target_path: target_path.to_string(),
            is_directory,
        }
    }

    pub fn source_name(&self) -> &str {
        file_name(&self.source_path)
    }

    pub fn target_name(&self) -> &str {
        file_name(&self.target_path)
    }

    /// True when the pattern reproduced the name the item already has, down to its casing. Kept
    /// in the plan rather than dropped so a preview can show every selected item, but never
    /// executed.
    pub fn is_no_op(&self) -> bool {
        self.source_name() == self.target_name()
    }
}

/// The validated answer to "what would renaming these to this produce?".
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RenamePlan {
    pub renames: Vec<PlannedRename>,
    pub rejected: Vec<RejectedRename>,
}

impl RenamePlan {
    pub fn new(renames: Vec<PlannedRename>, rejected: Vec<RejectedRename>) -> RenamePlan {
        RenamePlan { renames, rejected }
    }

    pub fn empty() -> RenamePlan {
        RenamePlan::default()
    }

    /// True when carrying the plan out would change at least one name.
    pub fn has_work(&self) -> bool {
        self.renames.iter().any(|r| !r.is_no_op())
    }

    /// The renames that actually touch disk, in the order they were planned.
    pub fn work(&self) -> Vec<&PlannedRename> {
        self.renames.iter().filter(|r| !r.is_no_op()).collect()
    }
}

/// Where an item was, and where it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedRename {
    pub source_path: String,
    pub final_path: String,
    pub is_directory: bool,
}

/// An item that could not be renamed, phrased for the status bar.
///
/// **`stranded_path` is not decoration.** When one item's target is another's current name, the
/// executor moves it aside to `.bertbrowser-rename-<nonce>` first; if the rename then fails *and*
/// the restore fails too, the item is sitting at that staged path. Were that fact only in the
/// prose of `message`, a retry derived from `source_path` would rename from somewhere that no
/// longer holds anything and report success. So the fact is a field and the prose is built from
/// it, rather than the other way round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedRename {
    pub source_path: String,
    pub message: String,
    /// Windows refused permission. The only failure an administrator token could fix.
    pub access_denied: bool,
    /// Where the item actually is, when a failed rename left it parked under a staging name.
    pub stranded_path: Option<String>,
}

/// What actually happened on disk. `completed` doubles as the undo record: a rename is its own
/// inverse, so putting it back is the same operation with the paths swapped.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RenameOutcome {
    pub completed: Vec<CompletedRename>,
    pub failed: Vec<FailedRename>,
}

impl RenameOutcome {
    pub fn empty() -> RenameOutcome {
        RenameOutcome::default()
    }

    pub fn can_undo(&self) -> bool {
        !self.completed.is_empty()
    }
}
