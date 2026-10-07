using C = BertBrowser.Core.Services.Commands.CommandCategories;
using static BertBrowser.Core.Services.Commands.CommandContext;
using static BertBrowser.Core.Services.Commands.CommandProminence;

namespace BertBrowser.Core.Services.Commands;

/// <summary>
/// Every command the app has, in the order the palette's "Browse all commands" and the Keyboard
/// page list them. This table is the one place a command's name, icon and shipped shortcut are
/// written down: the key dispatcher, the menus' gesture text, the tooltips and the palette all read
/// it, which is what stops them drifting apart the way three key handlers and a column of
/// hand-typed <c>InputGestureText</c> did.
/// </summary>
/// <remarks>
/// An id here with no handler in the App — or a handler with no row here — throws when the registry
/// is built, so a command cannot be listed and do nothing. Ids are persisted (keymap overrides,
/// palette pins), so a row may be renamed or re-categorised freely but its id never changes meaning.
/// </remarks>
public static class CommandCatalog
{
    public static IReadOnlyList<CommandDescriptor> All { get; } =
    [
        // --- Go ---
        Cmd("nav.back", "Back", C.Navigation, Browser, "Icon.Back", "Backspace Alt+Left", Essential,
            "previous folder", "history"),
        Cmd("nav.forward", "Forward", C.Navigation, Browser, "Icon.Forward", "Alt+Right", Standard, "next folder", "history"),
        Cmd("nav.up", "Up to parent folder", C.Navigation, Browser, "Icon.Up", "Alt+Up", Essential, "parent", "go up"),
        Cmd("nav.refresh", "Refresh", C.Navigation, Browser, "Icon.Refresh", "F5", Essential, "reload", "rescan"),
        Cmd("nav.refresh-all", "Refresh all tabs", C.Navigation, Browser, "Icon.Refresh", "", Rare, "reload everything"),
        // Three chords because three programs taught three: a browser's, Explorer's, and the F4 that
        // drops the address list in both.
        Cmd("nav.address-bar", "Edit address", C.Navigation, Browser, "Icon.Rename", "Ctrl+L Alt+D F4", Essential,
            "path bar", "location bar", "type a path", "focus address"),
        // The palette, opened ready for a path: the app's command bar.
        Cmd("nav.goto", "Go to folder…", C.Navigation, Browser, "Icon.Folder", "Ctrl+G", Essential,
            "open path", "jump to", "cd", "change directory", "command bar"),
        Cmd("nav.drive-root", "Go to drive root", C.Navigation, Browser, "Icon.Up", "", Standard, "top of drive"),
        Cmd("nav.home", "Go to Home folder", C.Navigation, Browser, "Icon.Home", "", Standard, "user profile", "my files"),
        Cmd("nav.desktop", "Go to Desktop", C.Navigation, Browser, "Icon.Desktop", "", Standard),
        Cmd("nav.documents", "Go to Documents", C.Navigation, Browser, "Icon.Folder", "", Standard, "my documents"),
        Cmd("nav.downloads", "Go to Downloads", C.Navigation, Browser, "Icon.Download", "", Standard),
        Cmd("nav.containing-folder", "Open containing folder", C.Navigation, Browser, "Icon.MoveToFolder", "", Standard,
            "show in folder", "open file location", "reveal", "go to file"),

        // --- Tabs ---
        Cmd("tab.new", "New tab", C.Tabs, Browser, "Icon.OpenInNewTab", "Ctrl+T", Essential, "open tab"),
        Cmd("tab.duplicate", "Duplicate tab", C.Tabs, Browser, "Icon.OpenInNewTab", "Ctrl+Shift+K", Standard, "clone tab"),
        Cmd("tab.close", "Close tab", C.Tabs, Browser, "Icon.Dismiss", "Ctrl+W", Essential),
        Cmd("tab.close-others", "Close other tabs", C.Tabs, Browser, "Icon.Dismiss", "", Standard),
        Cmd("tab.close-right", "Close tabs to the right", C.Tabs, Browser, "Icon.Dismiss", "", Rare),
        Cmd("tab.reopen", "Reopen closed tab", C.Tabs, Browser, "Icon.Undo", "Ctrl+Shift+T", Standard, "restore tab", "undo close"),
        Cmd("tab.next", "Next tab", C.Tabs, Browser, "Icon.Tab", "Ctrl+Tab", Standard, "switch tab"),
        Cmd("tab.previous", "Previous tab", C.Tabs, Browser, "Icon.Tab", "Ctrl+Shift+Tab", Standard, "switch tab"),
        Cmd("tab.goto-1", "Go to tab 1", C.Tabs, Browser, "Icon.Tab", "Ctrl+1", Rare, "switch tab"),
        Cmd("tab.goto-2", "Go to tab 2", C.Tabs, Browser, "Icon.Tab", "Ctrl+2", Rare, "switch tab"),
        Cmd("tab.goto-3", "Go to tab 3", C.Tabs, Browser, "Icon.Tab", "Ctrl+3", Rare, "switch tab"),
        Cmd("tab.goto-4", "Go to tab 4", C.Tabs, Browser, "Icon.Tab", "Ctrl+4", Rare, "switch tab"),
        Cmd("tab.goto-5", "Go to tab 5", C.Tabs, Browser, "Icon.Tab", "Ctrl+5", Rare, "switch tab"),
        Cmd("tab.goto-6", "Go to tab 6", C.Tabs, Browser, "Icon.Tab", "Ctrl+6", Rare, "switch tab"),
        Cmd("tab.goto-7", "Go to tab 7", C.Tabs, Browser, "Icon.Tab", "Ctrl+7", Rare, "switch tab"),
        Cmd("tab.goto-8", "Go to tab 8", C.Tabs, Browser, "Icon.Tab", "Ctrl+8", Rare, "switch tab"),
        Cmd("tab.goto-last", "Go to last tab", C.Tabs, Browser, "Icon.Tab", "Ctrl+9", Rare, "switch tab"),
        Cmd("tab.move-to-new-pane", "Move tab to new pane", C.Tabs, Browser, "Icon.OpenInNewPane", "", Standard,
            "tear off", "detach tab"),
        Cmd("tab.move-to-other-pane", "Move tab to other pane", C.Tabs, Browser, "Icon.OpenInNewPane", "", Standard,
            "send tab"),
        Cmd("tab.move-left", "Move tab left", C.Tabs, Browser, "Icon.Tab", "", Rare, "reorder tabs"),
        Cmd("tab.move-right", "Move tab right", C.Tabs, Browser, "Icon.Tab", "", Rare, "reorder tabs"),

        // --- Panes ---
        Cmd("pane.split-right", "Split pane right", C.Panes, Browser, "Icon.SplitRight", "Ctrl+Alt+Right", Essential,
            "dual pane", "two panes", "side by side", "vertical split"),
        Cmd("pane.split-below", "Split pane below", C.Panes, Browser, "Icon.SplitBelow", "Ctrl+Alt+Down", Standard,
            "dual pane", "two panes", "horizontal split"),
        Cmd("pane.close", "Close pane", C.Panes, Browser, "Icon.Dismiss", "Ctrl+Shift+W", Standard, "unsplit", "single pane"),
        Cmd("pane.next", "Focus next pane", C.Panes, Browser, "Icon.SplitRight", "F6", Standard, "switch pane", "other pane"),
        Cmd("pane.previous", "Focus previous pane", C.Panes, Browser, "Icon.SplitRight", "Shift+F6", Rare, "switch pane", "other pane"),
        // Ctrl+U is the commanders' own chord for this, and the one a Total Commander hand reaches for.
        Cmd("pane.swap", "Swap panes", C.Panes, Browser, "Icon.Compare", "Ctrl+U", Standard, "exchange panes", "switch sides"),
        Cmd("pane.open-here-in-other", "Open this folder in other pane", C.Panes, Browser, "Icon.OpenInNewPane",
            "Ctrl+Shift+O", Standard, "same folder", "mirror", "target equals source"),
        Cmd("pane.open-selected-in-other", "Open selected folder in other pane", C.Panes, Browser, "Icon.OpenInNewPane",
            "", Standard),
        Cmd("pane.copy-to-other", "Copy selection to other pane", C.Panes, Browser, "Icon.Copy", "Ctrl+Shift+F5", Essential,
            "copy across", "copy to target", "f5 copy"),
        Cmd("pane.move-to-other", "Move selection to other pane", C.Panes, Browser, "Icon.MoveToFolder", "Ctrl+Shift+F6", Essential,
            "move across", "move to target", "f6 move"),
        Cmd("pane.equalise", "Equalise pane sizes", C.Panes, Browser, "Icon.SplitRight", "", Rare, "equalize", "even split", "50/50"),
        Cmd("pane.focus-tree", "Focus folder tree", C.Panes, Browser, "Icon.SplitRight", "Ctrl+Shift+E", Standard, "sidebar"),
        Cmd("pane.focus-list", "Focus file list", C.Panes, Browser, "Icon.SplitRight", "", Standard),

        // --- Selection ---
        Cmd("select.all", "Select all", C.Selection, FileList, "Icon.SelectAll", "Ctrl+A", Essential),
        Cmd("select.none", "Select none", C.Selection, FileList, "Icon.SelectAll", "Ctrl+Shift+A", Standard, "deselect all", "clear selection"),
        Cmd("select.invert", "Invert selection", C.Selection, FileList, "Icon.SelectAll", "Ctrl+I", Essential),
        // The number pad's + and - are what every commander selects and deselects by pattern with.
        Cmd("select.pattern", "Select by pattern…", C.Selection, FileList, "Icon.Search", "Num+", Essential,
            "select group", "wildcard", "filter", "select files matching"),
        Cmd("select.unpattern", "Deselect by pattern…", C.Selection, FileList, "Icon.Search", "Num-", Standard,
            "unselect group", "wildcard"),
        Cmd("select.files", "Select all files", C.Selection, FileList, "Icon.SelectAll", "", Standard, "files only"),
        Cmd("select.folders", "Select all folders", C.Selection, FileList, "Icon.SelectAll", "", Standard, "folders only", "directories"),

        // --- File ---
        Cmd("file.open", "Open", C.File, FileList, "Icon.Open", "Enter", Essential, "launch", "run", "enter folder"),
        Cmd("file.run-as-admin", "Run as administrator", C.File, FileList, "Icon.Shield", "Ctrl+Shift+Enter", Standard,
            "elevated", "admin", "uac"),
        Cmd("file.open-new-tab", "Open in new tab", C.File, Browser, "Icon.OpenInNewTab", "", Standard),
        Cmd("file.open-pane-right", "Open in pane right", C.File, Browser, "Icon.SplitRight", "", Standard, "open in new pane"),
        Cmd("file.open-pane-below", "Open in pane below", C.File, Browser, "Icon.SplitBelow", "", Standard, "open in new pane"),
        Cmd("file.open-terminal", "Open in Terminal", C.File, Browser, "Icon.Terminal", "", Essential,
            "command prompt", "cmd", "powershell", "console", "shell"),
        Cmd("file.open-vscode", "Open in VS Code", C.File, Browser, "Icon.VSCode", "", Standard, "code", "editor"),
        Cmd("file.new-folder", "New folder", C.File, FileList, "Icon.NewFolder", "Ctrl+Shift+N", Essential,
            "create folder", "make directory", "mkdir"),
        Cmd("file.new-file", "New empty file", C.File, Browser, "Icon.NewFile", "", Standard, "create file", "touch"),
        Cmd("file.rename", "Rename", C.File, FileList, "Icon.Rename", "F2", Essential,
            "batch rename", "multi-rename", "bulk rename", "regex rename"),
        Cmd("file.delete", "Delete", C.File, FileList, "Icon.Delete", "Del", Essential, "remove", "recycle bin", "trash"),
        Cmd("file.delete-permanently", "Delete permanently", C.File, FileList, "Icon.DeletePermanent", "Shift+Del", Standard,
            "erase", "remove", "skip recycle bin"),
        Cmd("file.properties", "Properties", C.File, FileList, "Icon.Properties", "Alt+Enter", Essential,
            "attributes", "details", "info"),
        Cmd("file.compress", "Compress", C.File, Browser, "Icon.Archive", "", Essential,
            "zip", "pack", "create archive", "tar"),
        Cmd("file.extract-here", "Extract here", C.File, Browser, "Icon.MoveToFolder", "", Essential, "unzip", "unpack"),
        Cmd("file.extract-to", "Extract to…", C.File, Browser, "Icon.MoveToFolder", "", Standard, "unzip", "unpack"),
        Cmd("file.unlock-archive", "Unlock archive", C.File, Browser, "Icon.Archive", "", Rare, "password", "encrypted"),
        Cmd("file.bookmark", "Bookmark selection", C.File, Browser, "Icon.Bookmark", "", Standard,
            "favourite", "favorite", "pin", "remove bookmark"),
        Cmd("file.checksum", "Checksum", C.File, Browser, "Icon.Checksum", "", Standard,
            "hash", "sha256", "md5", "sha1", "crc32", "digest"),
        Cmd("file.verify-checksums", "Verify checksums", C.File, Browser, "Icon.Checksum", "", Rare,
            "sfv", "hash", "sha256", "md5"),
        Cmd("file.compare-files", "Compare two files", C.File, Browser, "Icon.CompareFiles", "", Standard,
            "diff", "compare by content"),
        Cmd("file.settle-by-content", "Settle by content", C.File, Browser, "Icon.CompareFiles", "", Rare,
            "compare by content", "byte compare"),
        Cmd("file.create-shortcut", "Create shortcut here", C.File, Browser, "Icon.Open", "", Standard, "lnk", "link"),
        Cmd("file.copy-to", "Copy to folder…", C.File, Browser, "Icon.Copy", "", Standard, "copy to", "copy elsewhere"),
        Cmd("file.move-to", "Move to folder…", C.File, Browser, "Icon.MoveToFolder", "", Standard, "move to", "move elsewhere"),

        // --- Edit ---
        Cmd("edit.cut", "Cut", C.Edit, FileList, "Icon.Cut", "Ctrl+X", Essential, "move"),
        Cmd("edit.copy", "Copy", C.Edit, FileList, "Icon.Copy", "Ctrl+C", Essential),
        Cmd("edit.paste", "Paste", C.Edit, FileList, "Icon.Paste", "Ctrl+V", Essential),
        Cmd("edit.copy-path", "Copy as path", C.Edit, FileList, "Icon.Copy", "Ctrl+Shift+C", Essential,
            "copy full path", "copy location"),
        Cmd("edit.copy-name", "Copy name", C.Edit, Browser, "Icon.Copy", "", Standard, "copy filename"),
        Cmd("edit.copy-folder-path", "Copy current folder path", C.Edit, Browser, "Icon.Copy", "", Standard,
            "copy address", "copy location"),
        Cmd("edit.paste-shortcut", "Paste shortcut", C.Edit, Browser, "Icon.Paste", "", Standard, "paste as link", "lnk"),
        Cmd("edit.undo", "Undo", C.Edit, Browser, "Icon.Undo", "Ctrl+Z", Essential),
        Cmd("edit.redo", "Redo", C.Edit, Browser, "Icon.Redo", "Ctrl+Y Ctrl+Shift+Z", Standard),
        Cmd("edit.undo-history", "Undo history", C.Edit, Browser, "Icon.Changes", "Ctrl+Alt+Z", Standard,
            "undo list", "redo list"),
        Cmd("edit.clear-undo-history", "Clear undo history", C.Edit, Browser, "Icon.Delete", "", Rare),

        // --- View ---
        Cmd("view.preview", "Toggle preview pane", C.View, Browser, "Icon.PreviewPane", "Ctrl+P Alt+P", Essential,
            "quick look", "viewer", "show preview", "hide preview"),
        Cmd("view.metadata", "Toggle metadata pane", C.View, Browser, "Icon.Metadata", "Ctrl+M Alt+M", Essential,
            "edit metadata", "tags", "exif", "id3", "details", "mp3 tag", "artist", "album", "date taken",
            "remove location", "gps"),
        Cmd("view.side-pane", "Cycle side pane: none, preview, metadata", C.View, Browser, "Icon.PreviewPane", "", Rare,
            "side panel", "switch pane"),
        Cmd("view.flat", "Toggle flat view", C.View, Browser, "Icon.FlatView", "Ctrl+B", Essential,
            "branch view", "flatten", "show subfolders", "recursive"),
        Cmd("view.hidden", "Toggle hidden items", C.View, Browser, "Icon.Hidden", "Ctrl+H", Essential,
            "show hidden files", "hide hidden files", "dotfiles", "system files"),
        Cmd("view.details", "Details view", C.View, Browser, "Icon.DetailsList", "", Standard, "list view", "columns"),
        Cmd("view.thumbnails", "Thumbnail view", C.View, Browser, "Icon.Thumbnails", "", Standard, "tiles", "icons", "grid"),
        Cmd("view.toggle-thumbnails", "Toggle details / thumbnails", C.View, Browser, "Icon.DetailsList", "", Standard,
            "tiles", "icons", "view mode"),
        Cmd("view.thumbs-larger", "Larger thumbnails", C.View, Browser, "Icon.Thumbnails", "Ctrl+=", Standard, "zoom in", "bigger"),
        Cmd("view.thumbs-smaller", "Smaller thumbnails", C.View, Browser, "Icon.Thumbnails", "Ctrl+-", Standard, "zoom out"),
        Cmd("view.sort-name", "Sort by name", C.View, Browser, "Icon.Sort", "", Standard, "order by"),
        Cmd("view.sort-size", "Sort by size", C.View, Browser, "Icon.Sort", "", Standard, "order by", "largest"),
        Cmd("view.sort-type", "Sort by type", C.View, Browser, "Icon.Sort", "", Standard, "order by", "kind"),
        Cmd("view.sort-modified", "Sort by date modified", C.View, Browser, "Icon.Sort", "", Standard, "order by", "newest", "recent"),
        Cmd("view.sort-created", "Sort by date created", C.View, Browser, "Icon.Sort", "", Rare, "order by"),
        Cmd("view.sort-extension", "Sort by extension", C.View, Browser, "Icon.Sort", "", Rare, "order by"),
        Cmd("view.sort-reverse", "Reverse sort order", C.View, Browser, "Icon.Sort", "", Standard, "ascending", "descending"),
        Cmd("view.add-column", "Add column…", C.View, Browser, "Icon.Columns", "", Standard, "more columns", "properties"),
        Cmd("view.columns-default", "Use these columns for new tabs", C.View, Browser, "Icon.Columns", "", Rare,
            "set as default", "save columns"),
        Cmd("view.reset-columns", "Reset columns", C.View, Browser, "Icon.Columns", "", Rare, "default columns"),

        // --- Search ---
        Cmd("search.folder", "Search this folder", C.Search, Browser, "Icon.Search", "Ctrl+F Ctrl+E", Essential,
            "find", "filter"),
        Cmd("search.pc", "Search this PC", C.Search, Browser, "Icon.Search", "Ctrl+Shift+F", Essential,
            "find everywhere", "global search", "everything"),
        Cmd("search.clear", "Clear search", C.Search, Browser, "Icon.Dismiss", "", Standard),
        Cmd("search.stop", "Stop search", C.Search, Browser, "Icon.Dismiss", "", Rare, "cancel search"),
        Cmd("search.save", "Save this search…", C.Search, Browser, "Icon.Save", "Ctrl+S", Standard, "saved search"),
        Cmd("search.syntax", "Search syntax help", C.Search, App, "Icon.Info", "", Standard,
            "query language", "operators", "regex", "wildcards"),

        // --- Tools ---
        Cmd("tools.disk-usage", "Analyse disk usage", C.Tools, Browser, "Icon.DiskUsage", "Ctrl+Shift+D", Essential,
            "treemap", "folder sizes", "space", "wiztree", "treesize"),
        Cmd("tools.duplicates", "Find duplicates", C.Tools, Browser, "Icon.Duplicates", "Ctrl+Shift+U", Essential,
            "dupes", "identical files"),
        Cmd("tools.changes", "What changed here", C.Tools, Browser, "Icon.Changes", "Ctrl+Shift+H", Standard,
            "timeline", "recent changes", "change log"),
        Cmd("tools.compare", "Compare with other pane", C.Tools, Browser, "Icon.Compare", "F7", Essential,
            "sync", "synchronise", "synchronize", "diff folders", "stop comparing"),
        Cmd("tools.compare-rescan", "Compare: rescan", C.Tools, Browser, "Icon.Refresh", "", Rare),
        Cmd("tools.compare-sync", "Compare: sync…", C.Tools, Browser, "Icon.Compare", "", Standard,
            "synchronise", "synchronize", "mirror"),
        Cmd("tools.compare-differences", "Compare: show only differences", C.Tools, Browser, "Icon.Compare", "", Standard,
            "hide identical", "filter same"),
        Cmd("tools.indexer-start", "Start the search index", C.Tools, Browser, "Icon.Indexing", "", Rare,
            "indexer", "mft"),
        Cmd("tools.indexer-retry", "Retry indexing", C.Tools, Browser, "Icon.Indexing", "", Rare, "indexer"),
        Cmd("tools.indexer-dismiss", "Dismiss the index banner", C.Tools, Browser, "Icon.Dismiss", "", Rare, "indexer"),
        Cmd("tools.transfer-pause", "Pause transfers", C.Tools, Browser, "Icon.Pause", "", Standard, "queue", "copy"),
        Cmd("tools.transfer-resume", "Resume transfers", C.Tools, Browser, "Icon.Play", "", Standard, "queue", "copy"),
        Cmd("tools.transfer-cancel", "Cancel transfer", C.Tools, Browser, "Icon.Dismiss", "", Standard, "stop copy", "abort"),
        Cmd("tools.transfer-details", "Transfer details", C.Tools, Browser, "Icon.Info", "", Standard,
            "queue", "progress", "copy"),

        // --- Preview ---
        Cmd("preview.mode-auto", "Preview as: Auto", C.Preview, Browser, "Icon.PreviewPane", "", Rare),
        Cmd("preview.mode-raw", "Preview as: Raw text", C.Preview, Browser, "Icon.PreviewPane", "", Rare, "source"),
        Cmd("preview.mode-hex", "Preview as: Hex", C.Preview, Browser, "Icon.PreviewPane", "", Rare, "binary", "bytes"),
        Cmd("preview.wrap", "Toggle word wrap", C.Preview, Browser, "Icon.PreviewPane", "", Rare),
        Cmd("preview.fit", "Toggle fit image to pane", C.Preview, Browser, "Icon.PreviewPane", "", Rare, "zoom"),
        Cmd("preview.fit-width", "Fit preview pane to content", C.Preview, Browser, "Icon.FitWidth", "", Rare),
        Cmd("preview.play", "Play / pause", C.Preview, Browser, "Icon.Play", "", Standard, "media", "video", "audio"),
        Cmd("preview.fullscreen", "Full screen video", C.Preview, Browser, "Icon.FullScreen", "", Standard, "media", "maximise"),
        Cmd("preview.play-next", "Play next video", C.Preview, Browser, "Icon.SkipNext", "", Rare, "media", "skip"),
        Cmd("preview.auto-advance", "Toggle auto-advance", C.Preview, Browser, "Icon.AutoAdvance", "", Rare,
            "media", "playlist"),
        Cmd("preview.loop", "Toggle loop", C.Preview, Browser, "Icon.LoopOne", "", Rare, "media", "repeat"),
        Cmd("preview.autoplay", "Toggle autoplay", C.Preview, Browser, "Icon.AutoPlay", "", Rare, "media"),

        // --- App ---
        // F1 as well: it is the key people press to ask "what can this do?", and the palette is
        // this app's answer to that.
        Cmd("app.palette", "Command palette", C.App, App, "Icon.Palette", "Ctrl+Shift+P F1", Essential,
            "commands", "run command", "find command", "help"),
        Cmd("app.settings", "Settings", C.App, App, "Icon.Settings", "Ctrl+,", Essential,
            "options", "preferences", "configuration"),
        Cmd("app.keyboard", "Keyboard shortcuts", C.App, App, "Icon.Keyboard", "", Essential,
            "keybindings", "key bindings", "hotkeys", "remap keys", "change shortcut"),
        Cmd("app.customise-theme", "Customise theme", C.App, App, "Icon.Appearance", "", Standard,
            "colours", "colors", "theme editor"),
        Cmd("app.match-windows-theme", "Toggle match Windows light/dark", C.App, App, "Icon.Appearance", "", Rare,
            "follow system theme", "dark mode", "light mode"),
        Cmd("app.save-workspace", "Save workspace…", C.App, Browser, "Icon.Save", "", Standard,
            "tab set", "layout", "session"),
        Cmd("app.bookmark-folder", "Bookmark this folder", C.App, Browser, "Icon.Bookmark", "Ctrl+D", Standard,
            "favourite", "favorite", "add bookmark", "remove bookmark"),
        Cmd("app.exit", "Exit", C.App, App, "Icon.Dismiss", "", Rare, "quit", "close window"),
    ];

    private static readonly Dictionary<string, CommandDescriptor> ById =
        All.ToDictionary(c => c.Id, StringComparer.Ordinal);

    public static CommandDescriptor? Find(string id) => ById.GetValueOrDefault(id);

    public static bool IsKnown(string id) => ById.ContainsKey(id);

    /// <param name="keys">The shipped shortcuts, separated by spaces; the first is the one menus
    /// and tooltips print.</param>
    private static CommandDescriptor Cmd(
        string id, string name, string category, CommandContext context, string? icon, string keys,
        CommandProminence prominence, params string[] aliases) =>
        new(id, name, category, context, icon,
            [.. keys.Split(' ', StringSplitOptions.RemoveEmptyEntries).Select(KeyChord.Parse)],
            aliases, prominence);
}
