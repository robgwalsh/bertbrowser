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

// TODO(port): extended when UndoHistory is ported
/// How far back the undo history reaches: at most this many entries, and at most this much held
/// data. Spec: Core/Services/UndoHistory/UndoModels.cs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoBudget {
    pub max_entries: i32,
    pub max_held_bytes: i64,
}

impl UndoBudget {
    pub const DEFAULT_ENTRIES: i32 = 50;
    pub const DEFAULT_GIGABYTES: i32 = 10;

    /// The choices the Settings page offers.
    pub const ENTRY_OPTIONS: [i32; 5] = [10, 25, 50, 100, 200];
    pub const GIGABYTE_OPTIONS: [i32; 6] = [1, 5, 10, 25, 50, 100];

    /// A budget from the two settings, where None means "never set" and takes the default.
    /// Anything below one is lifted to one: a history that cannot hold a single entry is not a
    /// setting anybody asked for.
    pub fn from_settings(entries: Option<i32>, gigabytes: Option<i32>) -> UndoBudget {
        UndoBudget {
            max_entries: entries.unwrap_or(Self::DEFAULT_ENTRIES).max(1),
            max_held_bytes: i64::from(gigabytes.unwrap_or(Self::DEFAULT_GIGABYTES).max(1)) << 30,
        }
    }
}

impl Default for UndoBudget {
    fn default() -> Self {
        Self::from_settings(None, None)
    }
}
