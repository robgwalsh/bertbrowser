//! The menus and toolbar the app ships with. Not every command is here — the palette reaches all
//! of them — only the ones somebody would look for under a heading.
//!
//! Top-level access keys are spelled out and stay clear of D, P and M: Alt+D, Alt+P and Alt+M are
//! shipped shortcuts, and the key dispatcher claims a chord before any menu sees it.
//! Spec: Core/Services/CommandBars/CommandBarDefaults.cs.

use std::sync::LazyLock;

use super::{BarItem, CommandBarLists as L, CommandBarSet, ToolbarSpec};

pub struct CommandBarDefaults;

impl CommandBarDefaults {
    /// Raised whenever the default gains something a customised arrangement would want to hear
    /// about.
    pub const REVISION: i32 = 1;

    pub fn default_set() -> &'static CommandBarSet {
        &DEFAULT
    }
}

static DEFAULT: LazyLock<CommandBarSet> = LazyLock::new(|| CommandBarSet {
    revision: CommandBarDefaults::REVISION,
    menus: vec![file(), edit(), view(), go(), tools(), help()],
    title_bar: ToolbarSpec::new(vec![
        c("edit.undo"),
        c("edit.redo"),
        sep(),
        c("tools.disk-usage"),
        c("tools.duplicates"),
        c("tools.changes"),
        c("tools.compare"),
        c("app.settings"),
    ]),
    rows: vec![],
});

fn sep() -> BarItem {
    BarItem::separator()
}

fn c(id: &str) -> BarItem {
    BarItem::command(id)
}

fn cc(id: &str, caption: &str) -> BarItem {
    BarItem::command_with_caption(id, caption)
}

fn m(caption: &str, items: Vec<BarItem>) -> BarItem {
    BarItem::menu(caption, items)
}

fn file() -> BarItem {
    m(
        "&File",
        vec![
            c("tab.new"),
            m(
                "New",
                vec![
                    cc("file.new-folder", "Folder"),
                    cc("file.new-file", "Empty file"),
                    sep(),
                    BarItem::list(L::NEW_FILE_TYPES),
                ],
            ),
            sep(),
            c("file.open"),
            c("file.run-as-admin"),
            m(
                "Open in",
                vec![
                    cc("file.open-new-tab", "New tab"),
                    cc("file.open-pane-right", "Pane right"),
                    cc("file.open-pane-below", "Pane below"),
                    cc("pane.open-selected-in-other", "Other pane"),
                    sep(),
                    cc("file.open-terminal", "Terminal"),
                    cc("file.open-vscode", "VS Code"),
                ],
            ),
            sep(),
            c("file.rename"),
            c("file.delete"),
            c("file.delete-permanently"),
            sep(),
            c("file.copy-to"),
            c("file.move-to"),
            c("pane.copy-to-other"),
            c("pane.move-to-other"),
            c("file.create-shortcut"),
            sep(),
            m(
                "Archive",
                vec![
                    c("file.compress"),
                    c("file.extract-here"),
                    c("file.extract-to"),
                    cc("file.unlock-archive", "Unlock"),
                ],
            ),
            m("Your commands", vec![BarItem::list(L::CUSTOM_COMMANDS)]),
            sep(),
            c("file.properties"),
            sep(),
            c("tab.close"),
            c("app.exit"),
        ],
    )
}

fn edit() -> BarItem {
    m(
        "&Edit",
        vec![
            c("edit.undo"),
            c("edit.redo"),
            c("edit.undo-history"),
            sep(),
            c("edit.cut"),
            c("edit.copy"),
            c("edit.paste"),
            c("edit.paste-shortcut"),
            sep(),
            c("edit.copy-path"),
            c("edit.copy-name"),
            c("edit.copy-folder-path"),
            sep(),
            c("select.all"),
            c("select.none"),
            c("select.invert"),
            m(
                "Select",
                vec![
                    cc("select.pattern", "By pattern\u{2026}"),
                    cc("select.unpattern", "Deselect by pattern\u{2026}"),
                    sep(),
                    cc("select.files", "All files"),
                    cc("select.folders", "All folders"),
                ],
            ),
        ],
    )
}

