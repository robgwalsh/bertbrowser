using BenchmarkDotNet.Attributes;
using BertBrowser.Bench.Fakes;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Mft;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// Listing a folder of ten thousand files, bare and through the archive-aware decorator the app
/// actually registers — which probes <c>File.Exists</c> for every path segment that could be a container.
/// </summary>
[AllocationTolerance(2)]
public class FileSystemBenchmarks
{
    private FileSystemService _bare = null!;
    private ArchiveAwareFileSystemService _decorated = null!;
    private string _folder = "";

    [GlobalSetup]
    public void Setup()
    {
        var context = BenchContext.FromEnvironment();
        _folder = context.Real?.ListingFolder ?? context.Tree.Flat;
        _bare = new FileSystemService();
        _decorated = new ArchiveAwareFileSystemService(_bare, new SharpCompressArchiveReader());
        _bare.ListDirectory(_folder);
    }

    [Benchmark]
    public IReadOnlyList<FileEntry> ListDirectory() => _bare.ListDirectory(_folder);

    [Benchmark]
    public IReadOnlyList<FileEntry> ListDirectoryArchiveAware() => _decorated.ListDirectory(_folder);
}

/// <summary>The flat branch view's listing (Ctrl+B): every file under the root, from the disk, into one list.</summary>
[AllocationTolerance(5)]
public class SearchServiceBenchmarks
{
    [Params(1_000, 100_000)]
    public int Cap { get; set; }

    private string _dbPath = "";
    private SearchService _service = null!;
    private string _root = "";

    [GlobalSetup]
    public void Setup()
    {
        var context = BenchContext.FromEnvironment();
        _root = context.Real?.TreeFolder ?? context.Tree.Deep;

        _dbPath = BenchContext.ScratchDbPath();
        var db = new Db(_dbPath);
        db.Migrate();
        var repo = new FsIndexRepository(db);
        _service = new SearchService(repo, new IndexCrawler(repo), new NoWatchers(), new NullMftIndexService());
    }

    [Benchmark]
    public Task<SearchOutcome> ListSubtree() => _service.ListSubtreeAsync(_root, Cap, CancellationToken.None);

    [GlobalCleanup]
    public void Cleanup()
    {
        _service.Dispose();
        SqliteConnection.ClearAllPools();
        BenchContext.DeleteDb(_dbPath);
    }
}

/// <summary>The fallback crawler that indexes a root the MFT does not cover: walk the tree, upsert it
/// in 20,000-row chunks, sweep, register the root. The index is emptied before every iteration.</summary>
[AllocationTolerance(5)]
public class CrawlerBenchmarks
{
    private string _dbPath = "";
    private Db _db = null!;
    private IndexCrawler _crawler = null!;
    private string _root = "";

    [GlobalSetup]
    public void Setup()
    {
        var context = BenchContext.FromEnvironment();
        _root = context.Real?.TreeFolder ?? context.Tree.Deep;

        _dbPath = BenchContext.ScratchDbPath();
        _db = new Db(_dbPath);
        _db.Migrate();
        _crawler = new IndexCrawler(new FsIndexRepository(_db));
    }

    [IterationSetup]
    public void EmptyIndex()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "DELETE FROM fs_entry; DELETE FROM fs_index_root;";
        cmd.ExecuteNonQuery();
    }

    [Benchmark]
    public Task<bool> Crawl() => _crawler.CrawlAsync(_root, CancellationToken.None);

    [GlobalCleanup]
    public void Cleanup()
    {
        SqliteConnection.ClearAllPools();
        BenchContext.DeleteDb(_dbPath);
    }
}
