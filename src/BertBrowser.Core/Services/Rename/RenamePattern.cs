using System.Buffers;

namespace BertBrowser.Core.Services.Rename;

/// <summary>
/// Turns what the user typed into the name each selected item gets. One item takes the typed text
/// as its whole name, extension included; several items are numbered — "Holiday" over three photos
/// gives "Holiday 1.jpg", "Holiday 2.png", "Holiday 3.jpg", each keeping its own extension.
/// </summary>
/// <remarks>
/// Pure, and separate from <see cref="RenamePlanner"/>, because the dialog previews the result of
/// every keystroke: the naming rule and the "is this a legal Windows name" rule have to be the same
/// ones the rename itself will use, not a re-implementation that can drift.
/// </remarks>
public static class RenamePattern
{
    /// <summary>NTFS's per-component limit.</summary>
    public const int MaxNameLength = 255;

    /// <summary>Names Windows still refuses to give a file, with or without an extension.</summary>
    private static readonly HashSet<string> ReservedNames = new(StringComparer.OrdinalIgnoreCase)
    {
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    };

    /// <summary>The names <paramref name="pattern"/> gives <paramref name="sources"/>, one per
    /// source and in the same order. Never throws: an unusable pattern still produces names, and
    /// <see cref="Validate"/> is what rejects them.</summary>
    public static IReadOnlyList<string> Apply(IReadOnlyList<RenameSource> sources, string pattern)
    {
        var text = Clean(pattern);
        if (sources.Count == 0) return [];
        if (sources.Count == 1) return [text];

        var names = new string[sources.Count];
        for (var i = 0; i < sources.Count; i++)
            names[i] = text + " " + (i + 1) + Extension(sources[i]);
        return names;
    }

    /// <summary>What the dialog starts with: the one item's whole name, or — for a selection — the
    /// first item's name without its extension, which is the part a numbered rename replaces.</summary>
    public static string SuggestFor(IReadOnlyList<RenameSource> sources)
    {
        if (sources.Count == 0) return "";
        var first = sources[0];
        var name = System.IO.Path.GetFileName(first.Path);
        if (sources.Count == 1) return name;
        return first.IsDirectory ? name : System.IO.Path.GetFileNameWithoutExtension(name);
    }

    /// <summary>The length of the part of a single item's name that a rename usually replaces, so
    /// the dialog can pre-select it and leave the extension alone.</summary>
    public static int BaseNameLength(RenameSource source)
    {
        var name = System.IO.Path.GetFileName(source.Path);
        if (source.IsDirectory) return name.Length;
        var stem = System.IO.Path.GetFileNameWithoutExtension(name);
        // A dotfile such as .gitignore is all extension as far as Path is concerned; select the lot.
        return stem.Length == 0 ? name.Length : stem.Length;
    }

    /// <summary>Why <paramref name="name"/> can't be a file name, or null when it can be.</summary>
    public static string? Validate(string name)
    {
        if (string.IsNullOrWhiteSpace(name))
            return "Enter a name.";

        if (name.Length > MaxNameLength)
            return $"A name can be at most {MaxNameLength} characters.";

        foreach (var c in name)
        {
            if (char.IsControl(c))
                return "A name can't contain control characters.";
            if (InvalidChars.Contains(c))
                return "A name can't contain any of  \\ / : * ? \" < > |";
        }

        if (name[^1] is '.' or ' ')
            return "A name can't end with a space or a period.";

        var dot = name.IndexOf('.');
        var stem = dot < 0 ? name : name[..dot];
        if (ReservedNames.Contains(stem))
            return $"'{stem}' is a name Windows reserves for a device.";

        return null;
    }

    /// <summary>Cached because <see cref="System.IO.Path.GetInvalidFileNameChars"/> hands out a
    /// fresh copy on every call, and validation runs on every keystroke.</summary>
    private static readonly SearchValues<char> InvalidChars =
        SearchValues.Create(System.IO.Path.GetInvalidFileNameChars());

    /// <summary>Trailing spaces and periods are silently dropped by Windows, so a name that only
    /// differs by them is not a rename at all; take them off before anything sees the name.</summary>
    private static string Clean(string pattern) => pattern.Trim().TrimEnd('.', ' ');

    private static string Extension(RenameSource source) =>
        source.IsDirectory ? "" : System.IO.Path.GetExtension(source.Path);
}
