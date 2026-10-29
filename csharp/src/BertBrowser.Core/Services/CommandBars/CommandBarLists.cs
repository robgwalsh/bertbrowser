namespace BertBrowser.Core.Services.CommandBars;

/// <summary>A run of menu rows that comes from data rather than from the catalogue.</summary>
/// <param name="Name">What the Settings page calls it.</param>
/// <param name="EmptyText">The greyed row shown when there is nothing to list, or null when an
/// empty list simply shows nothing — which lets the submenu around it disappear.</param>
public sealed record CommandBarList(string Id, string Name, string Icon, string? EmptyText);

/// <summary>Every list a menu or toolbar can hold. What each one expands to is the App's business;
/// this is the catalogue the editor offers and the rules consult.</summary>
public static class CommandBarLists
{
    public const string Workspaces = "workspaces";
    public const string SavedSearches = "saved-searches";
    public const string Bookmarks = "bookmarks";
    public const string NewFileTypes = "new-file-types";
    public const string CustomCommands = "custom-commands";
    public const string CommandBars = "command-bars";
    public const string Drives = "drives";
    public const string UndoSteps = "undo-steps";
    public const string RedoSteps = "redo-steps";

    public static IReadOnlyList<CommandBarList> All { get; } =
    [
        new(Workspaces, "Saved workspaces", "Icon.Tab", "No saved workspaces"),
        new(SavedSearches, "Saved searches", "Icon.Search", "No saved searches"),
        new(Bookmarks, "Bookmarks", "Icon.Bookmark", "No bookmarks"),
        new(NewFileTypes, "New file types", "Icon.NewFile", null),
        new(CustomCommands, "Your commands", "Icon.CustomCommand", null),
        new(CommandBars, "Saved menu and toolbar arrangements", "Icon.Reorder", "No saved arrangements"),
        new(Drives, "Drives", "Icon.Drive", null),
        new(UndoSteps, "Steps to undo", "Icon.Undo", "Nothing to undo"),
        new(RedoSteps, "Steps to redo", "Icon.Redo", "Nothing to redo"),
    ];

    public static CommandBarList? Find(string? id) => All.FirstOrDefault(l => l.Id == id);
}
