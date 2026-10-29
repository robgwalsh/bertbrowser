using BertBrowser.Core.Data;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.Core.Services.Compare;

/// <summary>Whether a comparison could be run, and how far to trust it.</summary>
public enum CompareAvailability
{
    Ready,

    /// <summary>A volume's first enumeration is still running, so a side read from the index is a
    /// floor rather than a listing. The verdicts stand, but something may still be missing.</summary>
    Building,

    /// <summary>The pair was refused before anything was read. <c>Problem</c> says why.</summary>
    Refused,

    /// <summary>A side could not be listed at all — access denied at its root, or it went away
    /// between being chosen and being read.</summary>
    Unreadable,
}

/// <summary>How far a comparison has got. Two counters and no denominator: neither side's size is
/// known until its scan finishes, and a bar pinned at nothing reads as a stall.</summary>
public sealed record CompareProgress(int LeftSeen, int RightSeen);

/// <param name="Truncated">A side hit the entry ceiling, so what came back is a prefix of that
/// folder rather than all of it.</param>
public sealed record FolderCompareOutcome(
    string LeftPath,
    string RightPath,
    CompareResult Result,
    CompareAvailability Availability,
    CompareSourceKind LeftSource,
    CompareSourceKind RightSource,
    bool Truncated,
    bool Cancelled,
    string? Problem)
{
    /// <summary>
    /// Whether this result may be turned into a sync. A refusal, an unreadable side, a cancelled
    /// run and a truncated scan each leave a listing that does not describe the folder it names —
    /// and "only on the right", read off a partial left side, would offer to delete files that are
    /// sitting right there.
    /// </summary>
    public bool CanSync =>
        Availability is CompareAvailability.Ready or CompareAvailability.Building
        && !Truncated && !Cancelled;

    public static FolderCompareOutcome Refused(string left, string right, string problem) =>
        new(left, right, CompareResult.None, CompareAvailability.Refused,
            CompareSourceKind.Walk, CompareSourceKind.Walk, false, false, problem);
}

/// <summary>The ceilings a comparison works within.</summary>
public static class CompareLimits
{
    /// <summary>
    /// How many entries one side may contribute.
    /// </summary>
    /// <remarks>
    /// Measured against a real 1.65-million-row index: about 300 bytes and 1.5 microseconds an
    /// entry, so half a million a side is roughly 150 MB and a second of scanning each — and two
    /// sides of that is a peak this app can afford for the seconds a comparison lasts. It also
    /// clears the trees people actually compare (a large source checkout here is 261,000 entries)
    /// while stopping someone who points both panes at a drive root from spending a gigabyte
    /// finding that out.
    /// </remarks>
    public const int MaxEntriesPerSide = 500_000;

    /// <summary>How often a walk reports its running count. Frequent enough to look alive, rare
    /// enough that the reporting is not what the walk is doing.</summary>
    internal const int WalkReportInterval = 2_000;
}

public interface IFolderCompareService
{
    Task<FolderCompareOutcome> CompareAsync(
        string leftPath, string rightPath, bool includeHidden,
        CancellationToken ct, IProgress<CompareProgress>? progress = null);
}

/// <summary>
/// Listing two folders and handing both to <see cref="FolderComparer"/>.
/// </summary>
/// <remarks>
/// <para>
/// Each side chooses its own source. A complete, unstale, live-tailed index that actually carries
/// byte lengths is read with one range scan; anything else is walked, and the two sides may well
/// differ. That choice is deliberately <em>not</em> something the user is told: unlike a duplicate
/// scan, which <em>is</em> the index, a comparison always has an answer available, so "the index
/// cannot help here" picks a source rather than raising a message. The agreement test is what
/// keeps the two sources from meaning different things by it.
/// </para>
/// <para>
/// The sides are read one after the other rather than in parallel. They are usually the same disk,
/// and an index scan is bound by SQLite on one connection either way, so a second thread would buy
/// interleaved progress counters and nothing else.
/// </para>
/// </remarks>
public sealed class FolderCompareService : IFolderCompareService
{
    private readonly FsIndexRepository _index;
    private readonly IMftIndexService _mft;
    private readonly ICompareProbe _probe;
    private readonly Func<string, string?> _driveFormat;

    /// <param name="driveFormat">What filesystem a path sits on, for the timestamp tolerance.
    /// Injected so a test can pose a FAT volume without one.</param>
    public FolderCompareService(
        FsIndexRepository index,
        IMftIndexService mft,
        ICompareProbe? probe = null,
        Func<string, string?>? driveFormat = null)
    {
        _index = index;
        _mft = mft;
        _probe = probe ?? new FileSystemCompareProbe();
        _driveFormat = driveFormat ?? DriveFormat;
    }

    public Task<FolderCompareOutcome> CompareAsync(
        string leftPath, string rightPath, bool includeHidden,
        CancellationToken ct, IProgress<CompareProgress>? progress = null)
    {
        if (CompareRefusal.Check(leftPath, rightPath, _probe) is { } problem)
            return Task.FromResult(FolderCompareOutcome.Refused(leftPath, rightPath, problem));

        return Task.Run(() => Compare(leftPath, rightPath, includeHidden, ct, progress), ct);
    }

