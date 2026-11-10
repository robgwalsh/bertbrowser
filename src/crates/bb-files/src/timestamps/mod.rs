//! Changing a file's or folder's own modified and created dates: the planner that works out what
//! each item should end up with (a value, each item's own date shifted, or a picture's date taken)
//! and the executor that sets them and puts them back. Spec: Core/Services/Timestamps/*.cs.

mod executor;
mod models;
mod planner;

pub use executor::TimestampExecutor;
pub use models::{
    FailedTimestamp, PlannedTimestamp, RejectedTimestamp, StampedEntry, TimestampChange,
    TimestampEntry, TimestampOutcome, TimestampPlan, TimestampRejection, TimestampSource,
};
pub use planner::{FileSystemTimestampProbe, TimestampPlanner, TimestampProbe};
