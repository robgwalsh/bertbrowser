//! Orchestration: what ShellViewModel, DirectoryTabViewModel and FileListViewModel decided, minus the
//! painting. Panes and tabs over `bb_core::layout`, the listing and its sort bands, selection held
//! here, search and flat views, the transfer queue and undo recording, the watcher merge and the
//! settle predicate; and the services around them: the command registry and its one gate, the keymap
//! and the Keyboard page, the command palette, the menu bar and toolbars, columns (with the
//! shell-property seam the host fills), saved searches and workspaces, the session, new items,
//! settings with their debounce, themes and the system appearance, and the index banner. No async
//! runtime: a small worker pool, one transfer thread, events out through a sink. Every seam the
//! harness must fake is a trait chosen at construction (`AppConfig::fakes`).
//! Spec: src/BertBrowser.App/ViewModels/*, Services/Commands/*, Theming/*.

mod api;
mod app;
mod browse;
mod commands;
mod config;
mod events;
mod ids;
mod ops;
mod pool;
mod requests;
mod seams;
mod services;
mod session;
mod side_pane;
mod state;
mod tools;

pub use api::services::parse_aspect;
pub use api::tabs::TabInfo;
pub use app::{App, OpState};
pub use browse::columns::{PropSupply, date as prop_date, words as prop_words};
pub use browse::rows::{CELL_COLUMNS, Listing, Row, cell_index, stamp};
pub use browse::sort::SortSpec;
pub use browse::tabs::{Crumb, FLAT_ENTRY_CAP, SelectOp, breadcrumb, parent_path, tab_title};
pub use commands::{
    CommandOutcome, CommandOwner, CommandState, DIALOG_OPEN_REASON, SETTINGS_OPEN_REASON,
    is_handled as command_is_handled, owner_of as command_owner,
};
pub use config::{AppConfig, Fakes};
pub use events::{AppEvent, EventSink, Model};
pub use ids::{AppError, AppResult, OpId, PaneId, TabId};
pub use ops::launching::is_launch_command;
pub use requests::remaining_after_start;
pub use seams::{
    AppConflictPrompt, ElevatedRunner, ElevationLauncher, HarnessProbes, LaunchRequest,
    NoElevation, ProcessLauncher, PushedAppearance, Seams, SystemAppearanceSource,
    SystemProcessLauncher, UserConfirm, UserNotice,
};
pub use services::settings::{PAGE_KEYS, SETTINGS_PAGES};
pub use side_pane::gif_player::{GifOpenError, GifPlayer};
pub use side_pane::metadata_pane::{HostReading, HostValue, MetadataAction, ShellPropertyHost};
pub use side_pane::preview::{MODE_AUTO, MODE_GUESS, MODE_HEX, MODE_RAW};
