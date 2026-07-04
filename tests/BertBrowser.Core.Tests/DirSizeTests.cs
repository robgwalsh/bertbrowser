using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class DirSizeTests : IDisposable
{
    private readonly string _dbPath;
    private readonly string _rootDir;
    private readonly DirSizeRepository _repo;
    private readonly DirectorySizeService _service;

    public DirSizeTests()
    {
        var id = Guid.NewGuid().ToString("N");
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{id}.db");
        _rootDir = Path.Combine(Path.GetTempPath(), $"bertbrowser-tree-{id}");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new DirSizeRepository(db);
        _service = new DirectorySizeService(_repo);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
        if (Directory.Exists(_rootDir))
        {
            // Recursive delete chokes on junctions; remove reparse points non-recursively first.
            foreach (var dir in Directory.EnumerateDirectories(_rootDir, "*", SearchOption.AllDirectories)
                         .Where(d => (File.GetAttributes(d) & FileAttributes.ReparsePoint) != 0)
                         .ToList())
            {
                Directory.Delete(dir, recursive: false);
            }
            Directory.Delete(_rootDir, recursive: true);
        }
    }

    private void CreateFile(string relative, int bytes)
    {
        var full = Path.Combine(_rootDir, relative);
        Directory.CreateDirectory(Path.GetDirectoryName(full)!);
        File.WriteAllBytes(full, new byte[bytes]);
    }

    [Fact]
    public async Task Compute_SumsRecursivelyAndCachesEveryDescendant()
    {
        CreateFile(@"a.bin", 100);
        CreateFile(@"sub\b.bin", 200);
        CreateFile(@"sub\deep\c.bin", 300);
        CreateFile(@"sub2\d.bin", 50);

        var result = await _service.ComputeAsync(_rootDir, CancellationToken.None);

        Assert.NotNull(result);
        Assert.Equal(650, result!.SizeBytes);
        Assert.Equal(4, result.FileCount);
        Assert.Equal(3, result.DirCount);
        Assert.False(result.Incomplete);

        // Every descendant directory was cached by the single scan.
        var sub = _repo.Get(Path.Combine(_rootDir, "sub"));
        Assert.NotNull(sub);
        Assert.Equal(500, sub!.SizeBytes);
        Assert.Equal(2, sub.FileCount);
        Assert.Equal(1, sub.DirCount);

        var deep = _repo.Get(Path.Combine(_rootDir, @"sub\deep"));
        Assert.Equal(300, deep!.SizeBytes);

        var root = _repo.Get(_rootDir);
        Assert.Equal(650, root!.SizeBytes);
    }

    [Fact]
    public async Task Compute_EmptyDirectory()
    {
        Directory.CreateDirectory(_rootDir);

        var result = await _service.ComputeAsync(_rootDir, CancellationToken.None);

        Assert.Equal(0, result!.SizeBytes);
        Assert.Equal(0, result.FileCount);
        Assert.Equal(0, result.DirCount);
    }

    [Fact]
    public async Task Compute_SkipsJunctions()
    {
        CreateFile(@"real\a.bin", 100);
        var junction = Path.Combine(_rootDir, "junction");
        // Create a directory junction to "real" — if followed, size would double (or loop).
        var psi = new System.Diagnostics.ProcessStartInfo("cmd.exe",
            $"/c mklink /J \"{junction}\" \"{Path.Combine(_rootDir, "real")}\"")
        {
            CreateNoWindow = true,
            UseShellExecute = false,
        };
        using (var p = System.Diagnostics.Process.Start(psi)!)
            p.WaitForExit();
        Assert.True(Directory.Exists(junction), "junction was not created");

        var result = await _service.ComputeAsync(_rootDir, CancellationToken.None);

        Assert.Equal(100, result!.SizeBytes);
        Assert.Equal(1, result.FileCount);
        Assert.Equal(1, result.DirCount); // only "real"; the junction is skipped
    }

    [Fact]
    public async Task Compute_Cancelled_WritesNothing()
    {
        CreateFile(@"a.bin", 100);
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        var result = await _service.ComputeAsync(_rootDir, cts.Token);

        Assert.Null(result);
        Assert.Null(_repo.Get(_rootDir));
    }

    [Fact]
    public void UpsertMany_OverwritesExisting()
    {
        var key = PathKey.Canonicalize(@"C:\some\dir");
        _repo.UpsertMany(new[] { new DirSizeResult(key, 100, 1, 0, false, DateTime.UtcNow) });
        _repo.UpsertMany(new[] { new DirSizeResult(key, 250, 2, 1, true, DateTime.UtcNow) });

        var row = _repo.Get(@"C:\some\dir");
        Assert.Equal(250, row!.SizeBytes);
        Assert.Equal(2, row.FileCount);
        Assert.True(row.Incomplete);
    }
}

public class ByteSizeFormatterTests
{
    [Theory]
    [InlineData(0, "0 B")]
    [InlineData(999, "999 B")]
    [InlineData(1024, "1 KB")]
    [InlineData(1536, "1.5 KB")]
    [InlineData(1048576, "1 MB")]
    [InlineData(1469006, "1.4 MB")]
    [InlineData(1073741824, "1 GB")]
    public void Format_HumanReadable(long bytes, string expected) =>
        Assert.Equal(expected, ByteSizeFormatter.Format(bytes));
}
