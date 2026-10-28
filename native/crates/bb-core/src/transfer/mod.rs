//! Moving and copying files: the planner that decides what a drop would do, the merge expander
//! that opens a folder landing on a folder of the same name, the executor that carries the plan
//! out against live disk state and records enough to undo it, and the small rules around them
//! (the pause gate, progress coalescing, the queue, the rate, the estimate, the right-drag menu and
//! the two drag-and-drop contracts, and the redo planner the undo history runs from).
//! Spec: Core/Services/Transfer/*.cs, Core/Interop/CopyNative.cs.

mod conflict_prompt;
mod copier;
mod drag_out;
mod drop_in;
mod estimator;
mod executor;
mod merge_expander;
mod models;
mod pause_gate;
mod planner;
mod probe;
mod progress;
mod queue_rules;
mod rate;
mod redo;
mod right_drop_menu;

pub use conflict_prompt::{ConflictPrompt, KeepBothConflictPrompt};
pub use copier::{FileCopier, FileSystemFileCopier, ProgressFn};
pub use drag_out::{DragOutAction, DragOutContract, DropEffect};
pub use drop_in::{DropInContract, DropInDecision, DropOrigin};
pub use estimator::{DirectorySize, TransferEstimate, TransferEstimator, TransferSizeSource};
pub use executor::{TransferExecutor, TransferUndoResult};
pub use merge_expander::{TransferMergeExpander, TransferMergeLimits};
pub use models::{
    ClashSide, CompletedTransfer, ConflictDefaults, ConflictResolution, FailedTransfer,
    PlannedTransfer, RejectedTransfer, Resolutions, TransferClash, TransferError, TransferOutcome,
    TransferPlan, TransferProgress, TransferRejection, TransferVerb, io_message,
};
pub use pause_gate::PauseGate;
pub use planner::TransferPlanner;
pub use probe::{FileSystemTransferProbe, TransferEntry, TransferEntrySource, TransferProbe};
pub use progress::ProgressCoalescer;
pub use queue_rules::{QueuedJobState, QueuedJobSummary, TransferQueueRules};
pub use rate::TransferRate;
pub use redo::TransferRedo;
pub use right_drop_menu::{RightDropContext, RightDropEntry, RightDropMenuRules, RightDropVerb};
