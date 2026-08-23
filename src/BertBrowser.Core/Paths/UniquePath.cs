namespace BertBrowser.Core.Paths;

/// <summary>
/// The "name (2)" rule: given a path something already occupies, the next free one beside it.
/// </summary>
/// <remarks>
/// One copy, because three parts of the app need the same answer and a user who sees
/// "Report (2).txt" from a paste should see "Report (2).txt" from a New File too. The transfer and
/// delete executors ask it about staging and displaced entries; the new-item dialog asks it for the
/// name it suggests. Existence is passed in rather than looked up so the callers' probes — and the
/// tests' fakes — stay in charge of what is there.
/// </remarks>
public static class UniquePath
{
    /// <summary>The first free path at or beside <paramref name="path"/>. Directories number the
    /// whole name; files number before the extension, so "a.txt" becomes "a (2).txt".</summary>
    /// <param name="isDirectory">Whether the item being placed is a folder — which is what decides
    /// where the number goes. It is a parameter rather than something probed from
    /// <paramref name="path"/> because a caller placing something <em>new</em> there would
    /// otherwise be told about whatever is in the way instead: a folder named "notes.txt" blocking
    /// a file named "notes.txt" would produce "notes.txt (2)" rather than "notes (2).txt".</param>
    public static string For(
        string path,
        bool isDirectory,
        Func<string, bool> directoryExists,
        Func<string, bool> fileExists)
    {
        bool Exists(string candidate) => fileExists(candidate) || directoryExists(candidate);

        if (!Exists(path)) return path;

        var directory = Path.GetDirectoryName(path) ?? "";
        var name = Path.GetFileName(path);
        var stem = isDirectory ? name : Path.GetFileNameWithoutExtension(name);
        var extension = isDirectory ? "" : Path.GetExtension(name);

        for (var i = 2; ; i++)
        {
            var candidate = Path.Combine(directory, $"{stem} ({i}){extension}");
            if (!Exists(candidate)) return candidate;
        }
    }

    /// <summary>The same rule against the real filesystem, for an item already at
    /// <paramref name="path"/>.</summary>
    public static string For(string path) =>
        For(path, Directory.Exists(path), Directory.Exists, File.Exists);
}
