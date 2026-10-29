using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Delete;

namespace BertBrowser.Core.Services.Duplicates;

/// <summary>Which copy of a group to keep when the user asks for the others to be ticked.</summary>
public enum KeepStrategy
{
    /// <summary>The most recently modified.</summary>
    Newest,

    /// <summary>The least recently modified — usually the original.</summary>
    Oldest,

    /// <summary>The shortest path, which is the copy nearest the top of the tree.</summary>
    Shallowest,
}

/// <summary>
/// The judgements a duplicate scan has to make before it can show or act on anything, kept pure and
/// away from both SQLite and the filesystem so they can be tested rather than eyeballed.
/// </summary>
/// <remarks>
/// Two of them exist to protect the same invariant the rest of this app is shaped around — an
/// unmeasured thing is unknown, never zero. A scan that shortlists on size is silently useless on a
/// volume whose sizes are all zero, and a results list cannot tell that apart from a disk with no
/// duplicates on it. <see cref="Classify"/> is where that is decided, once.
/// </remarks>
public static class DuplicateRules
{
    /// <summary>
    /// How much of a file the first hashing pass reads.
    /// </summary>
    /// <remarks>
    /// Big enough that files differing anywhere in a header, a container's index or the first frame
    /// separate here for almost no I/O; small enough that a group of a hundred same-sized files
    /// costs six megabytes to eliminate rather than gigabytes. A file this size or smaller is
    /// <em>fully</em> hashed by that pass, so it never reaches the second one.
    /// </remarks>
    public const long HeadSampleBytes = 64 * 1024;

    /// <summary>
    /// What standing a scan's answer has, given what the index turned out to hold.
    /// </summary>
    /// <param name="filesInScope">Every file row in range, whatever its length.</param>
    /// <param name="sizedFilesInScope">
    /// How many of those have a real byte length. Rows but no lengths is decisive: it is the
    /// sizeless <c>FSCTL_ENUM_USN_DATA</c> build, on which every file collides with every other and
    /// the shortlist means nothing.
    /// </param>
    /// <param name="isBuilding">Whether any volume's initial enumeration is still running.</param>
    /// <param name="isIndexed">Whether this scope's volume reports a complete live index.</param>
    /// <remarks>
    /// The evidence is weighed before the index service's own opinion, deliberately. A network
    /// share or a removable disk is filled in by <c>IndexCrawler</c> rather than the MFT pass, so
    /// <paramref name="isIndexed"/> is false there while the rows — and their real sizes — are
    /// perfectly good. Asking the service first would refuse to scan a folder it could scan.
    /// </remarks>
    public static DuplicateScanAvailability Classify(
        int filesInScope, int sizedFilesInScope, bool isBuilding, bool isIndexed)
    {
        // Real lengths in range: the shortlist is meaningful whatever anyone thinks of the index.
        if (sizedFilesInScope > 0)
        {
            return isBuilding && !isIndexed
                ? DuplicateScanAvailability.Building
                : DuplicateScanAvailability.Ready;
        }

        // Rows, and not one of them with a length. Still building is a "come back in a moment";
        // otherwise this is the name-only build and saying "no duplicates" would be a lie.
        if (filesInScope > 0)
        {
            return isBuilding
                ? DuplicateScanAvailability.Building
                : DuplicateScanAvailability.NoSizeData;
        }

        // No file rows at all.
        if (isBuilding) return DuplicateScanAvailability.Building;

        // An indexed scope that really is empty has been scanned correctly and found nothing, which
        // is a result. An unindexed one has not been looked at, which is not.
        return isIndexed ? DuplicateScanAvailability.Ready : DuplicateScanAvailability.NotIndexed;
    }

    /// <summary>
    /// Whether a candidate should be left out of the scan altogether.
    /// </summary>
    /// <remarks>
    /// Held paths go for the reason <c>SearchService.Visible</c> drops them: those files are still
    /// on disk so a Ctrl+Z can reach them, but they have been deleted as far as the user is
    /// concerned, and offering to delete one again reads as a delete that silently failed.
    /// </remarks>
    public static bool IsExcluded(string pathKey, bool skipSystemFolders) =>
        DeleteExecutor.IsHeldPath(pathKey) || (skipSystemFolders && IsSystemSubtree(pathKey));