    private FolderCompareOutcome Compare(
        string leftPath, string rightPath, bool includeHidden,
        CancellationToken ct, IProgress<CompareProgress>? progress)
    {
        var seen = new CompareProgress(0, 0);

        SideListing left, right;
        try
        {
            left = Read(leftPath, includeHidden, ct, count =>
            {
                seen = seen with { LeftSeen = count };
                progress?.Report(seen);
            });

            right = Read(rightPath, includeHidden, ct, count =>
            {
                seen = seen with { RightSeen = count };
                progress?.Report(seen);
            });
        }
        catch (OperationCanceledException)
        {
            return Empty(leftPath, rightPath, CompareAvailability.Ready, cancelled: true, problem: null);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
            or System.Security.SecurityException)
        {
            return Empty(
                leftPath, rightPath, CompareAvailability.Unreadable, cancelled: false,
                problem: $"One of the folders could not be read. {ex.Message}");
        }

        var tolerance = CompareTolerance.For(_driveFormat(leftPath), _driveFormat(rightPath));
        var result = FolderComparer.Compare(left.Entries, right.Entries, tolerance);

        // Only worth saying when a side actually came from the index — a walk is as complete as the
        // disk is, whatever the volume enumeration is doing elsewhere.
        var building = _mft.IsBuilding
            && (left.Source is CompareSourceKind.Index || right.Source is CompareSourceKind.Index);

        return new FolderCompareOutcome(
            PathKey.NormalizeDisplay(leftPath),
            PathKey.NormalizeDisplay(rightPath),
            result,
            building ? CompareAvailability.Building : CompareAvailability.Ready,
            left.Source, right.Source,
            left.Truncated || right.Truncated,
            Cancelled: false,
            Problem: null);
    }

    private static FolderCompareOutcome Empty(
        string leftPath, string rightPath, CompareAvailability availability,
        bool cancelled, string? problem) =>
        new(leftPath, rightPath, CompareResult.None, availability,
            CompareSourceKind.Walk, CompareSourceKind.Walk, false, cancelled, problem);

    private readonly record struct SideListing(
        IReadOnlyList<CompareEntry> Entries, CompareSourceKind Source, bool Truncated);

    private SideListing Read(string path, bool includeHidden, CancellationToken ct, Action<int> report) =>
        UsesIndex(path)
            ? ReadFromIndex(path, includeHidden, ct, report)
            : ReadFromDisk(path, includeHidden, ct, report);

    /// <summary>
    /// Whether this side can be answered from the index.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The last clause is the one that matters. The name-only build path writes every row with no
    /// length and no timestamp, and a comparison served from such a volume would find every file
    /// the same size as its twin. It would still be safe — a missing timestamp is
    /// <see cref="CompareVerdict.Unknown"/> at the leaf and nothing may be synced from it — but it
    /// would be safe by refusing to say anything, where a walk gives a real answer.
    /// </para>
    /// <para>
    /// It cannot tell a volume with no lengths from a folder that honestly holds no bytes: a folder
    /// of empty files, or of nothing at all, reads the same as an unmeasured drive and falls to the
    /// walk. That is the harmless direction — a slower path to the same verdicts — and it is why
    /// the question is asked of the folder rather than of the volume, where proving the negative
    /// would mean scanning every row on the disk.
    /// </para>
    /// </remarks>
    private bool UsesIndex(string path)
    {
        var covering = _index.FindCoveringRoot(path);
        return covering is { Stale: false }
            && _mft.IsIndexed(covering.PathKey)
            && _index.HasSizeData(path);
    }

    private SideListing ReadFromIndex(
        string path, bool includeHidden, CancellationToken ct, Action<int> report)
    {
        var (rows, truncated) = _index.Subtree(
            path, includeHidden, CompareLimits.MaxEntriesPerSide, DeleteExecutor.IsHeldPath, ct);

        var entries = new List<CompareEntry>(rows.Count);
        foreach (var row in rows)
        {
            entries.Add(new CompareEntry(
                row.RelativeKey, row.Name, row.IsDirectory, row.SizeBytes, row.ModifiedUtc));
        }

        report(entries.Count);
        return new SideListing(entries, CompareSourceKind.Index, truncated);
    }

    /// <summary>
    /// The walk twin. <c>FileSystemWalker</c> emits a junction or symlink as an entry and never
    /// descends it, and both index build paths hold a row for the link and none for anything
    /// beneath it — so the two sources agree about a linked folder by construction rather than by a
    /// rule written out twice and kept in step by hand.
    /// </summary>
    private static SideListing ReadFromDisk(
        string path, bool includeHidden, CancellationToken ct, Action<int> report)
    {
        // The same slice the index scan takes, so a drive root does not leave a leading separator
        // on one side's keys and nothing pairs.
        var prefixLength = PathKey.PrefixBounds(PathKey.Canonicalize(path)).Lo.Length;
        var entries = new List<CompareEntry>();
        var truncated = false;

        FileSystemWalker.Walk(path, entry =>
        {
            if (DeleteExecutor.IsHeldPath(entry.PathKey)) return true;

            if (entries.Count == CompareLimits.MaxEntriesPerSide)
            {
                truncated = true;
                return false;
            }

            entries.Add(new CompareEntry(
                entry.PathKey[prefixLength..], entry.Name,
                entry.IsDirectory, entry.SizeBytes, entry.ModifiedUtc));

            if (entries.Count % CompareLimits.WalkReportInterval == 0) report(entries.Count);
            return true;
        }, ct, includeHidden);

        report(entries.Count);
        return new SideListing(entries, CompareSourceKind.Walk, truncated);
    }

    /// <summary>The volume's filesystem, or null when it will not say — a share, a disconnected
    /// mapping. Null is what keeps the timestamp tolerance strict.</summary>
    private static string? DriveFormat(string path)
    {
        try
        {
            var root = Path.GetPathRoot(Path.GetFullPath(path));
            return root is { Length: > 0 } ? new DriveInfo(root).DriveFormat : null;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
            or ArgumentException or NotSupportedException or System.Security.SecurityException)
        {
            return null;
        }
    }
}
