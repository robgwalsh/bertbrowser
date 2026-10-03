using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// What the flat branch view lists. The fixture is <see cref="SearchServiceTests"/>' — a real temp
/// database and a real directory tree — because the thing under test is a walk of an actual disk.
/// </summary>
public sealed class SubtreeListingTests : IDisposable
{
    private readonly string _dbPath;
    private readonly string _rootDir;
    private readonly FsIndexRepository _repo;
    private readonly SearchService _service;

    public SubtreeListingTests()
    {
        var id = Guid.NewGuid().ToString("N");
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-flat-{id}.db");
        _rootDir = Path.Combine(Path.GetTempPath(), $"bertbrowser-flattree-{id}");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new FsIndexRepository(db);
        _service = new SearchService(_repo, new IndexCrawler(_repo), new FakeWatchers(), new NullMftIndexService());
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

    private string CreateFile(string relative)
    {
        var full = Path.Combine(_rootDir, relative);
        Directory.CreateDirectory(Path.GetDirectoryName(full)!);
        File.WriteAllBytes(full, new byte[1]);
        return full;
    }

    private Task<SearchOutcome> List(
        bool includeHidden = false,
        int cap = 1000, IProgress<IReadOnlyList<SearchHit>>? batches = null) =>
        _service.ListSubtreeAsync(_rootDir, cap, CancellationToken.None, batches, includeHidden);

    private sealed class FakeWatchers : IIndexWatcherService
    {
        public bool IsWatching(string rootKey) => false;
        public void Watch(string rootKey, string displayPath) { }
        public void StopAll() { }
        public void Dispose() { }
    }

    private sealed class CollectingProgress : IProgress<IReadOnlyList<SearchHit>>
    {
        private readonly List<SearchHit> _hits = new();
        public IReadOnlyList<SearchHit> Hits { get { lock (_hits) return _hits.ToList(); } }
        public void Report(IReadOnlyList<SearchHit> value) { lock (_hits) _hits.AddRange(value); }
    }

    [Fact]
    public async Task ListsDescendantsAtEveryDepth()
    {
        CreateFile("top.txt");
        CreateFile(@"Sub\middle.txt");
        CreateFile(@"Sub\Deep\bottom.txt");

        var outcome = await List();

        // The whole point: a one-level listing can only ever show top.txt.
        Assert.Equal(
            new[] { "bottom.txt", "middle.txt", "top.txt" },
            outcome.Hits.Select(h => h.Name).Order().ToArray());
        Assert.False(outcome.Truncated);
    }

    [Fact]
    public async Task RelativeDirIsEmptyForADirectChildAndTheSubpathBelow()
    {
        // The Folder column's contract. Everything a flat list is readable at all by is this string.
        CreateFile("top.txt");
        CreateFile(@"Sub\Deep\bottom.txt");

        var hits = (await List()).Hits.ToDictionary(h => h.Name);

        Assert.Equal("", hits["top.txt"].RelativeDirDisplay);
        Assert.Equal(Path.Combine("Sub", "Deep"), hits["bottom.txt"].RelativeDirDisplay);
    }

    [Fact]
    public async Task DescendsIntoTheFoldersItDoesNotShow()
    {
        CreateFile(@"Sub\Deep\bottom.txt");

        var files = await List();

        Assert.Equal(["bottom.txt"], files.Hits.Select(h => h.Name));
        Assert.DoesNotContain(files.Hits, h => h.IsDirectory);
    }

    [Fact]
    public async Task HiddenDropsTheEntryAndEverythingUnderAHiddenFolder()
    {
        CreateFile("visible.txt");
        var secret = CreateFile("secret.txt");
        var buried = CreateFile(@"Private\buried.txt");
        File.SetAttributes(secret, File.GetAttributes(secret) | FileAttributes.Hidden);
        var dir = Path.Combine(_rootDir, "Private");
        File.SetAttributes(dir, File.GetAttributes(dir) | FileAttributes.Hidden);

        var without = await List(includeHidden: false);
        var with = await List(includeHidden: true);

        Assert.Equal(["visible.txt"], without.Hits.Select(h => h.Name));
        Assert.Contains(with.Hits, h => h.Name == "secret.txt");
        Assert.Contains(with.Hits, h => h.Name == Path.GetFileName(buried)); // the whole subtree, not just the folder
    }

    [Fact]
    public async Task StopsAtTheCapAndSaysSo()
    {
        for (var i = 0; i < 12; i++) CreateFile($"file{i:00}.txt");

        var outcome = await List(cap: 5);

        Assert.True(outcome.Truncated);
        Assert.Equal(5, outcome.Hits.Count);
    }

    [Fact]
    public async Task EverythingStreamedIsAlsoInTheFinalList()
    {
        // Nothing appears mid-listing that the finished listing then contradicts, and nothing the
        // finished listing holds was withheld from the stream.
        for (var i = 0; i < 30; i++) CreateFile($"Sub{i % 3}\\file{i:00}.txt");

        var progress = new CollectingProgress();
        var outcome = await List(batches: progress);

        Assert.Equal(
            outcome.Hits.Select(h => h.DisplayPath).Order(),
            progress.Hits.Select(h => h.DisplayPath).Order());
    }

    [Fact]
    public async Task AFileHeldByAnUndoableDeleteIsNotListed()
    {
        // Still on disk — that is what makes the delete undoable — but deleted as far as anyone
        // looking at the list is concerned.
        CreateFile("kept.txt");
        CreateFile(Path.Combine(DeleteExecutor.TrashFolderName, "gone.txt"));

        var hits = (await List(includeHidden: true)).Hits;

        Assert.Contains(hits, h => h.Name == "kept.txt");
        Assert.DoesNotContain(hits, h => h.Name == "gone.txt");
    }

    [Fact]
    public async Task AnArchiveRootIsRefusedRatherThanWalked()
    {
        // The invariant, not the feature: a virtual path that reached a PathKey-keyed table would
        // land inside PrefixBounds of the container's own folder and poison every subtree scan
        // over it. An interior is listed from the loaded ArchiveIndex instead.
        var zip = CreateFile("bundle.zip");

        var outcome = await _service.ListSubtreeAsync(
            Path.Combine(zip, "inside"), cap: 1000, CancellationToken.None);

        Assert.NotNull(outcome.Problem);
        Assert.Empty(outcome.Hits);
    }

    [Fact]
    public async Task ListingNeverEnrolsTheFolderInTheIndex()
    {
        // The other promise in ListSubtreeAsync's remarks. This is the test that goes red if
        // someone helpfully adds an EnsureIndexed call to make the second press faster.
        CreateFile(@"Sub\file.txt");

        await List();
        await Task.Delay(250); // a crawl, if one had been started, is single-flight and async

        Assert.Null(_repo.FindCoveringRoot(Core.Paths.PathKey.Canonicalize(_rootDir)));
    }
}
