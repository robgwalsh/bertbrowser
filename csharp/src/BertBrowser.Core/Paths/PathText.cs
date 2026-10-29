namespace BertBrowser.Core.Paths;

/// <summary>
/// Turning paths into text a person is about to paste somewhere else.
/// </summary>
/// <remarks>
/// Pure and separate from <see cref="PathKey"/> because it is the opposite job: PathKey normalises
/// a path so the database can compare it, this leaves a path exactly as it is and only decides how
/// to wrap it.
/// </remarks>
public static class PathText
{
    /// <summary>
    /// One path, wrapped for pasting into a command line.
    /// </summary>
    /// <remarks>
    /// Quoted <em>unconditionally</em>, which is what Explorer's own "Copy as path" does. Quoting
    /// only when a path happens to contain a space would be tidier to read and worse to use: the
    /// result would sometimes survive being pasted into a shell and sometimes not, depending on
    /// where the file lived, and the failure would arrive later as a mangled command rather than
    /// here. A command named after Explorer's should also behave like it.
    /// </remarks>
    public static string Quote(string path) => $"\"{path}\"";

    /// <summary>Several paths, one per line, each quoted — what "Copy as path" puts on the
    /// clipboard for a multiple selection.</summary>
    public static string ForClipboard(IEnumerable<string> paths)
    {
        ArgumentNullException.ThrowIfNull(paths);
        return string.Join(Environment.NewLine, paths.Select(Quote));
    }

    /// <summary>Several names, one per line, unquoted — a name is not a path and is far more often
    /// wanted as bare text.</summary>
    public static string NamesForClipboard(IEnumerable<string> paths)
    {
        ArgumentNullException.ThrowIfNull(paths);
        return string.Join(Environment.NewLine, paths.Select(Path.GetFileName));
    }
}
