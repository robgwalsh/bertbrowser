//! Delete and the Recycle Bin: a pure planner deciding through probes, an executor that re-checks
//! live state and holds what it deletes until the undo record is retired, a surveyor for the
//! confirmation's numbers, and the Windows Recycle Bin over `IFileOperation`.
//! Spec: Core/Services/Delete/*.cs.

mod executor;
mod models;
mod planner;
mod probe;
mod protected;
mod recycle_bin;
mod redo;
mod surveyor;

pub use executor::DeleteExecutor;
pub use models::{
    DeleteDisposition, DeleteMeasurement, DeleteMode, DeleteOutcome, DeletePlan, DeleteProgress,
    DeleteRejection, DeleteSource, DeleteSurvey, DeleteUndoResult, DeletedItem, FailedDelete,
    PlannedDelete, RejectedDelete,
};
pub use planner::DeletePlanner;
pub use probe::{DeleteProbe, FileSystemDeleteProbe, NoRecycleProbe, RecycleProbe};
pub use protected::ProtectedLocations;
pub use recycle_bin::{RecycleBin, RecycleResult, RecycledItem, ShellRecycleBin};
pub use redo::DeleteRedo;
pub use surveyor::DeleteSurveyor;
