using BertBrowser.Core.Layout;

namespace BertBrowser.Core.Services.SavedWorkspaces;

/// <summary>
/// The decisions behind a saved workspace — whether one may be saved, and what to call it by
/// default — kept pure so the dialog and the shell obey the same rules the tests pin.
/// </summary>
public static class SavedWorkspaceRules
{
    public const int MaxNameLength = 60;

    /// <summary>The first reason the workspace cannot be saved, in words for the user, or null
    /// when it can.</summary>
    /// <param name="nameTaken">Whether another saved workspace already has this (trimmed) name.
    /// The caller excludes the workspace being edited, so keeping its own name is not a
    /// clash.</param>
    public static string? Validate(string name, Func<string, bool> nameTaken)
    {
        var trimmed = name.Trim();
        if (trimmed.Length == 0) return "Give the workspace a name.";
        if (trimmed.Length > MaxNameLength) return $"Keep the name under {MaxNameLength} characters.";
        if (nameTaken(trimmed)) return $"There is already a workspace called \"{trimmed}\".";
        return null;
    }

    /// <summary>The name a new workspace starts with. There is no query or path to summarize the
    /// way a saved search's default comes from what was typed, so the default is simply when it
    /// was saved — the user renames if they want something else.</summary>
    public static string DefaultName(DateTime now) => $"Workspace {now:yyyy-MM-dd HH:mm}";

    /// <summary>"2 panes, 5 tabs" — the one-line shape of a layout, for the sidebar's tooltip and
    /// the Settings list alike.</summary>
    public static string ShapeText(SessionLayout layout)
    {
        var panes = SessionLayoutRules.CountPanes(layout);
        var tabs = SessionLayoutRules.Panes(layout).Sum(p => p.Tabs?.Count ?? 0);
        return $"{panes} pane{(panes == 1 ? "" : "s")}, {tabs} tab{(tabs == 1 ? "" : "s")}";
    }

    /// <summary>Every folder the layout opens, in pane then tab order, each once (ignoring case) —
    /// two panes on one folder is one folder to the person reading the list.</summary>
    public static IReadOnlyList<string> Folders(SessionLayout layout)
    {
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var folders = new List<string>();
        foreach (var pane in SessionLayoutRules.Panes(layout))
        foreach (var tab in pane.Tabs ?? [])
        {
            if (tab.Path.Length > 0 && seen.Add(tab.Path)) folders.Add(tab.Path);
        }
        return folders;
    }
}
