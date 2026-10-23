//! The app's own right-click entries, in the file list's menu order with the tree-only rows where
//! the tree has them. One row per verb, not per menu: "Open in new tab" is one thing wherever it is
//! offered, so unticking it takes it out of both menus — the way unticking 7-Zip does.
//! Spec: Core/Services/ShellMenu/BuiltInMenuItems.cs.

use std::collections::HashSet;

use super::eq_ignore_case;

/// Which of the app's right-click menus a built-in entry is part of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltInMenuPlaces {
    FileList,
    FolderTree,
    Both,
}

impl BuiltInMenuPlaces {
    pub fn includes(self, place: BuiltInMenuPlaces) -> bool {
        match (self, place) {
            (BuiltInMenuPlaces::Both, _) | (_, BuiltInMenuPlaces::Both) => true,
            (a, b) => a == b,
        }
    }
}

/// One of the app's own right-click entries, as the Context menu page lists it. The id is what the
/// hidden list stores and what the menu tags the item with; the name is the menu's wording without
/// the ellipsis and without the counts a selection adds. The icon (an `Icon.*` resource name) is
/// what the menu shows beside it, so the Settings page can draw its preview of the menu the way
/// the menu draws itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltInMenuItem {
    pub id: &'static str,
    pub name: &'static str,
    pub places: BuiltInMenuPlaces,
    pub icon: Option<&'static str>,
    /// The `CommandCatalog` command this entry runs, whose shortcut the menu prints beside it —
    /// read from the keymap in force, never written here, so rebinding a key changes the menu too.
    /// None for a row that is a group rather than one command: New and "Open in new pane" are
    /// submenus, and Extract is two items under one id.
    pub command_id: Option<&'static str>,
}

impl BuiltInMenuItem {
    pub fn is_in(&self, place: BuiltInMenuPlaces) -> bool {
        self.places.includes(place)
    }
}

use BuiltInMenuPlaces::{Both, FileList, FolderTree};

const fn row(
    id: &'static str,
    name: &'static str,
    places: BuiltInMenuPlaces,
    icon: Option<&'static str>,
    command_id: Option<&'static str>,
) -> BuiltInMenuItem {
    BuiltInMenuItem {
        id,
        name,
        places,
        icon,
        command_id,
    }
}

const ALL: &[BuiltInMenuItem] = &[
    row("new", "New", Both, Some("Icon.Add"), None),
    row(
        "open",
        "Open",
        FileList,
        Some("Icon.Open"),
        Some("file.open"),
    ),
    row(
        "run-as-admin",
        "Run as administrator",
        FileList,
        Some("Icon.Shield"),
        Some("file.run-as-admin"),
    ),
    row(
        "open-new-tab",
        "Open in new tab",
        Both,
        Some("Icon.OpenInNewTab"),
        Some("file.open-new-tab"),
    ),
    row(
        "open-new-pane",
        "Open in new pane",
        Both,
        Some("Icon.OpenInNewPane"),
        None,
    ),
    row(
        "open-terminal",
        "Open in Terminal",
        Both,
        Some("Icon.Terminal"),
        Some("file.open-terminal"),
    ),
    row(
        "open-vscode",
        "Open in VS Code",
        Both,
        Some("Icon.VSCode"),
        Some("file.open-vscode"),
    ),
    row(
        "disk-usage",
        "Analyse disk usage",
        Both,
        Some("Icon.DiskUsage"),
        Some("tools.disk-usage"),
    ),
    row(
        "duplicates",
        "Find duplicates",
        Both,
        Some("Icon.Duplicates"),
        Some("tools.duplicates"),
    ),
    row(
        "changes",
        "What changed here",
        Both,
        Some("Icon.Changes"),
        Some("tools.changes"),
    ),
    row(
        "compare-panes",
        "Compare with other pane",
        FileList,
        Some("Icon.Compare"),
        Some("tools.compare"),
    ),
    row(
        "compress",
        "Compress",
        FileList,
        Some("Icon.Archive"),
        Some("file.compress"),
    ),
    row(
        "extract",
        "Extract here / Extract to",
        FileList,
        Some("Icon.MoveToFolder"),
        None,
    ),
    row(
        "copy-path",
        "Copy as path",
        Both,
        Some("Icon.Copy"),
        Some("edit.copy-path"),
    ),
    row(
        "copy-name",
        "Copy name",
        FileList,
        None,
        Some("edit.copy-name"),
    ),
    row(
        "compare-files",
        "Compare these two files",
        FileList,
        Some("Icon.CompareFiles"),
        Some("file.compare-files"),
    ),
    row(
        "settle-by-content",
        "Settle by content",
        FileList,
        Some("Icon.CompareFiles"),
        Some("file.settle-by-content"),
    ),
    row(
        "verify-checksums",
        "Verify checksums",
        FileList,
        Some("Icon.Checksum"),
        Some("file.verify-checksums"),
    ),
    row(
        "checksum",
        "Checksum",
        FileList,
        Some("Icon.Checksum"),
        Some("file.checksum"),
    ),
    row("cut", "Cut", FileList, Some("Icon.Cut"), Some("edit.cut")),
    row(
        "copy",
        "Copy",
        FileList,
        Some("Icon.Copy"),
        Some("edit.copy"),
    ),
    row(
        "paste",
        "Paste",
        FileList,
        Some("Icon.Paste"),
        Some("edit.paste"),
    ),
    row(
        "rename",
        "Rename",
        FileList,
        Some("Icon.Rename"),
        Some("file.rename"),
    ),
    row(
        "delete",
        "Delete",
        Both,
        Some("Icon.Delete"),
        Some("file.delete"),
    ),
    row(
        "delete-permanently",
        "Delete permanently",
        FileList,
        Some("Icon.DeletePermanent"),
        Some("file.delete-permanently"),
    ),
    row(
        "bookmark",
        "Bookmark",
        Both,
        Some("Icon.Bookmark"),
        Some("file.bookmark"),
    ),
    row(
        "properties",
        "Properties",
        Both,
        Some("Icon.Properties"),
        Some("file.properties"),
    ),
];

pub struct BuiltInMenuItems;

impl BuiltInMenuItems {
    pub fn all() -> &'static [BuiltInMenuItem] {
        ALL
    }

    pub fn is_known(id: &str) -> bool {
        Self::find(id).is_some()
    }

    /// Case-insensitive.
    pub fn find(id: &str) -> Option<&'static BuiltInMenuItem> {
        ALL.iter().find(|i| eq_ignore_case(i.id, id))
    }

    /// Whether the user has unticked this entry (ids compared ignoring case). An id nothing tags is
    /// a bug in the menu, not a preference, and panics rather than quietly showing or hiding.
    pub fn is_shown(id: &str, hidden_ids: &HashSet<String>) -> bool {
        assert!(
            Self::is_known(id),
            "'{id}' is not one of the app's menu entries."
        );
        !hidden_ids.iter().any(|h| eq_ignore_case(h, id))
    }

    /// The wording for where a row appears, beside its name on the Settings page.
    pub fn places_text(places: BuiltInMenuPlaces) -> &'static str {
        match places {
            FileList => "file list",
            FolderTree => "folder tree",
            Both => "file list and tree",
        }
    }
}
