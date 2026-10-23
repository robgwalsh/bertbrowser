//! Types the settings file embeds that belong to areas not yet ported, plus re-exports of the
//! ones that have landed since. Each
//! is the minimal shape with the C# field names, so the orchestrator swaps a `use` line and nothing
//! else. Behaviour that lives on these types (defaults, the one or two rules the settings need) is
//! carried over verbatim.

use serde::{Deserialize, Serialize};

pub use crate::commandbars::{BarItem, CommandBarSet, SavedCommandBarSet, ToolbarSpec};
pub use crate::custom_commands::CustomCommandDefinition;

/// A built-in id (`dark-plus`, `light-plus`, …) or the file stem of one of the user's own themes.
/// The rules about one (`is_safe`, `unique`) live in `crate::theming::ThemeId`; the value is text.
pub type ThemeId = String;

// TODO(port): extended when NewItem is ported
/// One entry on the "New" submenu. Only two shapes: a template is either empty or a file on disk.
/// Spec: Core/Services/NewItem/NewFileTemplate.cs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NewFileTemplate {
    /// What the menu says, e.g. "Text Document".
    pub label: String,
    /// Including the dot, e.g. ".txt".
    pub extension: String,
    /// A file to copy the new file's contents from, or None for an empty one.
    pub template_path: Option<String>,
    /// Whether it appears on the menu. True is also what an entry written before this existed
    /// deserializes to.
    pub enabled: bool,
}

impl Default for NewFileTemplate {
    fn default() -> Self {
        NewFileTemplate {
            label: String::new(),
            extension: String::new(),
            template_path: None,
            enabled: true,
        }
    }
}

impl NewFileTemplate {
    pub fn new(label: &str, extension: &str) -> NewFileTemplate {
        NewFileTemplate {
            label: label.to_string(),
            extension: extension.to_string(),
            ..Default::default()
        }
    }

    /// The name the dialog opens with, before its extension: "New Text Document".
    pub fn default_base_name(&self) -> String {
        format!("New {}", self.label)
    }

    /// What ships when the user has never configured the list.
    pub fn defaults() -> Vec<NewFileTemplate> {
        vec![
            NewFileTemplate::new("Text Document", ".txt"),
            NewFileTemplate::new("Markdown Document", ".md"),
            NewFileTemplate::new("JSON File", ".json"),
        ]
    }
}

// TODO(port): extended when Rename is ported
/// How the scoped part of a name is re-cased.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RenameCase {
    #[default]
    AsIs,
    Lower,
    Upper,
    Title,
    Sentence,
}

// TODO(port): extended when Rename is ported
/// Which part of the *existing* name find/replace and the case transform act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RenameScope {
    /// The name without its extension. A folder is all stem.
    #[default]
    Stem,
    /// The extension, leading dot included. Empty for a folder.
    Extension,
    /// Stem and extension together.
    WholeName,
}

// TODO(port): extended when Rename is ported
/// Everything the rename dialog can ask for. Pure data. The scope and case are written as names,
/// not numbers: reordering the enum later must not reinterpret what was already saved.
/// Spec: Core/Services/Rename/RenameRule.cs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RenameRule {
    pub template: String,
    pub find: String,
    pub replace: String,
    pub use_regex: bool,
    pub match_case: bool,
    pub scope: RenameScope,
    pub case: RenameCase,
    pub counter_start: i32,
    pub counter_step: i32,
    /// True for the rule the plain F2 box has always produced: the template is taken literally,
    /// braces and all, and no token is expanded.
    pub is_literal: bool,
}

impl Default for RenameRule {
    /// What the expanded panel starts from before anything is typed.
    fn default() -> Self {
        RenameRule {
            template: "{name}".to_string(),
            find: String::new(),
            replace: String::new(),
            use_regex: false,
            match_case: false,
            scope: RenameScope::Stem,
            case: RenameCase::AsIs,
            counter_start: 1,
            counter_step: 1,
            is_literal: false,
        }
    }
}

impl RenameRule {
    /// The literal, token-free rule the plain rename box produces.
    pub fn simple(template: &str) -> RenameRule {
        RenameRule {
            template: template.to_string(),
            is_literal: true,
            ..Default::default()
        }
    }
}

// TODO(port): extended when Checksums is ported
/// The hash algorithms the checksum tool offers, weakest first. Ordered deliberately: CRC-32
/// catches corruption and nothing else, and sits at the end of a list rather than the front.
/// Spec: Core/Services/Checksums/ChecksumAlgorithm.cs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChecksumAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Crc32,
}

impl ChecksumAlgorithm {
    /// Every algorithm, in the order a UI should offer them.
    pub const ALL: [ChecksumAlgorithm; 5] = [
        ChecksumAlgorithm::Md5,
        ChecksumAlgorithm::Sha1,
        ChecksumAlgorithm::Sha256,
        ChecksumAlgorithm::Sha512,
        ChecksumAlgorithm::Crc32,
    ];

    /// The enum's own name, as the settings file writes it.
    pub fn name(self) -> &'static str {
        match self {
            ChecksumAlgorithm::Md5 => "Md5",
            ChecksumAlgorithm::Sha1 => "Sha1",
            ChecksumAlgorithm::Sha256 => "Sha256",
            ChecksumAlgorithm::Sha512 => "Sha512",
            ChecksumAlgorithm::Crc32 => "Crc32",
        }
    }

    /// `Enum.TryParse(name, ignoreCase: true)`.
    pub fn parse_ignore_case(name: &str) -> Option<ChecksumAlgorithm> {
        Self::ALL
            .into_iter()
            .find(|a| a.name().eq_ignore_ascii_case(name.trim()))
    }

    /// The requested set in the offered order, once each.
    pub fn normalise(
        requested: impl IntoIterator<Item = ChecksumAlgorithm>,
    ) -> Vec<ChecksumAlgorithm> {
        let wanted: Vec<ChecksumAlgorithm> = requested.into_iter().collect();
        Self::ALL
            .into_iter()
            .filter(|a| wanted.contains(a))
            .collect()
    }
}

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