fn view() -> BarItem {
    m(
        "&View",
        vec![
            c("view.details"),
            c("view.thumbnails"),
            c("view.thumbs-larger"),
            c("view.thumbs-smaller"),
            sep(),
            m(
                "Sort by",
                vec![
                    cc("view.sort-name", "Name"),
                    cc("view.sort-size", "Size"),
                    cc("view.sort-type", "Type"),
                    cc("view.sort-modified", "Date modified"),
                    cc("view.sort-created", "Date created"),
                    cc("view.sort-extension", "Extension"),
                    sep(),
                    cc("view.sort-reverse", "Reverse order"),
                ],
            ),
            m(
                "Columns",
                vec![
                    cc("view.add-column", "Add\u{2026}"),
                    cc("view.columns-default", "Use for new tabs"),
                    cc("view.reset-columns", "Reset"),
                ],
            ),
            sep(),
            c("view.preview"),
            c("view.metadata"),
            m(
                "Preview",
                vec![
                    cc("preview.mode-auto", "Auto"),
                    cc("preview.mode-raw", "Raw text"),
                    cc("preview.mode-hex", "Hex"),
                    sep(),
                    c("preview.wrap"),
                    c("preview.fit"),
                    cc("preview.fit-width", "Fit pane to content"),
                    sep(),
                    c("preview.play"),
                    cc("preview.play-next", "Play next"),
                    cc("preview.fullscreen", "Full screen"),
                    sep(),
                    c("preview.auto-advance"),
                    c("preview.loop"),
                    c("preview.autoplay"),
                ],
            ),
            sep(),
            c("view.flat"),
            c("view.hidden"),
            c("nav.refresh"),
            c("nav.refresh-all"),
            sep(),
            m(
                "Panes",
                vec![
                    cc("pane.split-right", "Split right"),
                    cc("pane.split-below", "Split below"),
                    cc("pane.close", "Close"),
                    sep(),
                    cc("pane.next", "Focus next"),
                    cc("pane.previous", "Focus previous"),
                    cc("pane.swap", "Swap"),
                    cc("pane.equalise", "Equalise sizes"),
                    sep(),
                    c("pane.open-here-in-other"),
                    sep(),
                    c("pane.focus-tree"),
                    c("pane.focus-list"),
                ],
            ),
            m(
                "Tabs",
                vec![
                    cc("tab.duplicate", "Duplicate"),
                    cc("tab.reopen", "Reopen closed"),
                    sep(),
                    cc("tab.next", "Next"),
                    cc("tab.previous", "Previous"),
                    cc("tab.move-left", "Move left"),
                    cc("tab.move-right", "Move right"),
                    sep(),
                    cc("tab.move-to-new-pane", "Move to new pane"),
                    cc("tab.move-to-other-pane", "Move to other pane"),
                    sep(),
                    cc("tab.close-others", "Close others"),
                    cc("tab.close-right", "Close to the right"),
                ],
            ),
            sep(),
            m(
                "Workspaces",
                vec![BarItem::list(L::WORKSPACES), sep(), c("app.save-workspace")],
            ),
            m(
                "Menus and toolbars",
                vec![
                    BarItem::list(L::COMMAND_BARS),
                    sep(),
                    cc("app.save-command-bars", "Save arrangement\u{2026}"),
                    cc("app.command-bars", "Customise\u{2026}"),
                ],
            ),
        ],
    )
}

fn go() -> BarItem {
    m(
        "&Go",
        vec![
            c("nav.back"),
            c("nav.forward"),
            c("nav.up"),
            cc("nav.drive-root", "Drive root"),
            sep(),
            c("nav.address-bar"),
            c("nav.goto"),
            c("nav.containing-folder"),
            sep(),
            cc("nav.home", "Home"),
            cc("nav.desktop", "Desktop"),
            cc("nav.documents", "Documents"),
            cc("nav.downloads", "Downloads"),
            sep(),
            m("Drives", vec![BarItem::list(L::DRIVES)]),
            m(
                "Bookmarks",
                vec![
                    c("app.bookmark-folder"),
                    c("file.bookmark"),
                    sep(),
                    BarItem::list(L::BOOKMARKS),
                ],
            ),
        ],
    )
}

fn tools() -> BarItem {
    m(
        "&Tools",
        vec![
            c("search.folder"),
            c("search.pc"),
            c("search.save"),
            c("search.clear"),
            m("Saved searches", vec![BarItem::list(L::SAVED_SEARCHES)]),
            sep(),
            c("tools.disk-usage"),
            c("tools.duplicates"),
            c("tools.changes"),
            sep(),
            c("tools.compare"),
            c("tools.compare-differences"),
            c("tools.compare-rescan"),
            c("tools.compare-sync"),
            c("file.compare-files"),
            c("file.settle-by-content"),
            sep(),
            c("file.checksum"),
            c("file.verify-checksums"),
            sep(),
            m(
                "Transfers",
                vec![
                    cc("tools.transfer-details", "Details"),
                    cc("tools.transfer-pause", "Pause"),
                    cc("tools.transfer-resume", "Resume"),
                    cc("tools.transfer-cancel", "Cancel"),
                ],
            ),
            sep(),
            c("app.customise-theme"),
            c("app.keyboard"),
            c("app.settings"),
        ],
    )
}

fn help() -> BarItem {
    m(
        "&Help",
        vec![c("app.palette"), c("search.syntax"), sep(), c("app.about")],
    )
}
