namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// Turns a container's flat list of entries into the directory tree the file list browses.
/// </summary>
/// <remarks>
/// <para>
/// Pure: it takes entries and returns a tree, so every rule here is testable with no archive, no
/// temp file and no library. The reader does the I/O and hands the result to this.
/// </para>
/// <para>
/// <b>Intermediate directories are synthesized from path prefixes, and an explicit directory entry
/// only decorates a node that already exists.</b> Doing it the other way round is the bug: a zip
/// carrying <c>src/lib/util.js</c> and no <c>src/</c> entry is completely ordinary, and trusting
/// explicit entries leaves a folder that is in every path and cannot be entered.
/// </para>
/// <para>
/// <b>An entry whose key escapes the root is refused here</b> rather than in the extractor. That is
/// Zip Slip — <c>..\..\Windows\System32\evil.dll</c> as an entry name — and refusing it at read
/// time means the entry never exists to be extracted, which makes the extractor's own check a
/// second line of defence rather than the only one.
/// </para>
/// </remarks>
public static class ArchiveIndexBuilder
{
    /// <summary>Builds the tree. Never throws.</summary>
    public static ArchiveIndex Build(
        IEnumerable<RawArchiveEntry> entries,
        ArchiveCapabilities capabilities,
        int maxEntries = ArchiveIndex.MaxEntries)
    {
        var root = new ArchiveNode { Path = "", Name = "", IsDirectory = true };
        var byPath = new Dictionary<string, ArchiveNode>(StringComparer.OrdinalIgnoreCase);
        var refused = 0;
        var files = 0;
        var seen = 0;

        foreach (var entry in entries)
        {
            var key = Normalize(entry.Key);
            if (key is null) { refused++; continue; }

            if (++seen > maxEntries)
                return ArchiveIndex.Failed(
                    ArchiveFailure.TooManyEntries,
                    $"This archive holds more than {maxEntries:N0} entries, which is more than can be browsed.");

            if (entry.IsDirectory)
            {
                var dir = EnsureDirectory(root, byPath, key, ref refused, ref files);
                // An explicit directory entry contributes a timestamp and nothing else. It must
                // never be what creates the node, or a folder that exists only implicitly is lost.
                if (entry.Modified is not null) dir.Modified = entry.Modified;
                continue;
            }

            var cut = key.LastIndexOf('\\');
            var parentPath = cut < 0 ? "" : key[..cut];
            var name = cut < 0 ? key : key[(cut + 1)..];
            if (name.Length == 0) { refused++; continue; }

            var parent = EnsureDirectory(root, byPath, parentPath, ref refused, ref files);

            if (byPath.TryGetValue(key, out var existing))
            {
                // A file whose name a directory already holds is dropped: the directory is
                // reachable and has contents, and the file would make them unreachable.
                if (existing.IsDirectory) { refused++; continue; }

                // Duplicate keys are legal in a zip. Last wins, and the earlier one is counted
                // rather than left behind as a second row with the same path. It was already
                // counted as a file, so the replacement below must not count twice.
                parent.Children!.Remove(existing);
                files--;
                refused++;
            }

            var node = new ArchiveNode
            {
                Path = key,
                Name = name,
                IsDirectory = false,
                SizeBytes = Math.Max(0, entry.Size),
                CompressedBytes = Math.Max(0, entry.CompressedSize),
                Modified = entry.Modified,
                IsEncrypted = entry.IsEncrypted,
                LinkTarget = entry.LinkTarget,
            };
            byPath[key] = node;
            (parent.Children ??= []).Add(node);
            files++;
        }

        Total(root);
        Sort(root);

        return new ArchiveIndex
        {
            Root = root,
            ByPath = byPath,
            FileCount = files,
            Capabilities = capabilities,
            RefusedCount = refused,
        };
    }

    /// <summary>
    /// Normalises an entry key, or returns null when the entry must not exist at all.
    /// </summary>
    /// <remarks>
    /// Refuses a null or empty key, a rooted or UNC key, a drive-qualified key, and any <c>..</c>
    /// segment. The <c>..</c> case is Zip Slip; the rooted cases are the same attack spelled
    /// absolutely, and a container is equally free to spell it either way.
    /// </remarks>
    internal static string? Normalize(string? key)
    {
        if (string.IsNullOrWhiteSpace(key)) return null;

        var path = key.Replace('/', '\\');

        // "\\server\share\x" and "\x" are both rooted; "C:\x" and "C:x" are drive-qualified.
        if (path.StartsWith('\\')) return null;
        if (path.Length >= 2 && path[1] == ':') return null;

        var parts = new List<string>();
        foreach (var segment in path.Split('\\'))
        {
            if (segment.Length == 0 || segment == ".") continue;
            if (segment == "..") return null;

            // Windows silently drops a trailing dot or space, so a container using them to collide
            // two entries would produce a path that does not round-trip through the filesystem.
            var trimmed = segment.TrimEnd(' ', '.');
            if (trimmed.Length == 0) return null;
            parts.Add(trimmed);
        }

        return parts.Count == 0 ? null : string.Join('\\', parts);
    }

    /// <summary>Walks a normalised directory path, creating nodes for anything not yet there.</summary>
    /// <remarks>
    /// It takes the counters by reference because displacing a file to make room for a directory
    /// is a refusal like any other — an entry the container listed that this app did not keep —
    /// and a refusal nobody counts is one the user is never told about.
    /// </remarks>
    private static ArchiveNode EnsureDirectory(
        ArchiveNode root, Dictionary<string, ArchiveNode> byPath, string path,
        ref int refused, ref int files)
    {
        if (path.Length == 0) return root;

        var current = root;
        var accumulated = "";
        foreach (var segment in path.Split('\\'))
        {
            accumulated = accumulated.Length == 0 ? segment : accumulated + "\\" + segment;

            if (byPath.TryGetValue(accumulated, out var existing))
            {
                if (existing.IsDirectory) { current = existing; continue; }

                // A directory some other entry's path needs, whose name a file already took. The
                // directory wins, for the same reason as above.
                current.Children?.Remove(existing);
                files--;
                refused++;
                existing = new ArchiveNode { Path = accumulated, Name = segment, IsDirectory = true };
                byPath[accumulated] = existing;
                (current.Children ??= []).Add(existing);
                current = existing;
                continue;
            }

            var node = new ArchiveNode { Path = accumulated, Name = segment, IsDirectory = true };
            byPath[accumulated] = node;
            (current.Children ??= []).Add(node);
            current = node;
        }
        return current;
    }

    /// <summary>
    /// Sums each directory's contents into it, exactly. Nothing is walked on disk to get this —
    /// the numbers were already in the container's own directory — so the app's rule that nothing
    /// scans the filesystem to size a folder is untouched.
    /// </summary>
    private static (long Size, long Compressed) Total(ArchiveNode node)
    {
        if (!node.IsDirectory) return (node.SizeBytes, node.CompressedBytes);

        long size = 0, compressed = 0;
        if (node.Children is { } children)
        {
            foreach (var child in children)
            {
                var (s, c) = Total(child);
                size += s;
                compressed += c;
            }
        }
        node.SizeBytes = size;
        node.CompressedBytes = compressed;
        return (size, compressed);
    }

    private static void Sort(ArchiveNode node)
    {
        if (node.Children is not { } children) return;
        children.Sort(static (a, b) => string.Compare(a.Name, b.Name, StringComparison.OrdinalIgnoreCase));
        foreach (var child in children) Sort(child);
    }
}
