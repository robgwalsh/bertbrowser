//! Types the settings file embeds that belong to areas not yet ported, plus re-exports of the
//! ones that have landed since. Each
//! is the minimal shape with the C# field names, so the orchestrator swaps a `use` line and nothing
//! else. Behaviour that lives on these types (defaults, the one or two rules the settings need) is
//! carried over verbatim.

pub use crate::checksums::ChecksumAlgorithm;
pub use crate::commandbars::{BarItem, CommandBarSet, SavedCommandBarSet, ToolbarSpec};
pub use crate::custom_commands::CustomCommandDefinition;

/// A built-in id (`dark-plus`, `light-plus`, …) or the file stem of one of the user's own themes.
/// The rules about one (`is_safe`, `unique`) live in `crate::theming::ThemeId`; the value is text.
pub type ThemeId = String;

pub use crate::newitem::NewFileTemplate;
pub use crate::rename::{RenameCase, RenameRule, RenameScope};
pub use crate::undo::UndoBudget;
