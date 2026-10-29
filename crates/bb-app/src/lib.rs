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
mod api_ext;
mod app;
mod bars;
mod columns;
mod commands;
mod compare_view;
mod config;
mod delete_confirm;
mod dirwatch;
mod events;
mod gif_player;
mod ids;
mod index;
mod keys;
mod launching;
mod metadata_pane;
mod misc;
mod newitem;
mod ops;
mod ops_api;
mod ops_arch;
mod ops_ui;
mod palette;
mod panes_api;
mod pool;
mod preview;
mod properties;
mod requests;
mod rows;
mod saved;
mod seams;
mod session;
mod settings_svc;
mod shell_link;
mod sort;
mod state;
mod tabs;
mod themes;
mod tools;
mod tools_changes;
mod tools_checksum;
mod tools_disk;
mod tools_duplicates;
mod tools_filecompare;
mod undo_host;

pub use api::TabInfo;
pub use api_ext::parse_aspect;
pub use app::{App, OpState};
pub use columns::{PropSupply, date as prop_date, words as prop_words};
pub use commands::{
    CommandOutcome, CommandOwner, CommandState, DIALOG_OPEN_REASON, SETTINGS_OPEN_REASON,
    is_handled as command_is_handled, owner_of as command_owner,
};
pub use config::{AppConfig, Fakes};
pub use events::{AppEvent, EventSink, Model};
pub use gif_player::{GifOpenError, GifPlayer};
pub use ids::{AppError, AppResult, OpId, PaneId, TabId};
pub use launching::is_launch_command;
pub use metadata_pane::{HostReading, HostValue, MetadataAction, ShellPropertyHost};
pub use preview::{MODE_AUTO, MODE_GUESS, MODE_HEX, MODE_RAW};
pub use requests::remaining_after_start;
pub use rows::{CELL_COLUMNS, Listing, Row, cell_index, stamp};
pub use seams::{
    AppConflictPrompt, ElevatedRunner, ElevationLauncher, HarnessProbes, LaunchRequest,
    NoElevation, ProcessLauncher, PushedAppearance, Seams, SystemAppearanceSource,
    SystemProcessLauncher, UserConfirm, UserNotice,
};
pub use settings_svc::{PAGE_KEYS, SETTINGS_PAGES};
pub use sort::SortSpec;
pub use tabs::{Crumb, FLAT_ENTRY_CAP, SelectOp, breadcrumb, parent_path, tab_title};
