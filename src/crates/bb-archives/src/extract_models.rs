//! What an extract would do, and what one did. Spec: Core/Services/Archives/ExtractModels.cs.
//!
//! **There is deliberately no `Replace`.** With only Skip and KeepBoth an extract is *purely
//! additive*, which buys the same things it buys Copy: no undo to keep, no staging, nothing
//! deleted to make room. Overwriting the destination is the single most common way people lose
//! work to an unzip, and a file browser should not make that one keystroke away.

/// What to do about a name the destination already holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtractConflict {
    /// Leave what is there and skip the entry.
    Skip,
    /// Write beside it as "name (2)", through `UniquePath`.
    KeepBoth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtractRejection {
    /// The container is gone, damaged, or not what its name claims.
    ArchiveUnreadable,
    /// The container needs a password nobody has given.
    PasswordRequired,
    /// The destination is missing and could not be created.
    DestinationMissing,
    /// The destination names a file rather than a folder.
    DestinationNotDirectory,
    /// Extracting here would write into the container's own path.
    DestinationInsideArchive,
    /// Nothing was selected, or nothing selected is extractable.
    NothingToExtract,
}

/// One entry, and where on disk it is going.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedExtraction {
    pub entry_path: String,
    pub destination_path: String,
    pub size_bytes: i64,
    pub is_directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedExtraction {
    pub reason: ExtractRejection,
    pub message: String,
}

/// What an extract would do. Built while a dialog is open, re-checked before it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractPlan {
    pub archive_file: String,
    pub destination_directory: String,
    pub items: Vec<PlannedExtraction>,
    pub conflicts: Vec<String>,
    pub rejected: Option<RejectedExtraction>,
    pub total_bytes: i64,
    pub bytes_are_exact: bool,
}

impl ExtractPlan {
    pub fn has_work(&self) -> bool {
        self.rejected.is_none() && !self.items.is_empty()
    }

    pub fn refused(reason: ExtractRejection, message: &str) -> ExtractPlan {
        ExtractPlan {
            archive_file: String::new(),
            destination_directory: String::new(),
            items: Vec::new(),
            conflicts: Vec::new(),
            rejected: Some(RejectedExtraction {
                reason,
                message: message.to_string(),
            }),
            total_bytes: 0,
            bytes_are_exact: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedExtraction {
    pub entry_path: String,
    pub message: String,
}

/// Extracting is additive, so there is nothing to undo, the same contract Copy and New have. The
/// absence of a `can_undo` is the design, not an omission: Delete removes what this made,
/// reversibly.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ExtractOutcome {
    pub files_written: i32,
    pub bytes_written: i64,
    pub failed: Vec<FailedExtraction>,
    pub cancelled: bool,
}

impl ExtractOutcome {
    pub fn nothing() -> ExtractOutcome {
        ExtractOutcome::default()
    }
}
