//! The settings model, where the data directory is, and the native app's first launch over a C#
//! data directory. Spec: App/Services/AppSettings.cs, App/Services/AppPaths.cs.

mod app_paths;
mod app_settings;
mod legacy;
mod placeholders;

pub use app_paths::{AppPaths, AppPathsError};
pub use app_settings::{
    AppSettings, DEFAULT_TILE_ASPECT_RATIO, DrivesOpenTarget, SectionPlacement, SettingsError,
};
pub use legacy::{DataDirEntry, LegacyData, LegacyFailure, LegacyMove, LegacyOutcome, LegacyPlan};
pub use placeholders::{
    BarItem, ChecksumAlgorithm, CommandBarSet, CustomCommandDefinition, NewFileTemplate,
    RenameCase, RenameRule, RenameScope, SavedCommandBarSet, ThemeId, ToolbarSpec, UndoBudget,
};
