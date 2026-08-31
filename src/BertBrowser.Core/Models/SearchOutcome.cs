namespace BertBrowser.Core.Models;

/// <summary>Where a search's results came from.</summary>
public enum SearchResultSource
{
    /// <summary>Streamed from a live filesystem scan (subtree not indexed yet).</summary>
    LiveScan,

    /// <summary>Served from a fresh index.</summary>
    Index,

    /// <summary>Served from an index that is being re-crawled in the background.</summary>
    StaleIndex,
}

/// <summary>
/// Final result of a search. When <paramref name="RefreshPending"/> is true a
/// background (re)crawl was started and <c>ISearchService.IndexRefreshed</c> will
/// fire for the covering root once it completes.
/// </summary>
/// <param name="Problem">
/// Why the query could not be used, in words for the user — an unreadable size, a regular
/// expression that will not compile, a filter this index cannot answer. An outcome carrying one
/// has no hits and is <em>not</em> the same as a search that found nothing: the caller shows the
/// message and leaves the previous results on screen, rather than reporting "no results" for a
/// query that never ran.
/// </param>
/// <param name="ScopeLacksMetadata">
/// Set when the query filtered on size or modified time, found nothing, and the index has no
/// lengths in scope to filter on — the sizeless <c>FSCTL_ENUM_USN_DATA</c> build. The search was
/// unanswerable rather than unmatched, and reporting a plain "no results" would say the disk
/// holds no such files when what is really true is that nothing measured it.
/// </param>
/// <param name="Cancelled">
/// The user stopped the search. <paramref name="Hits"/> is then a floor rather than the answer —
/// what is in it really did match. Separate from <paramref name="ContentScan"/>'s own
/// <c>Incomplete</c> for the reason <c>DuplicateScanOutcome</c> keeps its two flags apart:
/// conflating "you stopped it" with "some files could not be read" makes a cancelled run look like
/// a disk full of broken files.
/// </param>
/// <param name="ContentScan">
/// What the <c>content:</c> reading pass actually did, or null when the query had no content term.
/// Null is therefore also how a caller knows whether to show the Match column at all.
/// </param>
public sealed record SearchOutcome(
    IReadOnlyList<SearchHit> Hits,
    bool Truncated,
    SearchResultSource Source,
    bool RefreshPending,
    string? Problem = null,
    bool ScopeLacksMetadata = false,
    bool Cancelled = false,
    ContentScanReport? ContentScan = null);