    /// <summary>
    /// True for anything inside Windows, Program Files, or ProgramData.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>This is deliberately not <see cref="ProtectedLocations.Default"/>,</b> and the difference
    /// runs both ways. That set is exact-match and covers the folders themselves only, because its
    /// job is to refuse the one selection that ends the machine; this one has to cover whole
    /// subtrees, because its job is to keep results the user cannot act on out of the list.
    /// </para>
    /// <para>
    /// And it must <em>not</em> include the profile root, which that set does. The profile is where
    /// a person's duplicates actually live — downloads saved twice, photos imported twice, a project
    /// checked out in two places. Skipping it would leave the feature with nothing to find.
    /// </para>
    /// <para>
    /// The bounds are computed once. <see cref="PathKey.IsUnder"/> re-canonicalizes its second
    /// argument on every call, which is fine for a menu and ruinous once per row of a shortlist.
    /// </para>
    /// </remarks>
    public static bool IsSystemSubtree(string pathKey)
    {
        foreach (var (lo, hi) in SystemSubtrees)
        {
            if (string.CompareOrdinal(pathKey, lo) >= 0 && string.CompareOrdinal(pathKey, hi) < 0)
                return true;
        }
        return false;
    }

    /// <summary>
    /// Whether removing <paramref name="tickedCount"/> of a group of <paramref name="groupSize"/> is
    /// something this feature will do.
    /// </summary>
    /// <remarks>
    /// <b>A group may never have every copy ticked.</b> The point of the feature is to reclaim the
    /// space a redundant copy costs, which means one copy always stays; a batch that removed all of
    /// them would destroy the only remaining instance of a file the user was told they had several
    /// of. It is checked here, purely, as well as in the view, so a test can hold it still.
    /// </remarks>
    public static bool CanRemove(int groupSize, int tickedCount) =>
        tickedCount > 0 && groupSize > 1 && tickedCount < groupSize;

    /// <summary>
    /// Which copy of <paramref name="group"/> to keep under <paramref name="strategy"/>; every other
    /// one is what gets ticked.
    /// </summary>
    /// <returns>
    /// An index into <see cref="DuplicateGroup.Files"/>. Never -1: an empty group cannot reach here,
    /// and ties are broken by ordinal path so the same group always yields the same keeper — an
    /// auto-selection that shuffled between presses would be impossible to trust.
    /// </returns>
    public static int ChooseKeeper(DuplicateGroup group, KeepStrategy strategy)
    {
        ArgumentNullException.ThrowIfNull(group);
        if (group.Files.Count == 0) throw new ArgumentException("Group is empty.", nameof(group));

        var best = 0;
        for (var i = 1; i < group.Files.Count; i++)
        {
            if (IsBetterKeeper(group.Files[i], group.Files[best], strategy)) best = i;
        }
        return best;
    }

    private static bool IsBetterKeeper(DuplicateFile candidate, DuplicateFile incumbent, KeepStrategy strategy)
    {
        var verdict = strategy switch
        {
            KeepStrategy.Newest => candidate.ModifiedUtc.CompareTo(incumbent.ModifiedUtc),
            KeepStrategy.Oldest => incumbent.ModifiedUtc.CompareTo(candidate.ModifiedUtc),
            KeepStrategy.Shallowest => incumbent.DisplayPath.Length.CompareTo(candidate.DisplayPath.Length),
            _ => 0,
        };

        // The tiebreak is not decoration: several copies written by one unzip share a timestamp to
        // the tick, and that is exactly the case this feature is most often pointed at.
        return verdict > 0 || (verdict == 0 &&
            string.CompareOrdinal(candidate.DisplayPath, incumbent.DisplayPath) < 0);
    }

    private static readonly (string Lo, string Hi)[] SystemSubtrees = BuildSystemSubtrees();

    private static (string Lo, string Hi)[] BuildSystemSubtrees()
    {
        Environment.SpecialFolder[] folders =
        [
            Environment.SpecialFolder.Windows,
            Environment.SpecialFolder.System,
            Environment.SpecialFolder.SystemX86,
            Environment.SpecialFolder.ProgramFiles,
            Environment.SpecialFolder.ProgramFilesX86,
            Environment.SpecialFolder.CommonProgramFiles,
            Environment.SpecialFolder.CommonProgramFilesX86,
            Environment.SpecialFolder.CommonApplicationData,
        ];

        var bounds = new List<(string Lo, string Hi)>();
        foreach (var path in folders.Select(Environment.GetFolderPath).Distinct(StringComparer.OrdinalIgnoreCase))
        {
            // GetFolderPath answers "" for a folder this machine does not have.
            if (string.IsNullOrWhiteSpace(path)) continue;

            try
            {
                bounds.Add(PathKey.PrefixBounds(PathKey.Canonicalize(path)));
            }
            catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
            {
                // Not a path anything could be under either.
            }
        }
        return [.. bounds];
    }
}
