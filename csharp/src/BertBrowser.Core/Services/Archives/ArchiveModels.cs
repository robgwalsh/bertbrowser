namespace BertBrowser.Core.Services.Archives;

/// <summary>One entry as the container reported it, before any tree is built.</summary>
/// <param name="Key">Raw entry key, separators as the container spelled them.</param>
/// <param name="Size">Uncompressed length. Directories carry zero.</param>
/// <param name="CompressedSize">Stored length, or zero when the container does not say.</param>
/// <param name="Modified">Local modification time, or null when the container carries none.</param>
/// <param name="IsDirectory">Whether the container marked this as a directory.</param>
/// <param name="IsEncrypted">Whether this entry's data needs a password.</param>
/// <param name="LinkTarget">Non-null when the entry is a symlink; never followed or recreated.</param>
public sealed record RawArchiveEntry(
    string? Key,
    long Size,
    long CompressedSize,
    DateTime? Modified,
    bool IsDirectory,
    bool IsEncrypted = false,
    string? LinkTarget = null);

/// <summary>What an opened container turned out to support.</summary>
/// <param name="SequentialOnly">
/// Entries cannot be read individually — a solid archive, or a format with no index. Costs a full
/// pass to list, turns preview-on-selection into a button, and makes any byte estimate a floor.
/// </param>
/// <param name="IsEncrypted">The container reported encryption on itself or on some entry.</param>
/// <param name="IsComplete">False for a truncated or multi-volume-with-missing-parts container.</param>
public readonly record struct ArchiveCapabilities(
    bool SequentialOnly,
    bool IsEncrypted,
    bool IsComplete)
{
    public static ArchiveCapabilities Unknown => new(false, false, true);
}

/// <summary>Why an archive could not be listed. The file list turns each into a banner.</summary>
public enum ArchiveFailure
{
    None = 0,
    /// <summary>Not a container we can read, or the bytes are damaged.</summary>
    Damaged,
    /// <summary>Readable only with a password we do not have.</summary>
    PasswordRequired,
    /// <summary>More entries than the browse cap allows.</summary>
    TooManyEntries,
    /// <summary>The file could not be opened at all — gone, locked, or unreadable.</summary>
    Unreadable,
}

/// <summary>One node of the tree an archive's entries describe.</summary>
public sealed class ArchiveNode
{
    /// <summary>Path within the archive, <c>\</c>-separated, no leading or trailing separator.</summary>
    public required string Path { get; init; }

    /// <summary>Last segment of <see cref="Path"/>.</summary>
    public required string Name { get; init; }

    public required bool IsDirectory { get; init; }

    /// <summary>Uncompressed bytes: the entry's own for a file, the exact recursive sum for a
    /// directory. Never negative — inside an archive every size is known.</summary>
    public long SizeBytes { get; internal set; }

    /// <summary>Stored bytes, summed the same way. Zero when the container does not report it.</summary>
    public long CompressedBytes { get; internal set; }

    /// <summary>Null when the container carried no timestamp, which renders blank — never 1601.</summary>
    public DateTime? Modified { get; internal set; }

    public bool IsEncrypted { get; internal set; }

    /// <summary>Non-null for a symlink entry. Listed, never followed.</summary>
    public string? LinkTarget { get; init; }

    internal List<ArchiveNode>? Children { get; set; }
}

/// <summary>
/// An archive's entries as a directory tree, plus what reading it turned out to cost.
/// </summary>
/// <remarks>
/// Built once per open and cached. <see cref="Failed"/> is how a damaged, encrypted or oversized
/// container arrives — <b>a bad archive is a message, never a throw</b>, which is the contract
/// <c>ArchiveListing</c> already had for the preview pane and which now has to hold up a whole
/// browsing surface.
/// </remarks>
public sealed class ArchiveIndex
{
    /// <summary>
    /// The browse cap, deliberately a different number from
    /// <c>ArchiveListing.DefaultMaxEntries</c> (1,000).
    /// </summary>
    /// <remarks>
    /// That one is a "while somebody arrows down a list" number and must stay small. This one is
    /// the largest directory the app already renders, at about a node plus a dictionary slot each.
    /// Unifying them is how the preview pane starts costing 200,000 allocations per keypress.
    /// Re-measure it rather than trusting it.
    /// </remarks>
    public const int MaxEntries = 200_000;

    public required ArchiveNode Root { get; init; }
    public required IReadOnlyDictionary<string, ArchiveNode> ByPath { get; init; }
    public required int FileCount { get; init; }
    public required ArchiveCapabilities Capabilities { get; init; }

    /// <summary>Entries the container listed that this app refused: a key escaping the root, a
    /// null key, or a name colliding with a directory. Reported, never silently zero.</summary>
    public required int RefusedCount { get; init; }

    public ArchiveFailure Failure { get; init; }
    public string? Error { get; init; }

    public bool Ok => Failure == ArchiveFailure.None;

    public static ArchiveIndex Failed(ArchiveFailure failure, string message) => new()
    {
        Root = new ArchiveNode { Path = "", Name = "", IsDirectory = true },
        ByPath = new Dictionary<string, ArchiveNode>(StringComparer.OrdinalIgnoreCase),
        FileCount = 0,
        Capabilities = ArchiveCapabilities.Unknown,
        RefusedCount = 0,
        Failure = failure,
        Error = message,
    };

    /// <summary>The children of one directory inside the archive, or null when there is no such
    /// directory. An empty directory yields an empty list, which is not the same answer.</summary>
    public IReadOnlyList<ArchiveNode>? Children(string entryPath)
    {
        var key = entryPath.Trim('\\');
        var node = key.Length == 0 ? Root : ByPath.GetValueOrDefault(key);
        if (node is not { IsDirectory: true }) return null;
        return node.Children ?? (IReadOnlyList<ArchiveNode>)[];
    }

    /// <summary>The node at <paramref name="entryPath"/>, or null.</summary>
    public ArchiveNode? Find(string entryPath)
    {
        var key = entryPath.Trim('\\');
        return key.Length == 0 ? Root : ByPath.GetValueOrDefault(key);
    }
}
