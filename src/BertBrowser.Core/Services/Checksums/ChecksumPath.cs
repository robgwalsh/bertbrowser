using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Checksums;

/// <summary>
/// Turning a name listed inside a checksum file into a path on disk, or refusing to.
/// </summary>
/// <remarks>
/// <para>
/// <strong>A checksum file is untrusted input.</strong> It arrives beside a download, from whoever
/// made the download, and every line in it is a string that is about to be turned into a path and
/// opened. A line reading <c>..\..\..\Windows\System32\config\SAM</c> must produce a visible
/// refusal, not a read — the verify report has a row for it, so a hostile file is something the
/// user can see rather than something that quietly happened.
/// </para>
/// <para>
/// The same instinct as the rule that a virtual archive path must never reach a
/// <see cref="PathKey"/>-keyed table: a string that is syntactically a path is not thereby a path
/// this code may use.
/// </para>
/// </remarks>
public static class ChecksumPath
{
    /// <summary>
    /// The full path <paramref name="name"/> refers to under <paramref name="folder"/>, or null if
    /// it must not be resolved at all.
    /// </summary>
    /// <returns>
    /// Null for a refusal, never a throw — a bad line is one row of a report, and it must not cost
    /// the other three hundred lines their answer.
    /// </returns>
    public static string? Resolve(string folder, string name)
    {
        if (folder.Length == 0 || name.Length == 0) return null;

        // sha256sum files are written on Linux as often as not.
        var relative = name.Replace('/', '\\').Trim();
        if (relative.Length == 0) return null;

        // Rooted in any of its several spellings: "C:\x", "\x", "\\server\share\x", "C:x".
        if (Path.IsPathRooted(relative)) return null;
        if (relative.Length >= 2 && relative[1] == ':') return null;

        // The device and long-path prefixes, which sidestep normalisation entirely.
        if (relative.StartsWith(@"\\", StringComparison.Ordinal)) return null;

        if (relative.IndexOfAny(InvalidChars) >= 0 || relative.Contains('\0')) return null;

        foreach (var segment in relative.Split('\\', StringSplitOptions.RemoveEmptyEntries))
        {
            if (segment is ".." or ".") return null;
        }

        string combined;
        try
        {
            combined = Path.GetFullPath(Path.Combine(folder, relative));
        }
        catch (Exception ex) when (ex is ArgumentException or PathTooLongException or NotSupportedException)
        {
            return null;
        }

        // Belt and braces. Everything above is a syntactic rule about the listed name; this is the
        // one check on the answer, and it is the one that would still hold if a spelling of "up one
        // level" were invented tomorrow.
        try
        {
            var key = PathKey.Canonicalize(combined);
            var root = PathKey.Canonicalize(folder);
            return PathKey.IsUnder(key, root) ? combined : null;
        }
        catch (ArgumentException)
        {
            return null;
        }
    }

    /// <summary>
    /// How a path under <paramref name="folder"/> is written into a checksum file: relative, with
    /// forward slashes, which is what every other tool reading one expects.
    /// </summary>
    public static string? Relativise(string folder, string fullPath)
    {
        try
        {
            var relative = Path.GetRelativePath(folder, fullPath);
            if (relative.Length == 0 || relative == "." || Path.IsPathRooted(relative)) return null;
            if (relative.StartsWith("..", StringComparison.Ordinal)) return null;

            return relative.Replace('\\', '/');
        }
        catch (ArgumentException)
        {
            return null;
        }
    }

    private static readonly char[] InvalidChars = ['<', '>', '"', '|', '?', '*'];
}
