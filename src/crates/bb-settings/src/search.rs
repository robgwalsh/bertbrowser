//! What the Settings page's search box finds. A table, because a setting's other names ("dark mode",
//! "hotkeys") are written nowhere on its page, and building every page to read it would run their
//! registry reads and database counts on each opening. `label` is the words exactly as the page shows
//! them, which is how the page finds the row to point at, so it must be text the page always shows:
//! a row that comes and goes is a keyword on one that stays. `settings search-audit` in the harness
//! holds each label to its page.

use bb_commands::commands::CommandMatcher;

/// One thing the search can find.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsEntry {
    /// The id of the page it is on.
    pub page: &'static str,
    /// The words on the page.
    pub label: &'static str,
    /// Other things a person might call it.
    pub keywords: &'static [&'static str],
}

const fn entry(
    page: &'static str,
    label: &'static str,
    keywords: &'static [&'static str],
) -> SettingsEntry {
    SettingsEntry {
        page,
        label,
        keywords,
    }
}

pub struct SettingsIndex;

impl SettingsIndex {
    /// In page order, a page's heading first: the order equally good matches are shown in.
    pub const ENTRIES: &'static [SettingsEntry] = &[
        entry("general", "General", &["preferences", "options"]),
        entry(
            "general",
            "Show hidden items",
            &["hidden files", "dotfiles", "system files", "invisible"],
        ),
        entry(
            "general",
            "Open archives as folders",
            &["zip", "7z", "rar", "tar", "double-click", "browse archives"],
        ),
        entry(
            "general",
            "Middle-click a drive/device to open a new panel",
            &["drives", "devices", "new pane", "sidebar"],
        ),
        entry(
            "general",
            "Restore last session on startup",
            &["reopen tabs", "remember panes", "launch"],
        ),
        entry(
            "general",
            "Start in directory",
            &["startup folder", "default path", "home folder", "launch"],
        ),
        entry(
            "general",
            "Vertical scroll speed",
            &["mouse wheel", "scrolling", "multiplier"],
        ),
        entry(
            "general",
            "Tile aspect ratio",
            &["thumbnails", "tiles", "shape"],
        ),
        entry(
            "general",
            "Icon size",
            &[
                "toolbar",
                "buttons",
                "compact",
                "larger",
                "smaller",
                "title bar",
            ],
        ),
        entry(
            "general",
            "Open folders in BertBrowser instead of File Explorer",
            &["default file manager", "folder handler", "replace explorer"],
        ),
        entry(
            "general",
            "Win+E opens BertBrowser",
            &["windows key", "explorer", "file manager", "shortcut"],
        ),
        entry(
            "appearance",
            "Appearance",
            &["theme", "look", "colors", "colours"],
        ),
        entry(
            "appearance",
            "Match Windows light/dark",
            &[
                "dark mode",
                "light mode",
                "night mode",
                "follow system",
                "high contrast",
                "theme",
            ],
        ),
        entry(
            "appearance",
            "Customise colours\u{2026}",
            &["customize colors", "theme editor", "accent", "palette"],
        ),
        entry("preview", "Preview", &["side pane", "preview pane"]),
        entry(
            "preview",
            "Show the preview pane in new tabs",
            &["side pane", "default"],
        ),
        entry(
            "preview",
            "Read at most",
            &["text preview", "size limit", "trim", "bytes"],
        ),
        entry(
            "preview",
            "Search inside files up to",
            &["content search", "content:", "size limit", "bytes"],
        ),
        entry(
            "search-index",
            "Search index",
            &[
                "mft",
                "ntfs",
                "master file table",
                "administrator",
                "background indexer",
                "helper",
                "start",
                "stop",
                "memory",
                "ram",
                "cpu",
                "resource use",
                "process",
            ],
        ),
        entry(
            "search-index",
            "Start the indexer when BertBrowser launches",
            &["startup", "uac", "helper"],
        ),
        entry(
            "search-index",
            "Start the indexer automatically when I sign in",
            &["login", "logon", "boot", "scheduled task", "helper"],
        ),
        entry(
            "history",
            "History",
            &["what changed", "privacy", "tracking"],
        ),
        entry(
            "history",
            "Record file changes",
            &["what changed", "change log", "usn journal", "tracking"],
        ),
        entry(
            "history",
            "Keep changes for",
            &["retention", "how long", "hours", "days"],
        ),
        entry(
            "history",
            "Clear history now",
            &["delete change log", "wipe", "privacy"],
        ),
        entry("undo", "Undo/Redo", &["revert", "history"]),
        entry(
            "undo",
            "Keep up to",
            &["undo steps", "undo actions", "undo limit", "redo"],
        ),
        entry(
            "undo",
            "Hold up to this much set-aside data",
            &[
                "undo size",
                "undo limit",
                "disk space",
                "gigabytes",
                "holding folder",
            ],
        ),
        entry(
            "undo",
            "Clear the undo history now",
            &["release", "free space"],
        ),
        entry(
            "new-items",
            "New file types",
            &["new menu", "templates", "shellnew", "create file"],
        ),
        entry(
            "new-items",
            "Show this type on the New menu",
            &["hide", "enable", "file type"],
        ),
        entry(
            "new-items",
            "Import from Windows\u{2026}",
            &["shellnew", "new menu", "file types"],
        ),
        entry(
            "columns",
            "Columns",
            &["file list", "details view", "order", "default columns"],
        ),
        entry(
            "columns",
            "Add column\u{2026}",
            &["file list", "properties", "details view"],
        ),
        entry(
            "saved-searches",
            "Saved searches",
            &["queries", "favourites", "favorites"],
        ),
        entry(
            "saved-searches",
            "Show saved searches",
            &["sidebar", "title bar", "placement", "hide"],
        ),
        entry(
            "workspaces",
            "Workspaces",
            &["layouts", "sessions", "panes", "tabs"],
        ),
        entry(
            "workspaces",
            "Show the workspace switcher",
            &["sidebar", "title bar", "placement", "hide"],
        ),
        entry(
            "context-menu",
            "Context menu",
            &[
                "right-click",
                "right click",
                "shell extensions",
                "7-zip",
                "git",
                "tortoisesvn",
            ],
        ),
        entry(
            "context-menu",
            "Show entries added by other programs",
            &["shell extensions", "right-click", "right click"],
        ),
        entry(
            "context-menu",
            "In the menu",
            &["order", "layout", "separator"],
        ),
        entry(
            "context-menu",
            "New command\u{2026}",
            &["custom commands", "run program", "external tool"],
        ),
        entry(
            "command-bars",
            "Menus and toolbars",
            &[
                "menu bar",
                "title bar",
                "buttons",
                "customise",
                "customize",
                "show names",
                "icons",
            ],
        ),
        entry(
            "command-bars",
            "Saved arrangements",
            &["toolbar layouts", "menu layouts"],
        ),
        entry(
            "command-bars",
            "Add toolbar",
            &["new toolbar", "toolbar row"],
        ),
        entry(
            "command-bars",
            "Reset to default",
            &["restore menus", "restore toolbars"],
        ),
        entry(
            "keyboard",
            "Keyboard",
            &[
                "shortcuts",
                "hotkeys",
                "keys",
                "keybindings",
                "chords",
                "rebind",
            ],
        ),
        entry(
            "keyboard",
            "Find by key\u{2026}",
            &["which command", "shortcut lookup", "hotkey"],
        ),
        entry(
            "keyboard",
            "Reset all",
            &["restore shortcuts", "default shortcuts", "hotkeys"],
        ),
    ];

    /// What `query` finds, best first: every word typed has to be found somewhere, and on the page
    /// (its label) beats another name for it, which beats the name of the page it is on. `page_name`
    /// says what a page id is called. A blank query finds nothing.
    pub fn find<'a>(
        query: &str,
        page_name: impl Fn(&str) -> &'a str,
    ) -> Vec<&'static SettingsEntry> {
        let mut hits: Vec<_> = Self::ENTRIES
            .iter()
            .filter_map(|e| {
                let found =
                    CommandMatcher::matches(query, e.label, e.keywords, page_name(e.page), "");
                found.is_match().then_some((found.tier, found.score, e))
            })
            .collect();
        // Stable, so what ties stays in page order.
        hits.sort_by_key(|hit| std::cmp::Reverse((hit.0, hit.1)));
        hits.into_iter().map(|(_, _, e)| e).collect()
    }
}
