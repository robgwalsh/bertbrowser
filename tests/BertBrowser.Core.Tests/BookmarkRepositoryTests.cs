using BertBrowser.Core.Data;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class BookmarkRepositoryTests : IDisposable
{
    private readonly string _dbPath;
    private readonly BookmarkRepository _repo;

    public BookmarkRepositoryTests()
    {
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{Guid.NewGuid():N}.db");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new BookmarkRepository(db);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
    }

    [Fact]
    public void AddThenExistsAndGetAll()
    {
        Assert.True(_repo.Add(@"C:\data\project", isDirectory: true));

        Assert.True(_repo.Exists(@"C:\data\project"));
        var all = _repo.GetAll();
        var b = Assert.Single(all);
        Assert.Equal(@"C:\data\project", b.DisplayPath);
        Assert.True(b.IsDirectory);
    }

    [Fact]
    public void ExistsIsCaseInsensitive()
    {
        _repo.Add(@"C:\Data\Notes.txt", isDirectory: false);
        Assert.True(_repo.Exists(@"c:\data\notes.txt"));
    }

    [Fact]
    public void AddSamePathTwiceIsNoOp()
    {
        Assert.True(_repo.Add(@"C:\data\a", isDirectory: true));
        Assert.False(_repo.Add(@"C:\DATA\A", isDirectory: true)); // same canonical key
        Assert.Single(_repo.GetAll());
    }

    [Fact]
    public void RemoveDeletesBookmark()
    {
        _repo.Add(@"C:\data\a", isDirectory: true);
        _repo.Remove(@"c:\data\a");
        Assert.False(_repo.Exists(@"C:\data\a"));
        Assert.Empty(_repo.GetAll());
    }

    [Fact]
    public void GetAllOrdersDirectoriesBeforeFiles()
    {
        _repo.Add(@"C:\z\file.txt", isDirectory: false);
        _repo.Add(@"C:\a\folder", isDirectory: true);

        var all = _repo.GetAll();
        Assert.True(all[0].IsDirectory);
        Assert.False(all[1].IsDirectory);
    }
}
