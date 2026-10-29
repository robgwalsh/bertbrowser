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
/// without the ellipsis and without the counts a selection adds. <see cref="Icon"/> (an
/// <c>Icon.*</c> resource name) is what the menu shows beside it, so the Settings page can draw
/// its preview of the menu the way the menu draws itself.
/// </summary>
/// <param name="CommandId">
/// The <c>CommandCatalog</c> command this entry runs, whose shortcut the menu prints beside it —
/// read from the keymap in force, never written here, so rebinding a key changes the menu too.
/// Null for a row that is a group rather than one command: New and "Open in new pane" are
/// submenus, and Extract is two items under one id.
/// </param>
public sealed record BuiltInMenuItem(
    string Id, string Name, BuiltInMenuPlaces Places, string? Icon = null, string? CommandId = null)
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
        new("new", "New", BuiltInMenuPlaces.Both, "Icon.Add"),
        new("open", "Open", BuiltInMenuPlaces.FileList, "Icon.Open", "file.open"),
        new("run-as-admin", "Run as administrator", BuiltInMenuPlaces.FileList, "Icon.Shield", "file.run-as-admin"),
        new("open-new-tab", "Open in new tab", BuiltInMenuPlaces.Both, "Icon.OpenInNewTab", "file.open-new-tab"),
        new("open-new-pane", "Open in new pane", BuiltInMenuPlaces.Both, "Icon.OpenInNewPane"),
        new("open-terminal", "Open in Terminal", BuiltInMenuPlaces.Both, "Icon.Terminal", "file.open-terminal"),
        new("open-vscode", "Open in VS Code", BuiltInMenuPlaces.Both, "Icon.VSCode", "file.open-vscode"),
        new("disk-usage", "Analyse disk usage", BuiltInMenuPlaces.Both, "Icon.DiskUsage", "tools.disk-usage"),
        new("duplicates", "Find duplicates", BuiltInMenuPlaces.Both, "Icon.Duplicates", "tools.duplicates"),
        new("changes", "What changed here", BuiltInMenuPlaces.Both, "Icon.Changes", "tools.changes"),
        new("compare-panes", "Compare with other pane", BuiltInMenuPlaces.FileList, "Icon.Compare", "tools.compare"),
        new("compress", "Compress", BuiltInMenuPlaces.FileList, "Icon.Archive", "file.compress"),
        new("extract", "Extract here / Extract to", BuiltInMenuPlaces.FileList, "Icon.MoveToFolder"),
        new("copy-path", "Copy as path", BuiltInMenuPlaces.Both, "Icon.Copy", "edit.copy-path"),
        new("copy-name", "Copy name", BuiltInMenuPlaces.FileList, CommandId: "edit.copy-name"),
        new("compare-files", "Compare these two files", BuiltInMenuPlaces.FileList, "Icon.CompareFiles", "file.compare-files"),
        new("settle-by-content", "Settle by content", BuiltInMenuPlaces.FileList, "Icon.CompareFiles", "file.settle-by-content"),
        new("verify-checksums", "Verify checksums", BuiltInMenuPlaces.FileList, "Icon.Checksum", "file.verify-checksums"),
        new("checksum", "Checksum", BuiltInMenuPlaces.FileList, "Icon.Checksum", "file.checksum"),
        new("cut", "Cut", BuiltInMenuPlaces.FileList, "Icon.Cut", "edit.cut"),
        new("copy", "Copy", BuiltInMenuPlaces.FileList, "Icon.Copy", "edit.copy"),
        new("paste", "Paste", BuiltInMenuPlaces.FileList, "Icon.Paste", "edit.paste"),
        new("rename", "Rename", BuiltInMenuPlaces.FileList, "Icon.Rename", "file.rename"),
        new("delete", "Delete", BuiltInMenuPlaces.Both, "Icon.Delete", "file.delete"),
        new("delete-permanently", "Delete permanently", BuiltInMenuPlaces.FileList, "Icon.DeletePermanent", "file.delete-permanently"),
        new("bookmark", "Bookmark", BuiltInMenuPlaces.Both, "Icon.Bookmark", "file.bookmark"),
        new("properties", "Properties", BuiltInMenuPlaces.Both, "Icon.Properties", "file.properties"),
    ];

    private static readonly Dictionary<string, BuiltInMenuItem> ById =
        All.ToDictionary(i => i.Id, StringComparer.OrdinalIgnoreCase);

    public static bool IsKnown(string id) => ById.ContainsKey(id);

    public static BuiltInMenuItem? Find(string id) => ById.GetValueOrDefault(id);

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
