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
}
