namespace BertBrowser.Core.Services.ShellMenu;

/// <summary>Which of the app's right-click menus a built-in entry is part of.</summary>
[Flags]
public enum BuiltInMenuPlaces
{
    FileList = 1,
    FolderTree = 2,
    Both = FileList | FolderTree,
}

/// <summary>
/// One of the app's own right-click entries, as the Context menu page lists it. The id is what
/// the hidden list stores and what the XAML tags the item with; the name is the menu's wording
/// without the ellipsis and without the counts a selection adds.
/// </summary>
public sealed record BuiltInMenuItem(string Id, string Name, BuiltInMenuPlaces Places)
{
    public bool IsIn(BuiltInMenuPlaces place) => (Places & place) != 0;
}

/// <summary>
/// The app's own right-click entries, in the file list's menu order with the tree-only rows where
/// the tree has them. One row per verb, not per menu: "Open in new tab" is one thing wherever it is
/// offered, so unticking it takes it out of both menus — the way unticking 7-Zip does.
/// </summary>
public static class BuiltInMenuItems
{
    public static IReadOnlyList<BuiltInMenuItem> All { get; } =
    [
        new("new", "New", BuiltInMenuPlaces.Both),
        new("open", "Open", BuiltInMenuPlaces.FileList),
        new("run-as-admin", "Run as administrator", BuiltInMenuPlaces.FileList),
        new("open-new-tab", "Open in new tab", BuiltInMenuPlaces.Both),
        new("open-new-pane", "Open in new pane", BuiltInMenuPlaces.Both),
        new("open-terminal", "Open in Terminal", BuiltInMenuPlaces.Both),
        new("open-vscode", "Open in VS Code", BuiltInMenuPlaces.Both),
        new("disk-usage", "Analyse disk usage", BuiltInMenuPlaces.Both),
        new("duplicates", "Find duplicates", BuiltInMenuPlaces.Both),
        new("changes", "What changed here", BuiltInMenuPlaces.Both),
        new("compare-panes", "Compare with other pane", BuiltInMenuPlaces.FileList),
        new("compress", "Compress", BuiltInMenuPlaces.FileList),
        new("extract", "Extract here / Extract to", BuiltInMenuPlaces.FileList),
        new("copy-path", "Copy as path", BuiltInMenuPlaces.Both),
        new("copy-name", "Copy name", BuiltInMenuPlaces.FileList),
        new("compare-files", "Compare these two files", BuiltInMenuPlaces.FileList),
        new("settle-by-content", "Settle by content", BuiltInMenuPlaces.FileList),
        new("verify-checksums", "Verify checksums", BuiltInMenuPlaces.FileList),
        new("checksum", "Checksum", BuiltInMenuPlaces.FileList),
        new("cut", "Cut", BuiltInMenuPlaces.FileList),
        new("copy", "Copy", BuiltInMenuPlaces.FileList),
        new("paste", "Paste", BuiltInMenuPlaces.FileList),
        new("rename", "Rename", BuiltInMenuPlaces.FileList),
        new("delete", "Delete", BuiltInMenuPlaces.Both),
        new("delete-permanently", "Delete permanently", BuiltInMenuPlaces.FileList),
        new("bookmark", "Bookmark", BuiltInMenuPlaces.Both),
        new("properties", "Properties", BuiltInMenuPlaces.Both),
    ];

    private static readonly Dictionary<string, BuiltInMenuItem> ById =
        All.ToDictionary(i => i.Id, StringComparer.OrdinalIgnoreCase);

    public static bool IsKnown(string id) => ById.ContainsKey(id);

    /// <summary>Whether the user has unticked this entry. An id nothing tags is a bug in the
    /// menu, not a preference, and says so rather than quietly showing or hiding.</summary>
    public static bool IsShown(string id, IReadOnlySet<string> hiddenIds)
    {
        if (!IsKnown(id))
            throw new ArgumentException($"'{id}' is not one of the app's menu entries.", nameof(id));
        return !hiddenIds.Contains(id);
    }

    /// <summary>The wording for where a row appears, beside its name on the Settings page.</summary>
    public static string PlacesText(BuiltInMenuPlaces places) => places switch
    {
        BuiltInMenuPlaces.FileList => "file list",
        BuiltInMenuPlaces.FolderTree => "folder tree",
        _ => "file list and tree",
    };
}
