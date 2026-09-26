namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>The few phrases every record's description is built from.</summary>
public static class UndoText
{
    public static string Items(int count) => count == 1 ? "1 item" : $"{count:N0} items";

    /// <summary>A folder by its own name, or by its whole path when it is a drive root.</summary>
    public static string FolderName(string path)
    {
        var trimmed = Path.TrimEndingDirectorySeparator(path);
        var name = Path.GetFileName(trimmed);
        return name.Length > 0 ? name : path;
    }

    /// <summary>One item by name in quotes, several by count.</summary>
    public static string Named(IReadOnlyList<string> paths) =>
        paths.Count == 1 ? $"'{Path.GetFileName(Path.TrimEndingDirectorySeparator(paths[0]))}'" : Items(paths.Count);

    /// <summary>"Move 3 items to Documents" as the object of "undo" — its first letter lowered,
    /// so it reads as one sentence in the status bar.</summary>
    public static string AsObject(string description) =>
        description.Length == 0 ? description : char.ToLowerInvariant(description[0]) + description[1..];
}
