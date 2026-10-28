//! What crosses the elevated-operation pipe, and what comes back from an elevated attempt.
//! Spec: Core/Services/Elevation/ElevationModels.cs, Elevated.cs.

use crate::delete::DeleteMode;
use crate::transfer::{CompletedTransfer, ConflictResolution, PlannedTransfer};

/// An operation's result after the elevated retry has had its turn: the merged outcome, and a note
/// for the status line when the retry was declined or could not be offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Elevated<T> {
    pub outcome: T,
    pub note: String,
}

impl<T> Elevated<T> {
    pub fn new(outcome: T, note: &str) -> Elevated<T> {
        Elevated {
            outcome,
            note: note.to_string(),
        }
    }

    /// The outcome as it was, with nothing to say.
    pub fn plain(outcome: T) -> Elevated<T> {
        Elevated::new(outcome, "")
    }
}

/// Which of the four mutations, or which undo of one, the helper is being asked for.
///
/// There is deliberately no `RenameUndo`: a rename is its own inverse, so
/// `RenameExecutor::undo_plan` is computed app-side and sent as an ordinary `Rename`. Archives are
/// absent too, and that is a decision rather than an omission: rewriting a container means handing
/// a third-party decoder attacker-controlled bytes, which is not something to do with a token in
/// hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElevationOperation {
    TransferMove,
    TransferCopy,
    Delete,
    Rename,
    NewItem,
    TransferUndo,
    DeleteUndo,
}

/// The `Begin` line: everything about a request that is not one of its items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevationHeader {
    pub operation: ElevationOperation,
    pub destination_directory: String,
    pub delete_mode: DeleteMode,
    pub permanent: bool,
}

impl ElevationHeader {
    pub fn new(operation: ElevationOperation) -> ElevationHeader {
        ElevationHeader {
            operation,
            destination_directory: String::new(),
            delete_mode: DeleteMode::Recycle,
            permanent: false,
        }
    }

    pub fn with_destination(mut self, destination_directory: &str) -> ElevationHeader {
        self.destination_directory = destination_directory.to_string();
        self
    }

    pub fn with_delete_mode(mut self, mode: DeleteMode) -> ElevationHeader {
        self.delete_mode = mode;
        self
    }

    pub fn with_permanent(mut self, permanent: bool) -> ElevationHeader {
        self.permanent = permanent;
        self
    }
}

/// One item of a transfer request, carrying its conflict resolution rather than leaving the
/// resolutions in a dictionary on the header.
///
/// Two reasons, and the first is the load-bearing one. A dictionary on the header would put the
/// whole selection on one line, which is exactly the bound `ElevationProtocol` refuses to raise.
/// And the resolution has to travel at all, or a `Replace` the user chose silently becomes the
/// default `KeepBoth` on the second pass and the operation quietly changes meaning half way through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevationTransferItem {
    pub item: PlannedTransfer,
    pub resolution: ConflictResolution,
}

/// The `Done` line for a transfer. A skipped item produces neither a completion nor a failure, so
/// it needs somewhere of its own to be reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevationTransferResult {
    pub completed: Option<CompletedTransfer>,
    pub skipped: Option<String>,
}

/// The `Done` line for a creation: the one operation whose success is a bare path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevationNewItemResult {
    pub created_path: String,
}

/// A `Progress` line: the superset of what a transfer and a delete each report, so one shape covers
/// both.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ElevationProgressReport {
    pub done: usize,
    pub total: usize,
    pub current_name: String,
    pub bytes_done: i64,
    pub current_item_bytes: i64,
    pub current_item_total: i64,
}

/// The `End` line: what an outcome needs that is not per-item.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ElevationSummary {
    pub cancelled: bool,
    pub staging_directories: Vec<String>,
    pub restored: usize,
}

/// How an attempt to run something elevated ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElevationStatus {
    /// The helper ran and reported an outcome. That outcome may still contain failures; this only
    /// says the elevated pass happened.
    Completed,
    /// The user answered the UAC prompt with No. Their choice, not a failure.
    Declined,
    /// This account cannot elevate, so no prompt was raised. Asked before the dialog is shown,
    /// never after: a standard user offered a shield gets a credential prompt for somebody else's
    /// password, which is worse than never being offered one.
    NotAdministrator,
    /// The helper was missing, could not be started, or died mid-operation.
    Unavailable,
}

/// What came back from an elevated attempt. `detail` is phrased for the status bar; empty when
/// nothing needs saying.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevatedRun<T> {
    pub status: ElevationStatus,
    pub result: Option<T>,
    pub detail: String,
}

impl<T> ElevatedRun<T> {
    pub fn ran(result: T) -> ElevatedRun<T> {
        ElevatedRun {
            status: ElevationStatus::Completed,
            result: Some(result),
            detail: String::new(),
        }
    }

    pub fn declined() -> ElevatedRun<T> {
        ElevatedRun {
            status: ElevationStatus::Declined,
            result: None,
            detail: "permission was declined".to_string(),
        }
    }

    pub fn not_administrator() -> ElevatedRun<T> {
        ElevatedRun {
            status: ElevationStatus::NotAdministrator,
            result: None,
            detail: "this account cannot provide administrator permission".to_string(),
        }
    }

    pub fn unavailable(detail: &str) -> ElevatedRun<T> {
        ElevatedRun {
            status: ElevationStatus::Unavailable,
            result: None,
            detail: detail.to_string(),
        }
    }
}
