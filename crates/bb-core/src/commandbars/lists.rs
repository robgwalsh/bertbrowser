//! Every list a menu or toolbar can hold. What each one expands to is the app's business; this is
//! the catalogue the editor offers and the rules consult.
//! Spec: Core/Services/CommandBars/CommandBarLists.cs.

/// A run of menu rows that comes from data rather than from the catalogue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandBarList {
    pub id: &'static str,
    /// What the Settings page calls it.
    pub name: &'static str,
    pub icon: &'static str,
    /// The greyed row shown when there is nothing to list, or None when an empty list simply
    /// shows nothing — which lets the submenu around it disappear.
    pub empty_text: Option<&'static str>,
}

pub struct CommandBarLists;

impl CommandBarLists {
    pub const WORKSPACES: &'static str = "workspaces";
    pub const SAVED_SEARCHES: &'static str = "saved-searches";
    pub const BOOKMARKS: &'static str = "bookmarks";
    pub const NEW_FILE_TYPES: &'static str = "new-file-types";
    pub const CUSTOM_COMMANDS: &'static str = "custom-commands";
    pub const COMMAND_BARS: &'static str = "command-bars";
    pub const DRIVES: &'static str = "drives";
    pub const UNDO_STEPS: &'static str = "undo-steps";
    pub const REDO_STEPS: &'static str = "redo-steps";

    const ALL: [CommandBarList; 9] = [
        CommandBarList {
            id: Self::WORKSPACES,
            name: "Saved workspaces",
            icon: "Icon.Tab",
            empty_text: Some("No saved workspaces"),
        },
        CommandBarList {
            id: Self::SAVED_SEARCHES,
            name: "Saved searches",
            icon: "Icon.Search",
            empty_text: Some("No saved searches"),
        },
        CommandBarList {
            id: Self::BOOKMARKS,
            name: "Bookmarks",
            icon: "Icon.Bookmark",
            empty_text: Some("No bookmarks"),
        },
        CommandBarList {
            id: Self::NEW_FILE_TYPES,
            name: "New file types",
            icon: "Icon.NewFile",
            empty_text: None,
        },
        CommandBarList {
            id: Self::CUSTOM_COMMANDS,
            name: "Your commands",
            icon: "Icon.CustomCommand",
            empty_text: None,
        },
        CommandBarList {
            id: Self::COMMAND_BARS,
            name: "Saved menu and toolbar arrangements",
            icon: "Icon.Reorder",
            empty_text: Some("No saved arrangements"),
        },
        CommandBarList {
            id: Self::DRIVES,
            name: "Drives",
            icon: "Icon.Drive",
            empty_text: None,
        },
        CommandBarList {
            id: Self::UNDO_STEPS,
            name: "Steps to undo",
            icon: "Icon.Undo",
            empty_text: Some("Nothing to undo"),
        },
        CommandBarList {
            id: Self::REDO_STEPS,
            name: "Steps to redo",
            icon: "Icon.Redo",
            empty_text: Some("Nothing to redo"),
        },
    ];

    pub fn all() -> &'static [CommandBarList] {
        &Self::ALL
    }

    /// Exact, case-sensitive.
    pub fn find(id: &str) -> Option<&'static CommandBarList> {
        Self::ALL.iter().find(|l| l.id == id)
    }
}
