using System.Collections.Concurrent;
using System.Diagnostics;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services;

public interface ISearchService
{
    /// <summary>
    /// Searches for <paramref name="queryText"/> under <paramref name="rootPath"/>.
    /// Returns null when the query is too short to run. When the subtree is not
    /// indexed yet, live matches stream through <paramref name="liveBatches"/> while
    /// a background crawl builds the index; the returned outcome contains the full
    /// (possibly capped) hit list either way.
    /// </summary>
    Task<SearchOutcome?> SearchAsync(
        string rootPath, string queryText, CancellationToken ct,
        IProgress<IReadOnlyList<SearchHit>>? liveBatches = null, bool includeHidden = true);

    /// <summary>Fires (on a worker thread) with the canonical root key whose (re)crawl just completed.</summary>
    event Action<string>? IndexRefreshed;
}

/// <summary>
/// Search orchestration — lazy indexing with stale-while-revalidate:
/// fresh index → instant DB query; stale/unwatched index → instant DB query plus a
/// background re-crawl; unindexed → live filesystem scan streaming hits while a
/// single-flight background crawl indexes the subtree for next time.
/// </summary>
public sealed class SearchService : ISearchService, IDisposable
{
    private const int MaxResults = 1000;
    private const int LiveBatchSize = 50;
    private static readonly TimeSpan LiveBatchInterval = TimeSpan.FromMilliseconds(250);

    private readonly FsIndexRepository _repository;
    private readonly IndexCrawler _crawler;
    private readonly IIndexWatcherService _watchers;
    private readonly ConcurrentDictionary<string, Task> _activeCrawls = new(StringComparer.Ordinal);
    private readonly CancellationTokenSource _lifetime = new();

    public event Action<string>? IndexRefreshed;

    public SearchService(FsIndexRepository repository, IndexCrawler crawler, IIndexWatcherService watchers)
    {
        _repository = repository;
        _crawler = crawler;
        _watchers = watchers;
    }

    public async Task<SearchOutcome?> SearchAsync(
        string rootPath, string queryText, CancellationToken ct,
        IProgress<IReadOnlyList<SearchHit>>? liveBatches = null, bool includeHidden = true)
    {
        var query = SearchQuery.Parse(queryText);
        if (query is null)
            return null;

        var rootKey = PathKey.Canonicalize(rootPath);
        var covering = await Task.Run(() => _repository.FindCoveringRoot(rootKey), ct).ConfigureAwait(false);

        if (covering is not null)
        {
            // Fresh = crawl completed, nothing flagged stale, and a live watcher is
            // patching changes. Watchers are in-memory, so the first search each app
            // session deliberately lands on the stale path: instant cached results
            // plus one background re-crawl that re-attaches the watcher.
            var fresh = !covering.Stale && _watchers.IsWatching(covering.PathKey);
            if (!fresh)
                EnsureIndexed(covering.PathKey, covering.DisplayPath);

            var (hits, truncated) = await Task.Run(
                () => _repository.Search(rootPath, query, MaxResults, includeHidden), ct).ConfigureAwait(false);
            return new SearchOutcome(
                hits, truncated,
                fresh ? SearchResultSource.Index : SearchResultSource.StaleIndex,
                RefreshPending: !fresh);
        }

        EnsureIndexed(rootKey, PathKey.NormalizeDisplay(rootPath));
        return await Task.Run(() => LiveScan(rootPath, query, ct, liveBatches, includeHidden), ct).ConfigureAwait(false);
    }

    private SearchOutcome LiveScan(
        string rootPath, SearchQuery query, CancellationToken ct,
        IProgress<IReadOnlyList<SearchHit>>? liveBatches, bool includeHidden)
    {
        var rootDisplay = PathKey.NormalizeDisplay(rootPath);
        var hits = new List<SearchHit>();
        var batch = new List<SearchHit>();
        var truncated = false;
        var sinceFlush = Stopwatch.StartNew();

        FileSystemWalker.Walk(rootPath, entry =>
        {
            if (!query.Matches(entry.Name))
                return true;
            if (hits.Count >= MaxResults)
            {
                truncated = true;
                return false; // stop scanning for hits; the background crawl keeps indexing
            }

            var relDir = Path.GetRelativePath(rootDisplay, Path.GetDirectoryName(entry.DisplayPath) ?? rootDisplay);
            if (relDir == ".") relDir = "";

            var hit = new SearchHit(
                entry.DisplayPath, relDir, entry.Name, entry.IsDirectory, entry.SizeBytes, entry.ModifiedUtc, entry.Hidden);
            hits.Add(hit);
            batch.Add(hit);

            if (batch.Count >= LiveBatchSize || sinceFlush.Elapsed >= LiveBatchInterval)
            {
                liveBatches?.Report(batch.ToArray());
                batch.Clear();
                sinceFlush.Restart();
            }
            return true;
        }, ct, includeHidden);

        if (batch.Count > 0)
            liveBatches?.Report(batch.ToArray());

        return new SearchOutcome(hits, truncated, SearchResultSource.LiveScan, RefreshPending: true);
    }

    /// <summary>
    /// Kicks off a background crawl of <paramref name="rootKey"/> unless one is
    /// already in flight (single-flight per root — typing more characters never
    /// restarts the crawl). Runs on the service lifetime, not any search's token.
    /// The watcher attaches before crawling starts so no change event is missed;
    /// crawl_gen stamping makes concurrent watcher writes safe.
    /// </summary>
    private void EnsureIndexed(string rootKey, string displayPath)
    {
        _activeCrawls.GetOrAdd(rootKey, key => Task.Run(async () =>
        {
            try
            {
                _watchers.Watch(key, displayPath);
                var completed = await _crawler.CrawlAsync(displayPath, _lifetime.Token).ConfigureAwait(false);
                if (completed)
                    IndexRefreshed?.Invoke(key);
            }
            catch (Exception)
            {
                // Background index build is best-effort; searches fall back to live scans.
            }
            finally
            {
                _activeCrawls.TryRemove(key, out _);
            }
        }));
    }

    public void Dispose() => _lifetime.Cancel();
}
