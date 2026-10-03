namespace BertBrowser.Core.Services.Commands;

/// <summary>What a typed path came to, before anyone has looked at the disk.</summary>
/// <param name="Path">The path with <c>~</c> and <c>%VARIABLES%</c> filled in.</param>
/// <param name="Folder">The folder to list for completions: the path itself when it ends in a
/// separator, otherwise the folder above its last segment.</param>
/// <param name="Prefix">The start of a name being typed inside <see cref="Folder"/>, or empty.</param>
public readonly record struct GoToTarget(string Path, string Folder, string Prefix);

/// <summary>
/// Recognises a path typed into the palette, so the palette can offer to go there. This is the
/// app's command bar: <c>C:\Windows</c>, <c>~\Downloads</c>, <c>%TEMP%</c> and <c>\\server\share</c>
/// are destinations rather than searches.
/// </summary>
/// <remarks>
/// Pure: nothing here touches the disk or the environment — both are handed in — so what counts as
/// "looks like a path" is a tested rule rather than whatever <c>Directory.Exists</c> happened to
/// say about a half-typed word. Whether the place is real is the caller's question, asked once
/// this has said the text is worth asking about.
/// </remarks>
public static class GoToRules
{
    /// <summary>
    /// The path this text names, or null when it is not one — in which case it is a search for a
    /// command, as everything else typed into the palette is.
    /// </summary>
    /// <param name="variable">Looks an environment variable up by name; null when there is none.</param>
    /// <param name="home">The user's profile folder, which <c>~</c> stands for.</param>
    public static GoToTarget? Parse(string? text, Func<string, string?> variable, string home)
    {
        var typed = (text ?? "").Trim().Trim('"');
        if (typed.Length == 0) return null;

        var expanded = Expand(typed, variable, home);
        if (expanded is null || !LooksRooted(expanded)) return null;

        // A bare drive is its root: "c:" on its own means the top of C, not whatever folder the
        // process happens to be sitting in on that drive.
        if (expanded.Length == 2) expanded += "\\";
        expanded = expanded.Replace('/', '\\');

        if (expanded.EndsWith('\\')) return new GoToTarget(expanded, expanded, "");

        var cut = expanded.LastIndexOf('\\');
        return cut < 0
            ? new GoToTarget(expanded, expanded, "")
            : new GoToTarget(expanded, expanded[..(cut + 1)], expanded[(cut + 1)..]);
    }

    /// <summary>Fills in a leading <c>~</c> and any <c>%NAME%</c>. Null when a variable is named
    /// that does not exist, since a path built on nothing leads nowhere.</summary>
    private static string? Expand(string typed, Func<string, string?> variable, string home)
    {
        if (typed == "~") return home;
        if (typed.StartsWith("~\\", StringComparison.Ordinal) || typed.StartsWith("~/", StringComparison.Ordinal))
            typed = home.TrimEnd('\\') + typed[1..];

        var at = 0;
        while (true)
        {
            var open = typed.IndexOf('%', at);
            if (open < 0) return typed;
            var close = typed.IndexOf('%', open + 1);
            // An unclosed % is still being typed: not a path yet, and not worth a refusal.
            if (close < 0) return null;

            var name = typed[(open + 1)..close];
            if (name.Length == 0 || variable(name) is not { Length: > 0 } value) return null;

            typed = typed[..open] + value + typed[(close + 1)..];
            at = open + value.Length;
        }
    }

    /// <summary>A drive letter and a colon, or the two slashes of a share.</summary>
    private static bool LooksRooted(string path) =>
        (path.Length >= 2 && char.IsAsciiLetter(path[0]) && path[1] == ':' &&
         (path.Length == 2 || path[2] is '\\' or '/')) ||
        path.StartsWith(@"\\", StringComparison.Ordinal);
}
