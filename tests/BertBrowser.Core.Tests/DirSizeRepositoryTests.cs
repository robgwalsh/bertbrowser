using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class DirSizeRepositoryTests : IDisposable
{
    private readonly string _dbPath;
    private readonly DirSizeRepository _repo;

    public DirSizeRepositoryTests()
    {
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{Guid.NewGuid():N}.db");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new DirSizeRepository(db);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
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

    [Fact]
    public void Get_UnknownDirectory_IsNull() => Assert.Null(_repo.Get(@"C:\never\indexed"));

    [Fact]
    public void GetMany_KeysAreCanonicalizedPaths()
    {
        var key = PathKey.Canonicalize(@"C:\some\dir");
        _repo.UpsertMany(new[] { new DirSizeResult(key, 100, 1, 0, false, DateTime.UtcNow) });

        // Queried with different casing and a trailing separator; both canonicalize to the same key.
        var rows = _repo.GetMany(new[] { @"c:\SOME\dir\" });

        Assert.Equal(100, rows[key].SizeBytes);
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
    [InlineData(1073741824, "1.000 GB")]
    public void Format_HumanReadable(long bytes, string expected) =>
        Assert.Equal(expected, ByteSizeFormatter.Format(bytes));
}
