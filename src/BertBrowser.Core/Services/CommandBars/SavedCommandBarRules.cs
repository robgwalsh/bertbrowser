using BertBrowser.Core.Services.SavedWorkspaces;

namespace BertBrowser.Core.Services.CommandBars;

/// <summary>
/// The decisions behind saved menu-and-toolbar arrangements: whether one may be saved under a
/// name, and what saving, renaming and removing do to the list. They behave as saved workspaces
/// do — a name is one arrangement whatever its case, and saving under a name already there
/// replaces it — but live in the settings file, so the list operations are here rather than in SQL.
/// </summary>
public static class SavedCommandBarRules
{
    public const int MaxNameLength = SavedWorkspaceRules.MaxNameLength;

    /// <summary>The first reason the arrangement cannot be saved under this name, in words for the
    /// user, or null when it can.</summary>
    /// <param name="nameTaken">Whether another saved arrangement has this (trimmed) name. The
    /// caller excludes the one being renamed.</param>
    public static string? Validate(string name, Func<string, bool> nameTaken)
    {
        var trimmed = name.Trim();
        if (trimmed.Length == 0) return "Give the arrangement a name.";
        if (trimmed.Length > MaxNameLength) return $"Keep the name under {MaxNameLength} characters.";
        if (nameTaken(trimmed)) return $"There is already an arrangement called \"{trimmed}\".";
        return null;
    }

    public static string DefaultName(DateTime now) => $"Arrangement {now:yyyy-MM-dd HH:mm}";

    public static SavedCommandBarSet? Find(IEnumerable<SavedCommandBarSet> saved, string name) =>
        saved.FirstOrDefault(s => Is(s, name));

    /// <summary>The list with this arrangement saved: in place of one of the same name, taking the
    /// casing just typed, or added. Always in name order.</summary>
    public static IReadOnlyList<SavedCommandBarSet> Upsert(
        IEnumerable<SavedCommandBarSet> saved, SavedCommandBarSet arrangement)
    {
        var named = arrangement with { Name = arrangement.Name.Trim() };
        return Sorted(saved.Where(s => !Is(s, named.Name)).Append(named));
    }

    /// <summary>The list with one arrangement renamed, or null when the new name belongs to
    /// another or there is nothing by the old one. A change of case alone is a rename.</summary>
    public static IReadOnlyList<SavedCommandBarSet>? Rename(
        IReadOnlyList<SavedCommandBarSet> saved, string oldName, string newName)
    {
        var trimmed = newName.Trim();
        if (Find(saved, oldName) is not { } target) return null;
        if (saved.Any(s => !ReferenceEquals(s, target) && Is(s, trimmed))) return null;

        return Sorted(saved.Select(s => ReferenceEquals(s, target) ? s with { Name = trimmed } : s));
    }

    public static IReadOnlyList<SavedCommandBarSet> Remove(IEnumerable<SavedCommandBarSet> saved, string name) =>
        [.. saved.Where(s => !Is(s, name))];

    /// <summary>The name of the saved arrangement this one is, or null when it is none of them —
    /// which is how the page knows to say "(modified)" and whether a recall would lose work.</summary>
    public static string? NameMatching(IEnumerable<SavedCommandBarSet> saved, CommandBarSet current) =>
        saved.FirstOrDefault(s => CommandBarRules.Same(s.Set, current))?.Name;

    /// <summary>Whether replacing the current arrangement would throw away something that exists
    /// nowhere else: it is neither the default nor any saved one.</summary>
    public static bool NeedsConfirmBeforeReplace(CommandBarSet current, IEnumerable<SavedCommandBarSet> saved) =>
        !CommandBarRules.IsDefault(current) && NameMatching(saved, current) is null;

    /// <summary>"6 menus, 2 toolbars" — the one-line shape, for the Settings list.</summary>
    public static string ShapeText(CommandBarSet set)
    {
        var menus = set.Menus.Count;
        var toolbars = 1 + set.Rows.Count;
        return $"{menus} menu{(menus == 1 ? "" : "s")}, {toolbars} toolbar{(toolbars == 1 ? "" : "s")}";
    }

    private static bool Is(SavedCommandBarSet saved, string name) =>
        string.Equals(saved.Name, name.Trim(), StringComparison.OrdinalIgnoreCase);

    private static IReadOnlyList<SavedCommandBarSet> Sorted(IEnumerable<SavedCommandBarSet> saved) =>
        [.. saved.OrderBy(s => s.Name, StringComparer.OrdinalIgnoreCase)];
}
