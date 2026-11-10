//! The menu bar and the toolbars as data: an arrangement (`CommandBarSet`) in which an item is only
//! a command id, the one the app ships with, what a stored one may contain, what a menu shows this
//! instant, the editor's operations and the saved arrangements. `None` for the arrangement means
//! the default, and one equal to the default is written back as `None`, so an uncustomised user
//! gets a later build's additions while a customised one is never edited behind their back.
//! Spec: Core/Services/CommandBars/*.

mod access_keys;
mod captions;
mod defaults;
mod edit;
mod lists;
mod models;
mod rules;
mod saved;

pub use access_keys::AccessKeys;
pub use captions::CommandBarCaptions;
pub use defaults::CommandBarDefaults;
pub use edit::{BarTarget, BarTargetKind, CommandBarEdit};
pub use lists::{CommandBarList, CommandBarLists};
pub use models::{BarItem, BarItemKind, CommandBarSet, SavedCommandBarSet, ToolbarSpec};
pub use rules::{BarCommandState, BarRow, BarRowKind, BarSource, CommandBarRules};
pub use saved::SavedCommandBarRules;
