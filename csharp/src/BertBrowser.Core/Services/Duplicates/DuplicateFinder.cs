using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.Core.Services.Duplicates;

public interface IDuplicateFinder
{
    /// <summary>
    /// Finds files that are byte-for-byte identical under <see cref="DuplicateScanRequest.RootPath"/>,
    /// or across every indexed volume when it is null.
    /// </summary>
    /// <remarks>
    /// The shortlist is a full scan of the index and the passes after it read real files, so this
    /// takes seconds at best and minutes on a large disk. It is meant to be reached from an explicit
    /// "go and find these" gesture, never from typing, and the token is honoured throughout —
    /// a cancelled scan hands back what it had already confirmed rather than nothing.
    /// </remarks>
    Task<DuplicateScanOutcome> ScanAsync(
        DuplicateScanRequest request,
        IProgress<DuplicateScanProgress>? progress,
        CancellationToken ct);
}

/// <summary>
/// The async facade over the duplicate scan, shaped like <see cref="DiskUsage.DiskUsageService"/>
/// and <see cref="SearchService"/>: the shortlist below is synchronous ADO.NET and the hashing is
/// blocking file I/O, and this is what keeps view models off both.
/// </summary>
/// <remarks>
/// It exists for one other reason too. <see cref="DuplicateScanner"/> takes the two index facts as
/// booleans so that every availability case can be tested by handing it a pair; resolving them from
/// <see cref="IMftIndexService"/> is this layer's job, and the whole-PC scope asks
/// <see cref="IMftIndexService.AnyIndexed"/> where a scoped one asks about its own volume — the
/// same distinction <c>DiskUsageService</c> draws.
/// </remarks>
public sealed class DuplicateFinder(
    IDuplicateCandidateSource candidates,
    IFileHasher hasher,
    IMftIndexService mftIndex) : IDuplicateFinder
{
    public Task<DuplicateScanOutcome> ScanAsync(
        DuplicateScanRequest request,
        IProgress<DuplicateScanProgress>? progress,
        CancellationToken ct)
    {
        ArgumentNullException.ThrowIfNull(request);

        return Task.Run(() =>
        {
            string? rootKey = null;
            if (request.RootPath is { Length: > 0 } path)
            {
                try
                {
                    rootKey = PathKey.Canonicalize(path);
                }
                catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
                {
                    // Not a folder anything could be scanned under.
                    return DuplicateScanOutcome.Empty(DuplicateScanAvailability.NotIndexed);
                }
            }

            var scanner = new DuplicateScanner(candidates, hasher);

            return scanner.Scan(
                request,
                mftIndex.IsBuilding,
                rootKey is null ? mftIndex.AnyIndexed : mftIndex.IsIndexed(rootKey),
                ct,
                progress);
        }, ct);
    }
}
