using L = BertBrowser.Core.Services.CommandBars.CommandBarLists;

namespace BertBrowser.Core.Services.CommandBars;

/// <summary>
/// The menus and toolbar the app ships with. Not every command is here — the palette reaches all
/// of them — only the ones somebody would look for under a heading.
/// </summary>
/// <remarks>
/// Top-level access keys are spelled out and stay clear of D, P and M: Alt+D, Alt+P and Alt+M are
/// shipped shortcuts, and the key dispatcher claims a chord before any menu sees it.
/// </remarks>
public static class CommandBarDefaults
{
    /// <summary>Raised whenever <see cref="Default"/> gains something a customised arrangement
    /// would want to hear about.</summary>
    public const int Revision = 1;

    public static CommandBarSet Default { get; } = new(
        Revision,
        [File(), Edit(), View(), Go(), Tools(), Help()],
        new ToolbarSpec(
        [
            C("edit.undo"), C("edit.redo"), Sep,
            C("tools.disk-usage"), C("tools.duplicates"), C("tools.changes"), C("tools.compare"),
            C("app.settings"),
        ]),
        []);

    private static BarItem Sep => BarItem.Separator;

    private static BarItem C(string id, string? caption = null) => BarItem.Command(id, caption);

    private static BarItem M(string caption, params BarItem[] items) => BarItem.Menu(caption, items);

    private static BarItem File() => M("&File",
        C("tab.new"),
        M("New", C("file.new-folder", "Folder"), C("file.new-file", "Empty file"), Sep, BarItem.List(L.NewFileTypes)),
        Sep,
        C("file.open"),
        C("file.run-as-admin"),
        M("Open in",
            C("file.open-new-tab", "New tab"), C("file.open-pane-right", "Pane right"),
            C("file.open-pane-below", "Pane below"), C("pane.open-selected-in-other", "Other pane"),
            Sep,
            C("file.open-terminal", "Terminal"), C("file.open-vscode", "VS Code")),
        Sep,
        C("file.rename"), C("file.delete"), C("file.delete-permanently"),
        Sep,
        C("file.copy-to"), C("file.move-to"), C("pane.copy-to-other"), C("pane.move-to-other"),
        C("file.create-shortcut"),
        Sep,
        M("Archive",
            C("file.compress"), C("file.extract-here"), C("file.extract-to"), C("file.unlock-archive", "Unlock")),
        M("Your commands", BarItem.List(L.CustomCommands)),
        Sep,
        C("file.properties"),
        Sep,
        C("tab.close"), C("app.exit"));

    private static BarItem Edit() => M("&Edit",
        C("edit.undo"), C("edit.redo"), C("edit.undo-history"),
        Sep,
        C("edit.cut"), C("edit.copy"), C("edit.paste"), C("edit.paste-shortcut"),
        Sep,
        C("edit.copy-path"), C("edit.copy-name"), C("edit.copy-folder-path"),
        Sep,
        C("select.all"), C("select.none"), C("select.invert"),
        M("Select",
            C("select.pattern", "By pattern…"), C("select.unpattern", "Deselect by pattern…"),
            Sep,
            C("select.files", "All files"), C("select.folders", "All folders")));

    private static BarItem View() => M("&View",
        C("view.details"), C("view.thumbnails"), C("view.thumbs-larger"), C("view.thumbs-smaller"),
        Sep,
        M("Sort by",
            C("view.sort-name", "Name"), C("view.sort-size", "Size"), C("view.sort-type", "Type"),
            C("view.sort-modified", "Date modified"), C("view.sort-created", "Date created"),
            C("view.sort-extension", "Extension"),
            Sep,
            C("view.sort-reverse", "Reverse order")),
        M("Columns",
            C("view.add-column", "Add…"), C("view.columns-default", "Use for new tabs"),
            C("view.reset-columns", "Reset")),
        Sep,
        C("view.preview"), C("view.metadata"),
        M("Preview",
            C("preview.mode-auto", "Auto"), C("preview.mode-raw", "Raw text"), C("preview.mode-hex", "Hex"),
            Sep,
            C("preview.wrap"), C("preview.fit"), C("preview.fit-width", "Fit pane to content"),
            Sep,
            C("preview.play"), C("preview.play-next", "Play next"), C("preview.fullscreen", "Full screen"),
            Sep,
            C("preview.auto-advance"), C("preview.loop"), C("preview.autoplay")),
        Sep,
        C("view.flat"), C("view.hidden"), C("nav.refresh"), C("nav.refresh-all"),
        Sep,
        M("Panes",
            C("pane.split-right", "Split right"), C("pane.split-below", "Split below"), C("pane.close", "Close"),
            Sep,
            C("pane.next", "Focus next"), C("pane.previous", "Focus previous"), C("pane.swap", "Swap"),
            C("pane.equalise", "Equalise sizes"),
            Sep,
            C("pane.open-here-in-other"),
            Sep,
            C("pane.focus-tree"), C("pane.focus-list")),
        M("Tabs",
            C("tab.duplicate", "Duplicate"), C("tab.reopen", "Reopen closed"),
            Sep,
            C("tab.next", "Next"), C("tab.previous", "Previous"),
            C("tab.move-left", "Move left"), C("tab.move-right", "Move right"),
            Sep,
            C("tab.move-to-new-pane", "Move to new pane"), C("tab.move-to-other-pane", "Move to other pane"),
            Sep,
            C("tab.close-others", "Close others"), C("tab.close-right", "Close to the right")),
        Sep,
        M("Workspaces", BarItem.List(L.Workspaces), Sep, C("app.save-workspace")),
        M("Menus and toolbars",
            BarItem.List(L.CommandBars), Sep, C("app.save-command-bars", "Save arrangement…"),
            C("app.command-bars", "Customise…")));

    private static BarItem Go() => M("&Go",
        C("nav.back"), C("nav.forward"), C("nav.up"), C("nav.drive-root", "Drive root"),
        Sep,
        C("nav.address-bar"), C("nav.goto"), C("nav.containing-folder"),
        Sep,
        C("nav.home", "Home"), C("nav.desktop", "Desktop"), C("nav.documents", "Documents"),
        C("nav.downloads", "Downloads"),
        Sep,
        M("Drives", BarItem.List(L.Drives)),
        M("Bookmarks", C("app.bookmark-folder"), C("file.bookmark"), Sep, BarItem.List(L.Bookmarks)));

    private static BarItem Tools() => M("&Tools",
        C("search.folder"), C("search.pc"), C("search.save"), C("search.clear"),
        M("Saved searches", BarItem.List(L.SavedSearches)),
        Sep,
        C("tools.disk-usage"), C("tools.duplicates"), C("tools.changes"),
        Sep,
        C("tools.compare"), C("tools.compare-differences"), C("tools.compare-rescan"), C("tools.compare-sync"),
        C("file.compare-files"), C("file.settle-by-content"),
        Sep,
        C("file.checksum"), C("file.verify-checksums"),
        Sep,
        M("Transfers",
            C("tools.transfer-details", "Details"), C("tools.transfer-pause", "Pause"),
            C("tools.transfer-resume", "Resume"), C("tools.transfer-cancel", "Cancel")),
        Sep,
        C("app.customise-theme"), C("app.keyboard"), C("app.settings"));

    private static BarItem Help() => M("&Help",
        C("app.palette"), C("search.syntax"),
        Sep,
        C("app.about"));
}
