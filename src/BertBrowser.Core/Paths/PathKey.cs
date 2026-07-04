namespace BertBrowser.Core.Paths;

/// <summary>
/// Canonical path keys for database storage. Windows paths are case-insensitive,
/// but SQLite's NOCASE collation only folds ASCII, so we normalize case here in C#
/// and let SQLite compare with plain BINARY collation.
/// </summary>
public static class PathKey
{
    /// <summary>
    /// Canonicalizes a path into the form used as a database key:
    /// fully qualified, '\' separators, no trailing separator (except drive roots
    /// like "C:\"), uppercased invariantly.
    /// </summary>
    public static string Canonicalize(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("Path must be non-empty.", nameof(path));

        var full = Path.GetFullPath(path);
        full = TrimTrailingSeparator(full);
        return full.ToUpperInvariant();
    }

    /// <summary>
    /// Same normalization as <see cref="Canonicalize"/> but preserving the original
    /// casing, for display.
    /// </summary>
    public static string NormalizeDisplay(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("Path must be non-empty.", nameof(path));

        return TrimTrailingSeparator(Path.GetFullPath(path));
    }

    private static string TrimTrailingSeparator(string full)
    {
        // Path.GetFullPath already normalized '/' to '\'. Keep the trailing
        // separator only when it is the root itself ("C:\", "\\server\share\").
        if (full.Length > 0 && (full[^1] == '\\' || full[^1] == '/'))
        {
            var root = Path.GetPathRoot(full);
            if (root is null || full.Length > root.Length)
                full = full.TrimEnd('\\', '/');
        }
        return full;
    }

    /// <summary>
    /// Returns the half-open range [lo, hi) such that a canonical key k is strictly
    /// inside the directory (at any depth) iff lo &lt;= k &lt; hi under ordinal (BINARY)
    /// comparison. lo = dir + "\", hi = dir + "]" because ']' (U+005D) is the
    /// character immediately after '\' (U+005C). This turns recursive subtree
    /// queries into pure index range scans, with no LIKE-escaping concerns.
    /// </summary>
    public static (string Lo, string Hi) PrefixBounds(string dirKey)
    {
        if (string.IsNullOrWhiteSpace(dirKey))
            throw new ArgumentException("Directory key must be non-empty.", nameof(dirKey));

        // Accept either a canonical key or a raw path; re-canonicalize is cheap and safe.
        var key = Canonicalize(dirKey);

        // Drive roots canonicalize to "C:\" (trailing separator retained), so avoid
        // doubling the separator: lo must end with exactly one '\'.
        var body = key[^1] == '\\' ? key[..^1] : key;
        return (body + '\\', body + ']');
    }

    /// <summary>True if <paramref name="key"/> is strictly under <paramref name="dirKey"/>.</summary>
    public static bool IsUnder(string key, string dirKey)
    {
        var (lo, hi) = PrefixBounds(dirKey);
        return string.CompareOrdinal(key, lo) >= 0 && string.CompareOrdinal(key, hi) < 0;
    }
}
