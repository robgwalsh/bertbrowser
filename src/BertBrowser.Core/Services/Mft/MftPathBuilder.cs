namespace BertBrowser.Core.Services.Mft;

/// <summary>One MFT entry as it lives in the in-memory reference map: its own name,
/// its parent's file reference number, and the two attribute bits we care about.
/// <see cref="OwnHidden"/> is the entry's own Hidden bit; effective (inherited) hidden
/// is computed by <see cref="MftPathBuilder"/> while walking to the volume root.</summary>
internal readonly record struct MftNode(string Name, ulong ParentFrn, bool IsDirectory, bool OwnHidden);

/// <summary>
/// Reconstructs full paths from the flat FRN→node map that an MFT enumeration produces.
/// The USN/MFT records only give each entry its own name plus its parent's reference
/// number, so a path is the chain of names from the entry up to the NTFS root (FRN 5).
///
/// Directory resolutions are memoized in a caller-supplied cache so a bulk pass over a
/// whole volume is ~O(entries) rather than O(entries × depth): resolving one deep entry
/// caches every ancestor directory along the way, and later siblings short-circuit at
/// their parent. Effective hidden mirrors <c>FileSystemWalker</c> — an entry is hidden if
/// it or any ancestor below the root carries the Hidden attribute.
/// </summary>
internal static class MftPathBuilder
{
    private const int MaxDepth = 512; // guards against cycles from a corrupt/racing table

    /// <summary>
    /// Resolves the display path and effective-hidden state of <paramref name="frn"/>.
    /// Returns false when the parent chain is broken (a missing link — common for entries
    /// whose ancestor was deleted mid-enumeration) or exceeds <see cref="MaxDepth"/>.
    /// <paramref name="dirCache"/> is read and populated for directory ancestors.
    /// </summary>
    public static bool TryResolve(
        IReadOnlyDictionary<ulong, MftNode> map,
        ulong frn,
        string driveRoot,
        Dictionary<ulong, (string Path, bool Hidden)> dirCache,
        out string displayPath,
        out bool hidden)
    {
        displayPath = "";
        hidden = false;

        if (frn == NtfsRoot)
        {
            displayPath = driveRoot;
            return true;
        }
        if (!map.TryGetValue(frn, out var node))
            return false;

        // Walk up collecting ancestors we have to compute, stopping at the root or the
        // first cached directory (whichever comes first).
        var pending = new List<(ulong Frn, MftNode Node)>();
        var current = node;
        var currentFrn = frn;
        string basePath = driveRoot;
        var baseHidden = false;

        for (var depth = 0; ; depth++)
        {
            if (depth > MaxDepth)
                return false;

            pending.Add((currentFrn, current));

            var parentFrn = current.ParentFrn;
            if (parentFrn == NtfsRoot || parentFrn == currentFrn)
                break; // reached the volume root; basePath stays driveRoot

            if (dirCache.TryGetValue(parentFrn, out var cached))
            {
                basePath = cached.Path;
                baseHidden = cached.Hidden;
                break;
            }

            if (!map.TryGetValue(parentFrn, out var parent))
                return false; // broken chain

            current = parent;
            currentFrn = parentFrn;
        }

        // Unwind from the ancestor nearest the base down to the target, building each
        // path once and caching directory results for later siblings.
        var path = basePath;
        var effectiveHidden = baseHidden;
        for (var i = pending.Count - 1; i >= 0; i--)
        {
            var (itemFrn, item) = pending[i];
            path = Combine(path, item.Name);
            effectiveHidden |= item.OwnHidden;
            if (item.IsDirectory)
                dirCache[itemFrn] = (path, effectiveHidden);
        }

        displayPath = path;
        hidden = effectiveHidden;
        return true;
    }

    private const ulong NtfsRoot = 0x0005000000000005UL;

    private static string Combine(string dir, string name) =>
        dir.EndsWith('\\') ? dir + name : dir + '\\' + name;
}
