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
public sealed record SearchOutcome(
    IReadOnlyList<SearchHit> Hits,
    bool Truncated,
    SearchResultSource Source,
    bool RefreshPending);
