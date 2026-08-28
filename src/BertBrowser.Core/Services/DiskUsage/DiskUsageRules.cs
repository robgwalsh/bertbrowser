using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services.DiskUsage;

/// <summary>
/// The two judgements a disk-usage view has to make before it can draw anything, kept pure and
/// away from SQLite so they can be tested rather than eyeballed: <em>can these numbers be
/// believed</em>, and <em>how much of a folder do its children fail to explain</em>.
/// </summary>
/// <remarks>
/// Both answers exist to protect one invariant, which this whole feature is shaped around: an
/// unmeasured folder is unknown, never zero. A size column cannot tell those apart, so the
/// decision is made here, once, where a test can hold it still.
/// </remarks>
public static class DiskUsageRules
{
    /// <summary>
    /// What standing the rows returned for <paramref name="rootKey"/> have.
    /// </summary>
    /// <param name="rowCount">How many file rows the query returned.</param>
    /// <param name="largestSizeBytes">
    /// The biggest size among them — which, since the query orders by size descending, is the first
    /// row's. Zero here is decisive: if the <em>largest</em> file in range is 0 bytes then every row
    /// in range is, and that is not a disk full of empty files, it is a volume indexed by the
    /// sizeless <c>FSCTL_ENUM_USN_DATA</c> fallback.
    /// </param>
    /// <param name="isBuilding">Whether any volume's initial enumeration is still running.</param>
    /// <param name="isIndexed">Whether this root's volume reports a complete live index.</param>
    public static DiskUsageAvailability Classify(
        string? rootKey, int rowCount, long largestSizeBytes, bool isBuilding, bool isIndexed)
    {
        // A null root is the deliberate "whole PC" scope, not a bad argument.
        if (rootKey is not null && rootKey.Length == 0) return DiskUsageAvailability.NotAPath;

        if (rowCount > 0)
        {
            // Rows but no bytes: the sizeless build path. Reporting Ready here is what would put a
            // screenful of "0 B" in front of the user, so it is checked before anything else.
            if (largestSizeBytes <= 0) return DiskUsageAvailability.NoSizeData;
            return isBuilding && !isIndexed ? DiskUsageAvailability.Building : DiskUsageAvailability.Ready;
        }

        // No rows at all. Still building is a "come back in a moment"; anything else is genuinely
        // not indexed, and the caller offers the same retry the status bar does.
        if (isBuilding) return DiskUsageAvailability.Building;
        return isIndexed ? DiskUsageAvailability.Ready : DiskUsageAvailability.NotIndexed;
    }

    /// <summary>
    /// What standing one folder's breakdown has.
    /// </summary>
    /// <remarks>
    /// This is deliberately <em>not</em> <see cref="Classify"/>. That rule reads an all-zero result
    /// as a sizeless volume, which is sound for a top-N by size across a subtree but wrong here: a
    /// folder holding three empty files really is all zeros, and calling that "no size data" would
    /// be a lie about ordinary content. The evidence differs too — a breakdown's file sizes come
    /// from the enumeration and are always real, so only the <em>directory</em> totals depend on the
    /// index, and it is those this weighs.
    /// </remarks>
    /// <param name="directoryChildCount">How many of the children are directories.</param>
    /// <param name="measuredDirectoryCount">How many of those had a dir_size_cache row.</param>
    public static DiskUsageAvailability ClassifyBreakdown(
        int directoryChildCount, int measuredDirectoryCount, bool isBuilding, bool isIndexed)
    {
        // Files only: the enumeration already carried real lengths, so this answer needs no index
        // at all and is complete on a volume nothing has ever looked at.
        if (directoryChildCount == 0) return DiskUsageAvailability.Ready;

        if (measuredDirectoryCount > 0)
            return isBuilding && !isIndexed ? DiskUsageAvailability.Building : DiskUsageAvailability.Ready;

        // Subfolders, and not one of them measured.
        if (isBuilding) return DiskUsageAvailability.Building;

        // A volume that claims to be indexed and still has no folder total anywhere is the sizeless
        // FSCTL_ENUM_USN_DATA build, which fills no dir_size_cache.
        return isIndexed ? DiskUsageAvailability.NoSizeData : DiskUsageAvailability.NotIndexed;
    }

    /// <summary>
    /// How many of <paramref name="totalBytes"/> the children do not account for — the folder's own
    /// loose files on a directory-only breakdown, or simply the part nobody measured.
    /// </summary>
    /// <returns>
    /// Null whenever the honest answer is "cannot say": the parent was never measured, or any one
    /// child was not. A remainder computed from an incomplete sum is not a smaller number, it is a
    /// wrong one — treating an unknown child as zero would silently attribute its bytes to the
    /// parent's own files.
    /// </returns>
    public static long? Unaccounted(long? totalBytes, IEnumerable<DiskUsageNode> children)
    {
        ArgumentNullException.ThrowIfNull(children);
        if (totalBytes is not { } total) return null;

        long sum = 0;
        foreach (var child in children)
        {
            if (child.SizeBytes is not { } size) return null;
            sum += size;
        }

        var remainder = total - sum;

        // A parent row that predates its children's — a stale dir_size_cache entry against a folder
        // that has since grown — makes this negative. There is no such thing as negative space, so
        // the answer is "cannot say" rather than a bar drawn backwards.
        return remainder < 0 ? null : remainder;
    }
}
