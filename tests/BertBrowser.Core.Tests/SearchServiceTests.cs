using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class SearchServiceTests : IDisposable
{
    private readonly string _dbPath;
    private readonly string _rootDir;
    private readonly FsIndexRepository _repo;
    private readonly IndexCrawler _crawler;
    private readonly FakeWatcherService _watchers = new();
    private readonly FakeMftIndexService _mft = new();
    private readonly SearchService _service;

    public SearchServiceTests()
    {
        var id = Guid.NewGuid().ToString("N");
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{id}.db");
        _rootDir = Path.Combine(Path.GetTempPath(), $"bertbrowser-tree-{id}");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new FsIndexRepository(db);
        _crawler = new IndexCrawler(_repo);
        _service = new SearchService(_repo, _crawler, _watchers, _mft);
    }

    public void Dispose()
    {
        _service.Dispose();
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
        if (Directory.Exists(_rootDir))
            Directory.Delete(_rootDir, recursive: true);
    }

    private void CreateFile(string relative)
    {
        var full = Path.Combine(_rootDir, relative);
        Directory.CreateDirectory(Path.GetDirectoryName(full)!);
        File.WriteAllBytes(full, new byte[1]);
    }

    private sealed class FakeWatcherService : IIndexWatcherService
    {
        private readonly HashSet<string> _watching = new(StringComparer.Ordinal);
        public bool IsWatching(string rootKey) { lock (_watching) return _watching.Contains(rootKey); }
        public void Watch(string rootKey, string displayPath) { lock (_watching) _watching.Add(rootKey); }
        public void StopAll() { lock (_watching) _watching.Clear(); }
        public void Dispose() { }
    }

    /// <summary>No NTFS volumes in tests — this stand-in reports nothing indexed, so the
    /// crawl/live-scan fallback paths are exercised exactly as before the MFT feature.</summary>
    private sealed class FakeMftIndexService : IMftIndexService
    {
        public void Start() { }
        public bool AnyIndexed => false;
        public bool IsBuilding => false;
        public bool IsIndexed(string pathKey) => false;
        public string StatusText => "";
        public event Action<string>? IndexRefreshed { add { } remove { } }
        public event Action? StatusChanged { add { } remove { } }
        public void Dispose() { }
    }

    /// <summary>Synchronous collector — Progress&lt;T&gt; would race the test via the sync context.</summary>
    private sealed class CollectingProgress : IProgress<IReadOnlyList<SearchHit>>
    {
        private readonly List<SearchHit> _hits = new();
        public IReadOnlyList<SearchHit> Hits { get { lock (_hits) return _hits.ToList(); } }
        public void Report(IReadOnlyList<SearchHit> value) { lock (_hits) _hits.AddRange(value); }
    }

    private Task<string> NextIndexRefreshedAsync()
    {
        var tcs = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        _service.IndexRefreshed += key => tcs.TrySetResult(key);
        return tcs.Task;
    }

    private static async Task<T> WithTimeout<T>(Task<T> task)
    {
        var winner = await Task.WhenAny(task, Task.Delay(TimeSpan.FromSeconds(30)));
        Assert.Same(task, winner);
        return await task;
    }

    [Fact]
    public async Task TooShortQuery_ReturnsNull()
    {
        Directory.CreateDirectory(_rootDir);
        Assert.Null(await _service.SearchAsync(_rootDir, "a", CancellationToken.None));
        Assert.Null(await _service.SearchAsync(_rootDir, "", CancellationToken.None));
    }

    [Fact]
    public async Task Uncovered_StreamsLiveScanAndIndexesInBackground()
    {
        CreateFile(@"Report.docx");
        CreateFile(@"Sub\Report-2026.pdf");
        CreateFile(@"Sub\notes.txt");

        var refreshed = NextIndexRefreshedAsync();
        var progress = new CollectingProgress();

        var outcome = await _service.SearchAsync(_rootDir, "report", CancellationToken.None, progress);

        Assert.NotNull(outcome);
        Assert.Equal(SearchResultSource.LiveScan, outcome!.Source);
        Assert.True(outcome.RefreshPending);
        Assert.Equal(2, outcome.Hits.Count);
        Assert.Equal(outcome.Hits.Count, progress.Hits.Count); // everything streamed

        // The lazy background crawl completes and registers the root.
        var refreshedKey = await WithTimeout(refreshed);
        Assert.Equal(BertBrowser.Core.Paths.PathKey.Canonicalize(_rootDir), refreshedKey);
        Assert.NotNull(_repo.FindCoveringRoot(_rootDir));

        // Second search: watcher attached during EnsureIndexed → fresh index hit.
        var second = await _service.SearchAsync(_rootDir, "report", CancellationToken.None);
        Assert.Equal(SearchResultSource.Index, second!.Source);
        Assert.False(second.RefreshPending);
        Assert.Equal(2, second.Hits.Count);
    }

    [Fact]
    public async Task CoveredButUnwatched_ServesStaleIndexAndRevalidates()
    {
        CreateFile(@"cached.txt");
        await _crawler.CrawlAsync(_rootDir, CancellationToken.None); // indexed, but no watcher attached

        var refreshed = NextIndexRefreshedAsync();
        var outcome = await _service.SearchAsync(_rootDir, "cached", CancellationToken.None);

        Assert.Equal(SearchResultSource.StaleIndex, outcome!.Source); // instant results from cache
        Assert.True(outcome.RefreshPending);
        Assert.Single(outcome.Hits);

        await WithTimeout(refreshed); // background re-crawl attached the watcher

        var second = await _service.SearchAsync(_rootDir, "cached", CancellationToken.None);
        Assert.Equal(SearchResultSource.Index, second!.Source);
    }

    [Fact]
    public async Task SearchUnderCoveredRoot_UsesIndexWithSubtreeScope()
    {
        CreateFile(@"Sub\inner.txt");
        CreateFile(@"outer.txt");
        await _crawler.CrawlAsync(_rootDir, CancellationToken.None);
        _watchers.Watch(BertBrowser.Core.Paths.PathKey.Canonicalize(_rootDir), _rootDir);

        // Searching a subfolder of the indexed root: fresh index, scoped to the subtree.
        var outcome = await _service.SearchAsync(Path.Combine(_rootDir, "Sub"), "txt", CancellationToken.None);

        Assert.Equal(SearchResultSource.Index, outcome!.Source);
        Assert.Equal("inner.txt", Assert.Single(outcome.Hits).Name);
    }
}
