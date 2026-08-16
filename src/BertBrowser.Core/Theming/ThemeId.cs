using System.Text;

namespace BertBrowser.Core.Theming;

/// <summary>
/// A theme's id is also its filename, so it is the one piece of a theme that can reach outside the
/// themes folder. An imported <c>*.json</c> is untrusted input — its <c>"id"</c> is whatever the
/// author typed — so an id of <c>C:\Windows\Temp\evil</c> or <c>..\..\..\something</c> would
/// otherwise become a write anywhere the user can write, rather than a file in the themes folder
/// (<see cref="System.IO.Path.Combine(string, string)"/> discards the first path entirely when the
/// second is rooted).
/// </summary>
/// <remarks>
/// Kept in Core, and deliberately not "clean the string up and carry on": a path that has to be
/// repaired is a path that was never a theme id. <see cref="IsSafe"/> decides, <see cref="Unique"/>
/// manufactures, and nothing else may build one.
/// </remarks>
public static class ThemeId
{
    /// <summary>Long enough for any real name, short enough to never approach MAX_PATH.</summary>
    public const int MaxLength = 64;

    /// <summary>Names Windows resolves to a device no matter the extension, so <c>con.json</c>
    /// opens the console rather than a file.</summary>
    private static readonly HashSet<string> ReservedNames = new(StringComparer.OrdinalIgnoreCase)
    {
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    };

    /// <summary>
    /// Whether <paramref name="id"/> is usable as a bare filename stem inside the themes folder —
    /// one path segment, no traversal, no device name, nothing Windows silently rewrites.
    /// </summary>
    public static bool IsSafe(string? id)
    {
        if (string.IsNullOrWhiteSpace(id)) return false;
        if (id.Length > MaxLength) return false;

        // Named explicitly rather than left to Path.GetInvalidFileNameChars, which is only this
        // strict on Windows — Core is plain net10.0 and the rule must not soften off-platform.
        if (id.AsSpan().IndexOfAny(@"\/:*?""<>|") >= 0) return false;
        if (id.AsSpan().IndexOfAny(Path.GetInvalidFileNameChars()) >= 0) return false;
        foreach (var c in id)
        {
            if (char.IsControl(c)) return false;
        }

        // "." and ".." carry no invalid characters but are not names.
        if (id.Trim('.').Length == 0) return false;

        // Windows strips trailing dots and spaces, so "evil " and "evil" are the same file while
        // comparing as different ids — the gap where an overwrite hides.
        if (id[0] == ' ' || id[^1] == ' ' || id[^1] == '.') return false;

        return !ReservedNames.Contains(id);
    }

    /// <summary>
    /// Turns a display name into a safe id, then suffixes it until it is unique among
    /// <paramref name="taken"/> — so "My Theme" and "my theme!" can coexist.
    /// </summary>
    public static string Unique(string? name, IEnumerable<string> taken)
    {
        var slug = new StringBuilder();
        foreach (var c in (name ?? "").Trim().ToLowerInvariant())
        {
            if (char.IsLetterOrDigit(c)) slug.Append(c);
            else if (slug.Length > 0 && slug[^1] != '-') slug.Append('-');
        }

        var basis = slug.ToString().Trim('-');
        if (basis.Length > MaxLength) basis = basis[..MaxLength].Trim('-');

        // A name that slugs to nothing, to a device name, or to a run of digits long enough to trip
        // the length rule all land here — the fallback is what keeps this total.
        if (!IsSafe(basis)) basis = "theme";

        var used = new HashSet<string>(taken, StringComparer.OrdinalIgnoreCase);
        if (!used.Contains(basis)) return basis;

        for (var i = 2; ; i++)
        {
            var candidate = $"{basis}-{i}";
            if (!used.Contains(candidate)) return candidate;
        }
    }
}
