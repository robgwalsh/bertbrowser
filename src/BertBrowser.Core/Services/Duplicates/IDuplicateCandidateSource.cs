using BertBrowser.Core.Data;
using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services.Duplicates;

/// <summary>
/// Where a scan gets its shortlist of files that might be duplicates.
/// </summary>
/// <remarks>
/// A seam for the reason the probes elsewhere in this app are: it lets <c>DuplicateScannerTests</c>
/// exercise the grouping, the hardlink folding, the cancellation and every availability case
/// against a handful of made-up rows, with no SQLite and no disk.
/// </remarks>
public interface IDuplicateCandidateSource
{
    /// <summary>
    /// Files sharing a byte length with at least one other, plus what the index turned out to hold —
    /// which is what decides whether the answer can be believed at all.
    /// </summary>
    DuplicateShortlist Shortlist(DuplicateScanRequest request, CancellationToken ct);
}

/// <summary>The real shortlist, straight out of the MFT/search index.</summary>
/// <remarks>
/// No file is opened here and nothing walks the filesystem. Every byte length this reads was
/// written by the MFT pass as a side effect of building the search index, which is what makes the
/// expensive half of finding duplicates already paid for.
/// </remarks>
public sealed class IndexedDuplicateCandidateSource(FsIndexRepository index) : IDuplicateCandidateSource
{
    public DuplicateShortlist Shortlist(DuplicateScanRequest request, CancellationToken ct)
    {
        ArgumentNullException.ThrowIfNull(request);

        return index.DuplicateCandidates(
            request.RootPath,
            request.MinSizeBytes,
            request.IncludeHidden,
            key => DuplicateRules.IsExcluded(key, request.SkipSystemFolders),
            ct);
    }
}
