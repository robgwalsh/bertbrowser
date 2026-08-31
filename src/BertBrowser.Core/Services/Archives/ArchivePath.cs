using BertBrowser.Core.Cli;

namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// A path that points inside an archive: the container on disk, and the entry path within it.
/// </summary>
/// <remarks>
/// <para>
/// <b>A virtual path is an ordinary Windows path</b> — <c>C:\x\a.zip\src\lib</c> —  and that is the
/// whole design. <c>Path.GetFullPath</c> accepts it, so <c>PathKey</c>, the breadcrumb,
/// <c>Path.GetDirectoryName</c> (which is what Up and the back stack use),
/// <c>NavigationRequest.IsAcceptablePath</c> and the single-instance wire format all keep working
/// with no changes at all. A <c>zip://</c> scheme would have thrown out of <c>GetFullPath</c> and
/// broken every one of them. Do not introduce one.
/// </para>
/// <para>
/// <b>Which segment is the archive cannot be decided without touching disk</b>, because a folder
/// really can be named <c>foo.zip</c>. So this splits the way <see cref="Paths.UniquePath"/> does:
/// the parser nominates candidates and an injected delegate decides, which keeps every rule here
/// testable and leaves "what is really on disk" with the caller.
/// </para>
/// </remarks>
public readonly record struct ArchivePath(string ArchiveFile, string EntryPath)
{
    /// <summary>The archive's own root, rather than something inside it.</summary>
    public bool IsRoot => EntryPath.Length == 0;

    /// <summary>The containing entry, or null at the root (whose parent is a real folder).</summary>
    public ArchivePath? Parent
    {
        get
        {
            if (IsRoot) return null;
            var cut = EntryPath.LastIndexOf('\\');
            return new ArchivePath(ArchiveFile, cut < 0 ? "" : EntryPath[..cut]);
        }
    }

    /// <summary>The full virtual path this describes.</summary>
    public override string ToString() => Compose(ArchiveFile, EntryPath);

    /// <summary>Joins a container and an entry path back into one virtual path.</summary>
    public static string Compose(string archiveFile, string entryPath) =>
        entryPath.Length == 0
            ? archiveFile
            : archiveFile.TrimEnd('\\') + "\\" + entryPath.Trim('\\');

    /// <summary>
    /// A purely syntactic pre-filter: does any segment of <paramref name="path"/> name an archive.
    /// Touches nothing. This is what keeps the navigation gate free for an ordinary folder — only
    /// when it says "maybe" does anything ask the disk.
    /// </summary>
    public static bool LooksVirtual(string? path)
    {
        if (string.IsNullOrEmpty(path)) return false;

        // The last segment counts too: "C:\x\a.zip" is the archive's own root, and navigating to it
        // is exactly what entering an archive is. Leaving it out made the gate refuse the one path
        // every double-click produces.
        var start = 0;
        while (start <= path.Length)
        {
            var cut = path.IndexOf('\\', start);
            var end = cut < 0 ? path.Length : cut;
            if (end > start && ArchiveFormats.IsArchiveName(path[start..end])) return true;
            if (cut < 0) break;
            start = cut + 1;
        }
        return false;
    }

    /// <summary>
    /// Splits <paramref name="path"/> into a container and an entry path, asking
    /// <paramref name="isArchiveFile"/> whether each candidate prefix is really a file on disk.
    /// Returns null when nothing in the path is an archive, or when the result would be unsafe.
    /// </summary>
    /// <remarks>
    /// <b>The shortest matching prefix wins</b>, so <c>a.zip\b.zip\c.txt</c> resolves to the
    /// outermost container: <c>a.zip</c> is a file and <c>a.zip\b.zip</c> is not, so the inner one
    /// is an entry rather than a second container. Nesting then falls out as a refusal below
    /// rather than as a misparse.
    /// </remarks>
    public static ArchivePath? Parse(string? path, Func<string, bool> isArchiveFile)
    {
        if (string.IsNullOrEmpty(path)) return null;
        if (!IsAcceptable(path)) return null;

        var start = 0;
        while (start <= path.Length)
        {
            var cut = path.IndexOf('\\', start);
            var end = cut < 0 ? path.Length : cut;

            if (end > start && ArchiveFormats.IsArchiveName(path[start..end]))
            {
                var candidate = path[..end];
                if (isArchiveFile(candidate))
                {
                    // The archive file itself is its own root, which is what a double-click
                    // navigates to before you have gone anywhere inside it.
                    var entry = cut < 0 ? "" : path[(cut + 1)..].Trim('\\');

                    // A zip inside a zip would have to be written out somewhere before it could be
                    // opened, and that means creating a file the user never asked for. Refused;
                    // the status line offers Extract instead.
                    if (entry.Length > 0 && ArchiveFormats.IsArchiveName(LastSegment(entry))) return null;
                    return new ArchivePath(candidate, entry);
                }
            }

            if (cut < 0) break;
            start = cut + 1;
        }
        return null;
    }

    /// <summary>Whether a string may be treated as a virtual path at all.</summary>
    /// <remarks>
    /// <para>
    /// <b>The <c>..</c> test is on the raw string and happens before anything canonicalizes.</b>
    /// <c>Path.GetFullPath(@"C:\x\a.zip\..\..\Windows")</c> is <c>C:\Windows</c> — a real folder —
    /// so a parser that canonicalized first would hand an extractor a destination outside the
    /// archive entirely, and hand navigation a path that is not the one it was given. This is the
    /// single most load-bearing line in the file.
    /// </para>
    /// <para>
    /// Everything else defers to <see cref="NavigationRequest.IsAcceptablePath"/> rather than
    /// restating it, for exactly the reason that function exists: one rule about what a path may
    /// be, in one place, so the two cannot drift.
    /// </para>
    /// </remarks>
    public static bool IsAcceptable(string? path)
    {
        if (!NavigationRequest.IsAcceptablePath(path)) return false;

        foreach (var segment in path!.Split('\\'))
            if (segment is ".." or ".") return false;

        return true;
    }

    private static string LastSegment(string entryPath)
    {
        var cut = entryPath.LastIndexOf('\\');
        return cut < 0 ? entryPath : entryPath[(cut + 1)..];
    }
}
