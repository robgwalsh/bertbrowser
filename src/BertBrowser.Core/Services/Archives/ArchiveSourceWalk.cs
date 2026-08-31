namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// Turns a selection into the flat list of files a new archive will hold, and the name each gets
/// inside it.
/// </summary>
/// <remarks>
/// <para>
/// Separate from <see cref="ArchiveCreator"/> so the naming — which is all a user ever notices
/// about a created archive — is testable without writing one.
/// </para>
/// <para>
/// <b>Reparse points are skipped rather than followed</b>, which is <c>DeleteSurveyor</c>'s rule
/// and <c>DirectoryRemoval</c>'s: a junction is one entry, and following it can walk a tree into
/// itself or drag in gigabytes nobody selected.
/// </para>
/// </remarks>
public static class ArchiveSourceWalk
{
    /// <summary>
    /// Collects the files under <paramref name="sources"/>, named relative to the folder that
    /// holds them.
    /// </summary>
    /// <param name="includeHidden">The browse setting, so what goes in matches what is on show.</param>
    public static IReadOnlyList<ArchiveSource> Collect(
        IReadOnlyList<string> sources, bool includeHidden, CancellationToken ct = default)
    {
        var collected = new List<ArchiveSource>();

        foreach (var source in sources)
        {
            ct.ThrowIfCancellationRequested();

            try
            {
                if (File.Exists(source))
                {
                    var info = new FileInfo(source);
                    if (!Skip(info.Attributes, includeHidden))
                        collected.Add(new ArchiveSource(source, info.Name, info.Length));
                    continue;
                }

                if (Directory.Exists(source))
                {
                    var root = Path.GetDirectoryName(source.TrimEnd('\\')) ?? source;
                    Walk(source, root, includeHidden, collected, ct);
                }
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                           or ArgumentException or NotSupportedException)
            {
                // One unreadable source costs the others nothing.
            }
        }

        return collected;
    }

    private static void Walk(
        string directory, string root, bool includeHidden, List<ArchiveSource> into,
        CancellationToken ct)
    {
        DirectoryInfo info;
        try
        {
            info = new DirectoryInfo(directory);
            if (Skip(info.Attributes, includeHidden)) return;
            // A junction is the one entry it is, never the tree it points at.
            if (info.Attributes.HasFlag(FileAttributes.ReparsePoint)) return;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { return; }

        IEnumerable<FileSystemInfo> children;
        try
        {
            children = info.EnumerateFileSystemInfos();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { return; }

        foreach (var child in children)
        {
            ct.ThrowIfCancellationRequested();

            if (child is DirectoryInfo dir)
            {
                Walk(dir.FullName, root, includeHidden, into, ct);
                continue;
            }

            if (Skip(child.Attributes, includeHidden)) continue;
            if (child.Attributes.HasFlag(FileAttributes.ReparsePoint)) continue;

            into.Add(new ArchiveSource(
                child.FullName,
                EntryNameFor(child.FullName, root),
                child is FileInfo file ? file.Length : 0));
        }
    }

    /// <summary>
    /// The name an entry gets: forward slashes, relative to the folder holding what was selected,
    /// so a selected folder keeps its own name at the top of the archive.
    /// </summary>
    internal static string EntryNameFor(string path, string root)
    {
        var relative = Path.GetRelativePath(root, path);
        return relative.Replace('\\', '/');
    }

    private static bool Skip(FileAttributes attributes, bool includeHidden) =>
        !includeHidden && attributes.HasFlag(FileAttributes.Hidden);
}
