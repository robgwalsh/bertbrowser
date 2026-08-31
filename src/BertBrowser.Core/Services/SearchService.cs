using System.Collections.Concurrent;
using System.Diagnostics;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Mft;
using BertBrowser.Core.Services.Search;

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
    /// <param name="contentProgress">
    /// How far a <c>content:</c> pass has got. A separate channel from
    /// <paramref name="liveBatches"/> because it answers a different question — that one carries
    /// what was found, this one carries how much is left to look at, which is the only thing worth
    /// saying during the tens of seconds a whole-tree grep can take.
    /// </param>
    Task<SearchOutcome?> SearchAsync(
        string rootPath, string queryText, CancellationToken ct,
        IProgress<IReadOnlyList<SearchHit>>? liveBatches = null, bool includeHidden = true,
        IProgress<ContentScanProgress>? contentProgress = null);

    /// <summary>
    /// Whole-PC search across every MFT-indexed volume. Returns null when the query is too
    /// short. Results are served straight from the index; while the MFT build is still in
    /// flight they are partial and <c>RefreshPending</c> is set (the caller re-queries when
    /// <c>IMftIndexService.IndexRefreshed</c> fires).
    /// </summary>
    /// <remarks>
    /// It gained the two streaming channels when <c>content:</c> did: the index answers a whole-PC
    /// query in milliseconds and needed neither, but a content term turns the same query into tens
    /// of seconds of reading, and a window that showed nothing for that long would read as hung.
    /// </remarks>
    Task<SearchOutcome?> SearchAllAsync(
        string queryText, CancellationToken ct, bool includeHidden = true,
        IProgress<IReadOnlyList<SearchHit>>? liveBatches = null,
        IProgress<ContentScanProgress>? contentProgress = null);

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

    /// <summary>How many containers one <c>in:archives</c> pass will open.</summary>
    private const int MaxArchivesScanned = 200;

    /// <summary>And how many bytes of them, whichever bound is reached first.</summary>
    private const long MaxArchiveBytesScanned = 2L * 1024 * 1024 * 1024;
    private const int LiveBatchSize = 50;
    private static readonly TimeSpan LiveBatchInterval = TimeSpan.FromMilliseconds(250);

    private readonly FsIndexRepository _repository;
    private readonly IndexCrawler _crawler;
    private readonly IIndexWatcherService _watchers;
    private readonly IMftIndexService _mft;
    private readonly ConcurrentDictionary<string, Task> _activeCrawls = new(StringComparer.Ordinal);
    private readonly CancellationTokenSource _lifetime = new();

    public event Action<string>? IndexRefreshed;

    /// <summary>Only consulted when a query says <c>in:archives</c>; null leaves that a no-op.</summary>
    private readonly Archives.IArchiveBrowser? _archives;

    /// <summary>The <c>content:</c> reading pass. Injectable for the reason the hasher is.</summary>
    private readonly ContentScanner _content;

    /// <summary>
    /// How much of each file to read, asked fresh each search.
    /// </summary>
    /// <remarks>
    /// A delegate rather than a value because this is a singleton and the setting can change while
    /// it lives — the same reason the browse settings are read at the point of use rather than
    /// captured. Core never sees <c>AppSettings</c>; the composition root closes over it, exactly
    /// as <c>includeHidden</c> travels as a parameter.
    /// </remarks>
    private readonly Func<long>? _contentBudget;

    public SearchService(
        FsIndexRepository repository, IndexCrawler crawler, IIndexWatcherService watchers,
        IMftIndexService mft, Archives.IArchiveBrowser? archives = null,
        IContentReader? contentReader = null, Func<long>? contentBudget = null)
    {
        _contentBudget = contentBudget;
        _archives = archives;
        _repository = repository;
        _crawler = crawler;
        _watchers = watchers;
        _mft = mft;
        _content = new ContentScanner(contentReader ?? new FileSystemContentReader());
    }

    /// <summary>
    /// How many rows the first pass asks for.
    /// </summary>
    /// <remarks>
    /// A content query wants <em>candidates</em>, not results. Using the 1,000-result cap would
    /// mean never grepping more than a thousand files — and most of those are about to be thrown
    /// away, since the whole point of the pass is that their names did not settle them. The larger
    /// number costs nothing extra to fetch: the rows come off a scan that was happening anyway.
    /// </remarks>
    private static int Cap(SearchQuery query) =>
        query.NeedsContent ? ContentSearchRules.MaxCandidates : MaxResults;

    /// <summary>
    /// Runs the reading pass over a first-pass result, or returns it untouched.
    /// </summary>
    /// <remarks>
    /// Deliberately takes the <em>visible</em> hits, after <see cref="Visible"/> has dropped
    /// anything sitting in a delete's holding folder or the Recycle Bin. Those files are still on
    /// disk, and opening them would spend the file ceiling on things the user has already deleted —
    /// on a whole-PC search, <c>$Recycle.Bin</c> sorts early enough to eat a good deal of it.
    /// </remarks>
    private (IReadOnlyList<SearchHit> Hits, bool Truncated, bool Cancelled, ContentScanReport? Report)
        ApplyContent(
            SearchQuery query, IReadOnlyList<SearchHit> hits, bool truncated,
            IProgress<IReadOnlyList<SearchHit>>? liveBatches,
            IProgress<ContentScanProgress>? contentProgress, CancellationToken ct)
    {
        if (!query.NeedsContent) return (hits, truncated, false, null);

        var budget = _contentBudget?.Invoke() ?? ContentSearchRules.MaxBytesPerFile;
        var scan = _content.Scan(
            query, hits, MaxResults, truncated, budget, liveBatches, contentProgress, ct);
        return (scan.Hits, scan.Truncated, scan.Cancelled, scan.Report);
    }

    /// <summary>Drops hits that live in a delete's holding folder. They are still on disk — that is
    /// the whole point, so Ctrl+Z can put them back — but they have been deleted as far as the user
    /// is concerned, and search saying otherwise reads as a delete that silently failed.</summary>
    private static IReadOnlyList<SearchHit> Visible(IReadOnlyList<SearchHit> hits) =>
        hits.Any(h => DeleteExecutor.IsHeldPath(h.DisplayPath))
            ? hits.Where(h => !DeleteExecutor.IsHeldPath(h.DisplayPath)).ToList()
            : hits;

    /// <summary>
    /// What to return when the text did not yield a runnable query. A parse <em>problem</em> is
    /// an outcome with no hits and a message; "nothing to run" is null, which is what tells the
    /// caller to go back to showing the plain directory listing.
    /// </summary>
    private static SearchOutcome? Refused(SearchQueryParse parse) =>
        parse.Problem is null
            ? null
            : new SearchOutcome([], Truncated: false, SearchResultSource.Index,
                RefreshPending: false, Problem: parse.Problem);

    /// <summary>
    /// Whether this run should surface hidden entries. Search normally excludes them whatever
    /// the browse setting says — hidden entries are index noise (AppData, system junk) that
    /// buries the results a search is for — but a query that explicitly asks for them has to
    /// override that, or <c>is:hidden</c> returns nothing every time and reads as broken.
    /// </summary>
    private static bool ShowHidden(SearchQuery query, bool includeHidden) =>
        includeHidden || query.WantsHidden;

    /// <summary>
    /// Whether an empty result means "nothing matched" or "nothing could have matched": a size
    /// or date filter over an index built by the names-only fallback can never return a row.
    /// Asked only when the answer could change what the user is told — a filtered search that
    /// found nothing — so the scan it may cost is paid once, on a path that already failed.
    /// </summary>
    private async Task<bool> UnansweredForWantOfMetadataAsync(
        SearchQuery query, IReadOnlyList<SearchHit> hits, string? rootPath, CancellationToken ct)
    {
        if (hits.Count > 0 || !query.NeedsMetadata) return false;
        return !await Task.Run(() => _repository.HasSizeData(rootPath), ct).ConfigureAwait(false);
    }

    public async Task<SearchOutcome?> SearchAllAsync(
        string queryText, CancellationToken ct, bool includeHidden = true,
        IProgress<IReadOnlyList<SearchHit>>? liveBatches = null,
        IProgress<ContentScanProgress>? contentProgress = null)
    {
        var parse = SearchQuery.Parse(queryText);
        if (parse.Query is not { } query)
            return Refused(parse);

        var (hits, truncated) = await Task.Run(
            () => _repository.SearchGlobal(query, Cap(query), ShowHidden(query, includeHidden)),
            ct).ConfigureAwait(false);

        var scoped = await UnansweredForWantOfMetadataAsync(query, hits, null, ct).ConfigureAwait(false);

        var (final, capped, cancelled, report) = await Task.Run(
            () => ApplyContent(query, Visible(hits), truncated, liveBatches, contentProgress, ct),
            ct).ConfigureAwait(false);

        // While volumes are still enumerating the results are partial; the ViewModel re-queries
        // on IMftIndexService.IndexRefreshed.
        return new SearchOutcome(
            final, capped, SearchResultSource.Index, RefreshPending: _mft.IsBuilding,
            ScopeLacksMetadata: scoped, Cancelled: cancelled, ContentScan: report);
    }

    public async Task<SearchOutcome?> SearchAsync(
        string rootPath, string queryText, CancellationToken ct,
        IProgress<IReadOnlyList<SearchHit>>? liveBatches = null, bool includeHidden = true,
        IProgress<ContentScanProgress>? contentProgress = null)
    {
        var parse = SearchQuery.Parse(queryText);
        if (parse.Query is not { } query)
            return Refused(parse);

        // A root inside an archive is refused outright, and this is the hard invariant rather than
        // a missing feature. PathKey.Canonicalize accepts a virtual path happily, FindCoveringRoot
        // then misses, and EnsureIndexed would crawl a path that does not exist straight into
        // fs_entry — after which every subtree range scan over the archive's own containing folder
        // returns archive interiors. Searching inside a container is answered from its already
        // loaded index instead; see the Archives section.
        if (Archives.ArchivePath.Parse(rootPath, File.Exists) is not null)
            return new SearchOutcome([], Truncated: false, SearchResultSource.Index,
                RefreshPending: false,
                Problem: "Searching inside an archive is not indexed. Extract it to search its contents.");

        var showHidden = ShowHidden(query, includeHidden);
        var rootKey = PathKey.Canonicalize(rootPath);
        var covering = await Task.Run(() => _repository.FindCoveringRoot(rootKey), ct).ConfigureAwait(false);

        if (covering is not null)
        {
            // Fresh = crawl completed, nothing flagged stale, and something is patching
            // changes live — either the MFT/USN tail (for NTFS volumes) or a FileSystemWatcher
            // (the crawl fallback for other roots). FileSystemWatchers are in-memory, so the
            // first crawl-backed search each session deliberately lands on the stale path:
            // instant cached results plus one background re-crawl that re-attaches the watcher.
            // MFT-covered roots are kept live by the indexer, so they never fall to the crawler.
            var fresh = !covering.Stale
                && (_mft.IsIndexed(covering.PathKey) || _watchers.IsWatching(covering.PathKey));
            if (!fresh)
                EnsureIndexed(covering.PathKey, covering.DisplayPath);

            var (hits, truncated) = await Task.Run(
                () => _repository.Search(rootPath, query, Cap(query), showHidden), ct).ConfigureAwait(false);

            var visible = Visible(hits).ToList();
            var inside = await ScanArchivesAsync(query, rootPath, liveBatches, ct).ConfigureAwait(false);
            visible.AddRange(inside);

            var scoped = await UnansweredForWantOfMetadataAsync(query, hits, rootPath, ct)
                .ConfigureAwait(false);

            var (final, capped, cancelled, report) = await Task.Run(
                () => ApplyContent(query, visible, truncated, liveBatches, contentProgress, ct),
                ct).ConfigureAwait(false);

            return new SearchOutcome(
                final, capped,
                fresh ? SearchResultSource.Index : SearchResultSource.StaleIndex,
                RefreshPending: !fresh,
                ScopeLacksMetadata: scoped, Cancelled: cancelled, ContentScan: report);
        }

        EnsureIndexed(rootKey, PathKey.NormalizeDisplay(rootPath));

        // A content query must not stream the *walk*: those rows are candidates, not results, and
        // most are about to be discarded. Putting them on screen and taking them away again would
        // be worse than a moment of nothing — the reading pass streams what actually matched.
        var outcome = await Task.Run(
            () => LiveScan(rootPath, query, ct, query.NeedsContent ? null : liveBatches, showHidden),
            ct).ConfigureAwait(false);

        var extra = await ScanArchivesAsync(query, rootPath, liveBatches, ct).ConfigureAwait(false);
        var combined = extra.Count == 0
            ? outcome.Hits
            : [.. outcome.Hits, .. extra];

        var scanned = await Task.Run(
            () => ApplyContent(query, combined, outcome.Truncated, liveBatches, contentProgress, ct),
            ct).ConfigureAwait(false);

        return outcome with
        {
            Hits = scanned.Hits,
            Truncated = scanned.Truncated,
            Cancelled = scanned.Cancelled,
            ContentScan = scanned.Report,
        };
    }

    /// <summary>
    /// The <c>in:archives</c> second pass: opens the containers the ordinary pass already found and
    /// searches their entry names.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>Opt-in, and it has to be.</b> A bare word must never open an archive — that is the
    /// compatibility rule the whole query language is bent around — and opening one is orders of
    /// magnitude more expensive than reading an index row. So this runs only when the query said so.
    /// </para>
    /// <para>
    /// <b>It finds the containers itself rather than reusing the first pass's hits</b>, which was
    /// the obvious shortcut and is wrong: the query the user typed describes what they are looking
    /// for <em>inside</em> an archive, and there is no reason it should also match the archive's own
    /// name. Searching for "util" would have found nothing in <c>sample.zip</c> because
    /// <c>sample.zip</c> is not called util.
    /// </para>
    /// <para>
    /// <b>No schema change, and there must not be one.</b> Storing archive contents would mean a
    /// second <c>PathKey</c>-keyed corpus full of virtual paths — the one thing that would poison
    /// every subtree range scan over the folders those containers live in — rebuilt whenever any
    /// archive's timestamp moved. <c>fs_entry</c> is <c>WITHOUT ROWID</c> and has already refused a
    /// secondary index on <c>name</c> and on <c>size_bytes</c> for far less than this would cost.
    /// </para>
    /// <para>
    /// Capped both ways, because the cost is per container and per byte: whichever bound is reached
    /// first stops the pass.
    /// </para>
    /// </remarks>
    private Task<IReadOnlyList<SearchHit>> ScanArchivesAsync(
        SearchQuery query,
        string rootPath,
        IProgress<IReadOnlyList<SearchHit>>? liveBatches,
        CancellationToken ct)
    {
        if (!query.WantsArchives || _archives is null)
            return Task.FromResult<IReadOnlyList<SearchHit>>([]);

        return Task.Run<IReadOnlyList<SearchHit>>(() =>
        {
            var containers = new List<(string Path, long Size)>();

            // One walk for the containers, matching on the name table rather than on the query.
            // Bounded as it goes, so a folder holding ten thousand zips stops at the cap rather
            // than collecting them all and then throwing most away.
            long found = 0;
            FileSystemWalker.Walk(rootPath, entry =>
            {
                if (entry.IsDirectory || !ArchiveFormats.IsArchiveName(entry.Name)) return true;

                containers.Add((entry.DisplayPath, entry.SizeBytes));
                found += entry.SizeBytes;
                return containers.Count < MaxArchivesScanned && found < MaxArchiveBytesScanned;
            }, ct, includeHidden: false);

            var hits = new List<SearchHit>();
            foreach (var (path, _) in containers)
            {
                ct.ThrowIfCancellationRequested();
                if (hits.Count >= MaxResults) break;

                var index = _archives.ReadArchive(path);
                if (!index.Ok) continue;

                var inside = ArchiveSearchScanner.Search(
                    index, path, "", query, MaxResults - hits.Count, ct);
                if (inside.Count == 0) continue;

                hits.AddRange(inside);
                // Straight onto the same stream the live scan uses, so results land in a list that
                // is already on screen and none of the UI needed new plumbing.
                liveBatches?.Report(inside);
            }

            return hits;
        }, ct);
    }

    private SearchOutcome LiveScan(
        string rootPath, SearchQuery query, CancellationToken ct,
        IProgress<IReadOnlyList<SearchHit>>? liveBatches, bool includeHidden)
    {
        // A content query walks for candidates rather than for results, so it stops at the larger
        // ceiling — the reading pass applies the 1,000-result cap afterwards.
        var cap = Cap(query);
        var rootDisplay = PathKey.NormalizeDisplay(rootPath);
        var hits = new List<SearchHit>();
        var batch = new List<SearchHit>();
        var truncated = false;
        var sinceFlush = Stopwatch.StartNew();

        FileSystemWalker.Walk(rootPath, entry =>
        {
            if (!query.Matches(new SearchCandidate(
                    entry.NameKey, entry.PathKey, entry.IsDirectory,
                    entry.SizeBytes, entry.ModifiedUtc, entry.Hidden)))
                return true;
            if (DeleteExecutor.IsHeldPath(entry.DisplayPath))
                return true; // deleted, just not committed yet
            if (hits.Count >= cap)
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
