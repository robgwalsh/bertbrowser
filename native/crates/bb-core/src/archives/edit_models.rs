//! What editing a container would amount to, and what one edit did. Spec: Core/Services/Archives/
//! ArchiveEditModels.cs.

use std::collections::{HashMap, HashSet};

use crate::fs::EntryStamp;

/// One change to make to a container's contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveEdit {
    /// Remove an entry, and everything under it if it is a folder.
    Remove { entry_path: String },
    /// Give an entry a new name in the same folder.
    Rename {
        entry_path: String,
        new_name: String,
    },
    /// Put a file from disk into the container at `entry_path`.
    AddFile(AddFile),
}

impl ArchiveEdit {
    pub fn remove(entry_path: &str) -> ArchiveEdit {
        ArchiveEdit::Remove {
            entry_path: entry_path.to_string(),
        }
    }

    pub fn rename(entry_path: &str, new_name: &str) -> ArchiveEdit {
        ArchiveEdit::Rename {
            entry_path: entry_path.to_string(),
            new_name: new_name.to_string(),
        }
    }

    pub fn add_file(source_path: &str, entry_path: &str) -> ArchiveEdit {
        ArchiveEdit::AddFile(AddFile {
            source_path: source_path.to_string(),
            entry_path: entry_path.to_string(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddFile {
    pub source_path: String,
    pub entry_path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveEditRejection {
    /// The container is gone, damaged, or not what its name claims.
    Unreadable,
    /// Nothing here can write this format.
    FormatNotWritable,
    /// Solid: rewriting would have to recompress every block, and cannot preserve it.
    Solid,
    /// Encrypted: a rewrite would silently drop the encryption.
    Encrypted,
    /// Truncated or missing volumes: a rewrite would make the loss permanent.
    Incomplete,
    /// Larger than this app will rewrite in one go.
    TooLarge,
    /// An entry named by the edit is not in the container.
    EntryMissing,
    /// Two entries would end up with the same path.
    NameTaken,
    /// The new name is not one Windows would accept.
    InvalidName,
    /// Nothing to do.
    NothingToDo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedArchiveEdit {
    pub reason: ArchiveEditRejection,
    pub message: String,
}

/// What editing a container would amount to, and what it will cost. `rewrite_bytes` is the whole
/// archive, not the change: nothing can modify one of these formats in place, so deleting one
/// entry from a 4 GB zip reads and writes 4 GB. The dialog says so before anyone agrees to it.
/// `renames` and `removals` are keyed by upper-cased entry path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEditPlan {
    pub archive_file: String,
    pub edits: Vec<ArchiveEdit>,
    pub renames: HashMap<String, String>,
    pub removals: HashSet<String>,
    pub additions: Vec<AddFile>,
    pub rewrite_bytes: i64,
    pub rejected: Option<RejectedArchiveEdit>,
}

impl ArchiveEditPlan {
    pub fn has_work(&self) -> bool {
        self.rejected.is_none() && !self.edits.is_empty()
    }

    pub fn refused(reason: ArchiveEditRejection, message: &str) -> ArchiveEditPlan {
        ArchiveEditPlan {
            archive_file: String::new(),
            edits: Vec::new(),
            renames: HashMap::new(),
            removals: HashSet::new(),
            additions: Vec::new(),
            rewrite_bytes: 0,
            rejected: Some(RejectedArchiveEdit {
                reason,
                message: message.to_string(),
            }),
        }
    }
}

/// What an archive edit did, and what it left behind so it can be undone. `staged_original` is
/// where the container that was replaced is being held: erased by `commit_staging` exactly one
/// operation later, the same contract a Replace's staging has, so the data outlives the undo
/// record rather than the other way round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEditOutcome {
    pub archive_file: String,
    pub staged_original: Option<String>,
    pub entries_written: i32,
    pub failure: Option<String>,
    pub cancelled: bool,
    /// The original container as it was before the swap: what a redo checks is still in place
    /// before it takes the edit back in.
    pub original: Option<EntryStamp>,
    /// The edited container as the swap left it: what an undo checks is still in place before it
    /// sets it aside. A container changed by anything else since is refused, not lost.
    pub edited: Option<EntryStamp>,
}

impl ArchiveEditOutcome {
    pub fn new(
        archive_file: &str,
        staged_original: Option<String>,
        entries_written: i32,
        failure: Option<String>,
        cancelled: bool,
    ) -> ArchiveEditOutcome {
        ArchiveEditOutcome {
            archive_file: archive_file.to_string(),
            staged_original,
            entries_written,
            failure,
            cancelled,
            original: None,
            edited: None,
        }
    }

    pub fn can_undo(&self) -> bool {
        self.staged_original.is_some() && self.failure.is_none() && !self.cancelled
    }

    pub fn nothing(archive_file: &str) -> ArchiveEditOutcome {
        ArchiveEditOutcome::new(archive_file, None, 0, None, false)
    }
}

/// An archive edit that was undone: the original is back in place, and the edited container is
/// held beside it so the edit can be redone. `staged_edited` is None when the undo did not
/// happen, and `failure` says why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEditUndo {
    pub archive_file: String,
    pub staged_edited: Option<String>,
    pub original: Option<EntryStamp>,
    pub edited: Option<EntryStamp>,
    pub failure: Option<String>,
}

impl ArchiveEditUndo {
    pub fn can_redo(&self) -> bool {
        self.staged_edited.is_some() && self.failure.is_none()
    }
}
