using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Exercises the watcher's event-application logic directly (internal Apply) —
/// real FileSystemWatcher tests are timing-flaky, and the interesting behavior
/// is how events mutate the index.
/// </summary>
public sealed class IndexWatcherApplyTests : IDisposable
{
    private readonly string _dbPath;
    private readonly string _rootDir;
    private readonly string _rootKey;
    private readonly FsIndexRepository _repo;
    private readonly IndexWatcherService _service;

    public IndexWatcherApplyTests()
    {
        var id = Guid.NewGuid().ToString("N");
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{id}.db");
        _rootDir = Path.Combine(Path.GetTempPath(), $"bertbrowser-tree-{id}");
        Directory.CreateDirectory(_rootDir);
        _rootKey = PathKey.Canonicalize(_rootDir);
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new FsIndexRepository(db);
        _service = new IndexWatcherService(_repo);
        _repo.UpsertRoot(_rootKey, _rootDir, DateTime.UtcNow, complete: true);
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

    private static SearchQuery Q(string text) => SearchQuery.Parse(text)!;

    private void Apply(WatcherChangeTypes type, string fullPath, string? oldFullPath = null) =>
        _service.Apply(new[] { new IndexWatcherService.FsChange(_rootKey, type, fullPath, oldFullPath) });

    [Fact]
    public void CreatedFile_IsIndexed()
    {
        var file = Path.Combine(_rootDir, "fresh.txt");
        File.WriteAllBytes(file, new byte[7]);

        Apply(WatcherChangeTypes.Created, file);

        var hit = Assert.Single(_repo.Search(_rootDir, Q("fresh"), 100).Hits);
        Assert.Equal(7, hit.SizeBytes);
        Assert.False(hit.IsDirectory);
    }

    [Fact]
    public void CreatedDirectory_MiniCrawlsItsContents()
    {
        // A folder moved into the tree raises one Created event for the top dir only.
        var moved = Path.Combine(_rootDir, "Moved");
        Directory.CreateDirectory(Path.Combine(moved, "Nested"));
        File.WriteAllBytes(Path.Combine(moved, "Nested", "payload.bin"), new byte[1]);

        Apply(WatcherChangeTypes.Created, moved);

        Assert.Single(_repo.Search(_rootDir, Q("moved"), 100).Hits);
        var payload = Assert.Single(_repo.Search(_rootDir, Q("payload"), 100).Hits);
        Assert.Equal(@"Moved\Nested", payload.RelativeDirDisplay);
    }

    [Fact]
    public void DeletedEntry_RemovesSubtree()
    {
        // Deletion applies to the index only — the entry is already gone from disk.
        var subKey = PathKey.Canonicalize(Path.Combine(_rootDir, "Sub"));
        _repo.UpsertEntries(new[]
        {
            new FsEntryRow(subKey, "Sub", true, 0, DateTime.UtcNow),
            new FsEntryRow(subKey + @"\A.TXT", "a.txt", false, 1, DateTime.UtcNow),
        }, crawlGen: 1);

        Apply(WatcherChangeTypes.Deleted, Path.Combine(_rootDir, "Sub"));

        Assert.Empty(_repo.Search(_rootDir, Q("a.txt"), 100).Hits);
        Assert.Empty(_repo.Search(_rootDir, Q("sub"), 100).Hits);
    }

    [Fact]
    public void RenamedDirectory_RewritesDescendants()
    {
        var oldKey = PathKey.Canonicalize(Path.Combine(_rootDir, "Old"));
        _repo.UpsertEntries(new[]
        {
            new FsEntryRow(oldKey, "Old", true, 0, DateTime.UtcNow),
            new FsEntryRow(oldKey + @"\DOC.PDF", "doc.pdf", false, 1, DateTime.UtcNow),
        }, crawlGen: 1);

        Apply(WatcherChangeTypes.Renamed,
            Path.Combine(_rootDir, "New"),
            oldFullPath: Path.Combine(_rootDir, "Old"));

        var hit = Assert.Single(_repo.Search(_rootDir, Q("doc.pdf"), 100).Hits);
        Assert.Equal("New", hit.RelativeDirDisplay);
        Assert.Empty(_repo.Search(Path.Combine(_rootDir, "Old"), Q("doc"), 100).Hits);
    }

    [Fact]
    public void ChangedFile_UpdatesSizeAndTimestamp()
    {
        var file = Path.Combine(_rootDir, "grow.txt");
        File.WriteAllBytes(file, new byte[3]);
        Apply(WatcherChangeTypes.Created, file);

        File.WriteAllBytes(file, new byte[9]);
        Apply(WatcherChangeTypes.Changed, file);

        Assert.Equal(9, Assert.Single(_repo.Search(_rootDir, Q("grow"), 100).Hits).SizeBytes);
    }

    [Fact]
    public void CreatedThenVanished_TreatedAsDelete()
    {
        // Temp-file churn: the Created event arrives after the file is already gone.
        var file = Path.Combine(_rootDir, "ephemeral.tmp");
        var key = PathKey.Canonicalize(file);
        _repo.UpsertEntries(new[]
        {
            new FsEntryRow(key, "ephemeral.tmp", false, 1, DateTime.UtcNow),
        }, crawlGen: 1);

        Apply(WatcherChangeTypes.Created, file); // file does not exist on disk

        Assert.Empty(_repo.Search(_rootDir, Q("ephemeral"), 100).Hits);
    }
}
