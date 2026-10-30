//! "Create shortcuts here": the planner that names each `.lnk` the way Explorer does and reserves
//! the names as it goes, and the executor that re-checks live disk and writes them through a
//! `ShortcutWriter` — the seam that keeps shell COM out of this crate (the real writer is the
//! shell crate's). Spec: Core/Services/Shortcuts/*.cs.

mod executor;
mod models;
mod planner;

pub use executor::{ShortcutExecutor, ShortcutWriter};
pub use models::{
    FailedShortcut, RejectedShortcut, ShortcutCreation, ShortcutOutcome, ShortcutPlan,
};
pub use planner::{FileSystemShortcutProbe, ShortcutPlanner, ShortcutProbe};
