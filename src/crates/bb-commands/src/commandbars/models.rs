//! The shape of an arrangement. One `BarItem` serves a menu and a toolbar alike, so an item can be
//! dragged from one to the other and a saved arrangement is one tree. The kind is text on disk: a
//! kind a later build adds must survive a round trip through this one, which is why `Other` exists
//! and why the cleaner keeps it.
//! Spec: Core/Services/CommandBars/CommandBarModels.cs.

use serde::{Deserialize, Deserializer, Serialize};

/// What a `BarItem` is.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum BarItemKind {
    /// A command, by the id the registry runs it under — a catalogue id, or `custom:<guid>` for
    /// one of the user's own.
    Command,
    Separator,
    /// A submenu: a caption and children. On a toolbar, a button that drops them down.
    Menu,
    /// Rows that come from data — the saved workspaces, the drives — by a `CommandBarLists` id.
    List,
    /// A kind this build has never heard of, kept as written.
    Other(String),
}

impl BarItemKind {
    pub const COMMAND: &'static str = "command";
    pub const SEPARATOR: &'static str = "separator";
    pub const MENU: &'static str = "menu";
    pub const LIST: &'static str = "list";

    pub fn parse(text: &str) -> BarItemKind {
        match text {
            Self::COMMAND => BarItemKind::Command,
            Self::SEPARATOR => BarItemKind::Separator,
            Self::MENU => BarItemKind::Menu,
            Self::LIST => BarItemKind::List,
            other => BarItemKind::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            BarItemKind::Command => Self::COMMAND,
            BarItemKind::Separator => Self::SEPARATOR,
            BarItemKind::Menu => Self::MENU,
            BarItemKind::List => Self::LIST,
            BarItemKind::Other(text) => text,
        }
    }
}

impl Default for BarItemKind {
    fn default() -> Self {
        BarItemKind::Other(String::new())
    }
}

impl From<String> for BarItemKind {
    fn from(text: String) -> Self {
        BarItemKind::parse(&text)
    }
}

impl From<BarItemKind> for String {
    fn from(kind: BarItemKind) -> Self {
        kind.as_str().to_string()
    }
}

/// One entry of a menu or a toolbar.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BarItem {
    pub kind: BarItemKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// For a menu, its name. For a command, the words to show instead of its own — "Name" under
    /// "Sort by" — or None for those. An `&` marks the access key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "without_nulls_opt"
    )]
    pub items: Option<Vec<BarItem>>,
    /// On a toolbar, whether this button carries its caption beside its icon. None follows the
    /// toolbar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_text: Option<bool>,
}

impl BarItem {
    pub fn separator() -> BarItem {
        BarItem {
            kind: BarItemKind::Separator,
            ..Default::default()
        }
    }

    pub fn command(id: &str) -> BarItem {
        BarItem {
            kind: BarItemKind::Command,
            id: Some(id.to_string()),
            ..Default::default()
        }
    }

    /// A command shown under words of this spot's own.
    pub fn command_with_caption(id: &str, caption: &str) -> BarItem {
        BarItem {
            caption: Some(caption.to_string()),
            ..BarItem::command(id)
        }
    }

    pub fn menu(caption: &str, items: Vec<BarItem>) -> BarItem {
        BarItem {
            kind: BarItemKind::Menu,
            caption: Some(caption.to_string()),
            items: Some(items),
            ..Default::default()
        }
    }

    pub fn list(id: &str) -> BarItem {
        BarItem {
            kind: BarItemKind::List,
            id: Some(id.to_string()),
            ..Default::default()
        }
    }

    pub fn is_command(&self) -> bool {
        self.kind == BarItemKind::Command
    }
    pub fn is_separator(&self) -> bool {
        self.kind == BarItemKind::Separator
    }
    pub fn is_menu(&self) -> bool {
        self.kind == BarItemKind::Menu
    }
    pub fn is_list(&self) -> bool {
        self.kind == BarItemKind::List
    }

    /// A menu's children; empty for anything else or a menu with none.
    pub fn children(&self) -> &[BarItem] {
        self.items.as_deref().unwrap_or(&[])
    }
}

/// One toolbar: the title bar's, or a row under it.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ToolbarSpec {
    #[serde(deserialize_with = "without_nulls")]
    pub items: Vec<BarItem>,
    /// Whether its buttons carry their captions unless one says otherwise.
    pub show_text: bool,
}

impl ToolbarSpec {
    pub fn new(items: Vec<BarItem>) -> ToolbarSpec {
        ToolbarSpec {
            items,
            show_text: false,
        }
    }
}

/// The whole arrangement: the menu bar, the title bar's toolbar and any rows below. This is what a
/// saved arrangement holds and what the settings file keeps as the one in force.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CommandBarSet {
    /// Which `CommandBarDefaults::REVISION` this was arranged against, so a later build can tell
    /// what has been added since.
    pub revision: i32,
    /// The top-level menus, each a `BarItemKind::Menu`.
    #[serde(deserialize_with = "without_nulls")]
    pub menus: Vec<BarItem>,
    #[serde(deserialize_with = "or_default")]
    pub title_bar: ToolbarSpec,
    #[serde(deserialize_with = "without_nulls")]
    pub rows: Vec<ToolbarSpec>,
}

/// An arrangement somebody named, to come back to.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedCommandBarSet {
    pub name: String,
    pub set: CommandBarSet,
}

impl SavedCommandBarSet {
    pub fn new(name: &str, set: CommandBarSet) -> SavedCommandBarSet {
        SavedCommandBarSet {
            name: name.to_string(),
            set,
        }
    }
}

/// A hand-edited file can hold `null` where an item goes; the holes are skipped rather than failing
/// the whole arrangement, and `null` for the list itself is an empty one.
fn without_nulls<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let items: Option<Vec<Option<T>>> = Option::deserialize(deserializer)?;
    Ok(items.into_iter().flatten().flatten().collect())
}

fn without_nulls_opt<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let items: Option<Vec<Option<T>>> = Option::deserialize(deserializer)?;
    Ok(items.map(|items| items.into_iter().flatten().collect()))
}

fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    let value: Option<T> = Option::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}
