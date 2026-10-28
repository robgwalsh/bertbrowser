//! The undo history: a session-only, LIFO history of moves, renames, deletes, merged pastes,
//! archive edits, syncs, metadata edits and date changes, with redo and "undo/redo to here".
//! Spec: Core/Services/UndoHistory/*.cs.
//!
//! Data is committed only when an entry's record is retired: when the budget (steps plus held
//! bytes) releases it, when a new operation discards the redo branch, on Clear, and on window
//! close. The budget never releases the next undo, however large. Records are immutable and every
//! step returns a new one holding only what actually moved, which is why a partial undo is never
//! replayed. A walk stops at the first step that is not clean. Redo re-plans from the record
//! (`TransferRedo` / `DeleteRedo`): same final names, displace only what was displaced before, Skip
//! everything else. Held bytes are file stats plus the size index by original path, falling back to
//! a capped walk of our own staged copy: an estimate for the budget only, never shown as a size.

mod archive_edit_record;
mod budget;
mod delete_record;
mod held_size;
mod host;
mod metadata_edit_record;
mod models;
mod navigator;
mod record;
mod rename_record;
mod stack;
mod sync_record;
mod text;
mod timestamp_record;
mod transfer_record;

pub use archive_edit_record::{
    ArchiveEditOutcome, ArchiveEditRecord, ArchiveEditUndo, ArchiveEdits, UnportedArchiveEdits,
};
pub use budget::UndoBudget;
pub use delete_record::DeleteRecord;
pub use held_size::{HeldFolderSizes, HeldSizeEstimator};
pub use host::{PlainUndoHost, ProgressSink, UndoHost};
pub use metadata_edit_record::{
    FailedMetadataEdit, HeldVersion, MetadataEditOutcome, MetadataEditRecord, MetadataEdits,
    MetadataSwapResult, UnportedMetadataEdits,
};
pub use models::{
    HeldEntry, HeldSize, StepReport, StepResult, StepVerdict, UndoDirection, UndoItemDetail,
    UndoKind, UndoRefresh,
};
pub use navigator::{NavigationResult, UndoNavigator};
pub use record::UndoableRecord;
pub use rename_record::RenameRecord;
pub use stack::{EntryId, Released, UndoEntry, UndoError, UndoStack};
pub use sync_record::SyncRecord;
pub use text::UndoText;
pub use timestamp_record::TimestampRecord;
pub use transfer_record::TransferRecord;
