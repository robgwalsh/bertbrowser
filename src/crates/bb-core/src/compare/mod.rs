//! Comparing two folders, settling pairs by content, and syncing one side onto the other. Spec:
//! src/BertBrowser.Core/Services/Compare/*.cs.
//!
//! A comparison's "same" is what authorises a delete, so every doubt resolves away from it: a
//! missing timestamp is `Unknown`, one `Unknown` descendant carries a whole subtree to `Unknown`,
//! and `dir_size_cache` never classifies a folder (equal totals do not mean equal trees). Content
//! settlement is one-directional, earned by reading both files, and rewrites the `CompareResult`
//! rather than overlaying it, because the sync planner reads verdicts straight off the result.

mod content_settlement;
mod equality;
mod file_compare_plan;
mod file_content_comparer;
mod folder_comparer;
mod models;
mod refusal;
mod rules;
mod service;
mod sync_models;
mod sync_planner;
mod sync_runner;

pub use content_settlement::ContentSettlement;
pub use equality::{CompareEquality, CompareTolerance};
pub use file_compare_plan::{FileCompareMode, FileComparePlan, FileCompareSide};
pub use file_content_comparer::{
    ContentComparer, ContentComparison, ContentVerdict, FileContentComparer,
};
pub use folder_comparer::{CompareResult, FolderComparer};
pub use models::{
    CompareEntry, CompareKeys, CompareRowState, CompareSide, CompareSourceKind, CompareVerdict,
};
pub use refusal::{CompareProbe, CompareRefusal, FileSystemCompareProbe};
pub use rules::CompareRules;
pub use service::{
    CompareAvailability, CompareLimits, CompareProgress, DriveFormat, FolderCompareOutcome,
    FolderCompareService,
};
pub use sync_models::{SyncAction, SyncActionKind, SyncOutcome, SyncPlans, SyncPreview};
pub use sync_planner::{DeletePlanning, SyncPlanner, TransferPlanning};
pub use sync_runner::{SyncRunner, SyncUndoResult};
