namespace BertBrowser.Core.Models;

/// <summary>
/// Whether a disk-usage answer can be trusted, and if not, why. The distinction that matters is
/// between "this folder holds nothing" and "nobody has measured this folder" — they look identical
/// in a size column and mean opposite things.
/// </summary>
public enum DiskUsageAvailability
{
    /// <summary>Real sizes, from a completed index.</summary>
    Ready,

    /// <summary>The volume is still being indexed. Whatever is shown is a floor, not a total.</summary>
    Building,

    /// <summary>
    /// The index covers this volume but carries no sizes for it. This is the
    /// <c>FSCTL_ENUM_USN_DATA</c> fallback shape: the volume indexer takes that path when the raw
    /// $MFT is unparseable, and it writes every row with <c>size_bytes = 0</c> and fills no
    /// dir_size_cache at all. Rows exist, so this is not <see cref="NotIndexed"/>, and every one of
    /// them is zero, so it must never be rendered as a list of empty files.
    /// </summary>
    NoSizeData,

    /// <summary>Nothing indexed here — a declined elevation prompt, a standard-user account, a
    /// non-NTFS volume, or an indexer that never ran.</summary>
    NotIndexed,

    /// <summary>The root asked about is not a usable directory path.</summary>
    NotAPath,
}

/// <summary>
/// One row of a disk-usage breakdown: a direct child of the folder being examined.
/// </summary>
/// <param name="SizeBytes">
/// Null means <em>unknown</em>, and the distinction is the whole point of this type. A directory
/// with no dir_size_cache row has not been measured; rendering that as 0 claims it is empty. Every
/// consumer must show blank, never a number.
/// </param>
public sealed record DiskUsageNode(
    string PathKey,
    string DisplayPath,
    string Name,
    bool IsDirectory,
    long? SizeBytes,
    bool Incomplete,
    bool Hidden);

/// <summary>The composition of one directory: its children, largest first.</summary>
/// <param name="TotalBytes">The folder's own cached total, or null when it has not been measured.</param>
/// <param name="UnaccountedBytes">
/// What <paramref name="TotalBytes"/> holds that the children do not explain. Null whenever it
/// cannot be computed honestly, which is most of the time on an unindexed volume.
/// </param>
/// <param name="UnknownChildCount">
/// How many children came back with no size. They are still listed, but they can be given no area.
/// </param>
public sealed record DiskUsageBreakdown(
    string RootDisplayPath,
    long? TotalBytes,
    IReadOnlyList<DiskUsageNode> Children,
    long? UnaccountedBytes,
    int UnknownChildCount,
    DiskUsageAvailability Availability);

/// <summary>The biggest files under some root, largest first, with the standing of the data.</summary>
public sealed record LargestFilesOutcome(
    IReadOnlyList<SearchHit> Files,
    DiskUsageAvailability Availability);
