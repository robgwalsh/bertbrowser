//! Renaming: the naming rule the dialog previews (`RenamePattern` over a `RenameRule` and its
//! parsed `RenameTemplate`), the planner that decides what may land where, and the executor that
//! carries a plan out with nothing ever overwritten. Spec: Core/Services/Rename/*.cs.
//!
//! The shape every executor here shares: a pure planner deciding through a probe trait, an executor
//! that re-applies the plan's rules against live disk before writing, and one item's failure never
//! affecting the others.

mod date_format;
mod executor;
mod metadata_fields;
mod models;
pub mod paths;
mod pattern;
mod planner;
mod regex_replace;
mod rule;
mod template;

pub use date_format::{DateFormatError, format_date};
pub use executor::RenameExecutor;
pub use metadata_fields::{MetadataField, MetadataFieldKind, MetadataFieldSpec, MetadataFields};
pub use models::{
    CompletedRename, FailedRename, PlannedRename, RejectedRename, RenameOutcome, RenamePlan,
    RenameRejection, RenameSource,
};
pub use pattern::{RenamePattern, RenamedName};
pub use planner::{FileSystemRenameProbe, RenamePlanner, RenameProbe};
pub use rule::{RenameCase, RenameRule, RenameScope};
pub use template::{RenamePart, RenameSegment, RenameTemplate};
