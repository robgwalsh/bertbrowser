using BertBrowser.Core.Data;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class IndexCrawlerTests : IDisposable
{
    private readonly string _dbPath;
    private readonly string _rootDir;
    private readonly FsIndexRepository _repo;
    private readonly IndexCrawler _crawler;

    public IndexCrawlerTests()
    {
        var id = Guid.NewGuid().ToString("N");
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{id}.db");
        _rootDir = Path.Combine(Path.GetTempPath(), $"bertbrowser-tree-{id}");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new FsIndexRepository(db);
        _crawler = new IndexCrawler(_repo);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
        if (Directory.Exists(_rootDir))
        {
            foreach (var dir in Directory.EnumerateDirectories(_rootDir, "*", SearchOption.AllDirectories)
                         .Where(d => (File.GetAttributes(d) & FileAttributes.ReparsePoint) != 0)
                         .ToList())
            {
                Directory.Delete(dir, recursive: false);
            }
            Directory.Delete(_rootDir, recursive: true);
        }
    }

    private void CreateFile(string relative, int bytes = 1)
    {
        var full = Path.Combine(_rootDir, relative);
        Directory.CreateDirectory(Path.GetDirectoryName(full)!);
        File.WriteAllBytes(full, new byte[bytes]);
    }

    private static SearchQuery Q(string text) => SearchQuery.Parse(text)!;

    [Fact]
    public async Task Crawl_IndexesFilesAndDirectoriesRecursively()
    {
        CreateFile(@"Alpha.txt", 100);
        CreateFile(@"Sub\Beta.txt");
        CreateFile(@"Sub\Deep\Gamma.bin");

        var completed = await _crawler.CrawlAsync(_rootDir, CancellationToken.None);
        Assert.True(completed);

        var root = _repo.FindCoveringRoot(_rootDir);
        Assert.NotNull(root);
        Assert.True(root!.Complete);
        Assert.False(root.Stale);

        var (alpha, _) = _repo.Search(_rootDir, Q("alpha"), 100);
        Assert.Equal(100, Assert.Single(alpha).SizeBytes);

        var (gamma, _) = _repo.Search(_rootDir, Q("gamma"), 100);
        Assert.Equal(@"Sub\Deep", Assert.Single(gamma).RelativeDirDisplay);

        // Directories are indexed too.
        var (dirs, _) = _repo.Search(_rootDir, Q("deep"), 100);
        Assert.True(Assert.Single(dirs).IsDirectory);
    }

    [Fact]
    public async Task Crawl_PropagatesHiddenIntoSubtree()
    {
        CreateFile(@"Visible\keep.txt");
        CreateFile(@"Secret\inside.txt"); // "inside" is not itself hidden…
        // …but its parent folder is, so it must count as hidden.
        var secret = Path.Combine(_rootDir, "Secret");
        File.SetAttributes(secret, File.GetAttributes(secret) | FileAttributes.Hidden);

        Assert.True(await _crawler.CrawlAsync(_rootDir, CancellationToken.None));

        // With hidden included, the file inside the hidden folder is found and flagged.
        var (withHidden, _) = _repo.Search(_rootDir, Q("inside"), 100);
        Assert.True(Assert.Single(withHidden).Hidden);

        // With hidden excluded, nothing under the hidden folder surfaces…
        Assert.Empty(_repo.Search(_rootDir, Q("inside"), 100, includeHidden: false).Hits);
        // …while a sibling under a visible folder still does.
        Assert.Single(_repo.Search(_rootDir, Q("keep"), 100, includeHidden: false).Hits);
    }

    [Fact]
    public async Task Crawl_EmitsJunctionButDoesNotDescend()
    {
        CreateFile(@"real\inner.bin");
        var junction = Path.Combine(_rootDir, "junction");
        var psi = new System.Diagnostics.ProcessStartInfo("cmd.exe",
            $"/c mklink /J \"{junction}\" \"{Path.Combine(_rootDir, "real")}\"")
        {
            CreateNoWindow = true,
            UseShellExecute = false,
        };
        using (var p = System.Diagnostics.Process.Start(psi)!)
            p.WaitForExit();
        Assert.True(Directory.Exists(junction), "junction was not created");

        await _crawler.CrawlAsync(_rootDir, CancellationToken.None);

        // The junction itself is searchable…
        var (junctions, _) = _repo.Search(_rootDir, Q("junction"), 100);
        Assert.True(Assert.Single(junctions).IsDirectory);

        // …but its contents were not double-indexed through it.
        var (inner, _) = _repo.Search(_rootDir, Q("inner"), 100);
        Assert.Equal("real", Assert.Single(inner).RelativeDirDisplay);
    }

    [Fact]
    public async Task Crawl_Cancelled_DoesNotRegisterRoot()
    {
        CreateFile(@"a.txt");
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        var completed = await _crawler.CrawlAsync(_rootDir, cts.Token);

        Assert.False(completed);
        Assert.Null(_repo.FindCoveringRoot(_rootDir));
    }

    [Fact]
    public async Task Recrawl_SweepsVanishedEntries()
    {
        CreateFile(@"keep.txt");
        CreateFile(@"gone.txt");
        await _crawler.CrawlAsync(_rootDir, CancellationToken.None);
        Assert.Single(_repo.Search(_rootDir, Q("gone"), 100).Hits);

        File.Delete(Path.Combine(_rootDir, "gone.txt"));
        await _crawler.CrawlAsync(_rootDir, CancellationToken.None);

        Assert.Empty(_repo.Search(_rootDir, Q("gone"), 100).Hits);
        Assert.Single(_repo.Search(_rootDir, Q("keep"), 100).Hits);
    }
}
