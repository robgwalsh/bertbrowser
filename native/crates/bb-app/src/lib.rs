//! Orchestration: what ShellViewModel, DirectoryTabViewModel and FileListViewModel decided, minus the
//! painting. Panes and tabs over `bb_core::layout`, the listing and its sort bands, selection held
//! here, search and flat views, the transfer queue and undo recording, the watcher merge and the
//! settle predicate. No async runtime: a small worker pool, one transfer thread, events out through a
//! sink. Every seam the harness must fake is a trait chosen at construction (`AppConfig::fakes`).
//! Spec: src/BertBrowser.App/ViewModels/*.

mod api;
mod app;
mod commands;
mod config;
mod dirwatch;
mod events;
mod ids;
mod ops;
mod pool;
mod rows;
mod seams;
mod sort;
mod tabs;

pub use api::TabInfo;
pub use app::{App, OpState};
pub use commands::is_handled as command_is_handled;
pub use config::{AppConfig, Fakes};
pub use events::{AppEvent, EventSink};
pub use ids::{AppError, AppResult, OpId, PaneId, TabId};
pub use rows::{CELL_COLUMNS, Listing, Row, cell_index, stamp};
pub use seams::{
    AppConflictPrompt, ElevationLauncher, HarnessProbes, ProcessLauncher, Seams,
    SystemAppearanceSource, UserConfirm, UserNotice,
};
pub use sort::SortSpec;
pub use tabs::{Crumb, FLAT_ENTRY_CAP, SelectOp, breadcrumb, parent_path, tab_title};
