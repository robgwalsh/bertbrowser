using BertBrowser.Core.Data;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class TagMoveTests : IDisposable
{
    private readonly string _dbPath;
    private readonly TagRepository _repo;

    public TagMoveTests()
    {
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{Guid.NewGuid():N}.db");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new TagRepository(db);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
    }

    [Fact]
    public void MovedFile_KeepsItsTags()
    {
        var work = _repo.CreateTag("work", null);
        _repo.AssignTags(new[] { @"C:\data\a.txt" }, new[] { work.Id });

        _repo.UpdateEntryPaths(@"C:\data\a.txt", @"C:\archive\a.txt");

        Assert.Empty(_repo.GetTagsForPaths(new[] { @"C:\data\a.txt" }));
        var map = _repo.GetTagsForPaths(new[] { @"C:\archive\a.txt" });
        Assert.Equal("work", map.Single().Value.Single().Name);
    }

    [Fact]
    public void MovedDirectory_RewritesDescendantPaths()
    {
        var work = _repo.CreateTag("work", null);
        _repo.AssignTags(new[] { @"C:\data\Proj\a.txt", @"C:\data\Proj\sub\b.txt" }, new[] { work.Id });

        _repo.UpdateEntryPaths(@"C:\data\Proj", @"C:\archive\Proj");

        var map = _repo.GetTagsForPaths(new[] { @"C:\archive\Proj\a.txt", @"C:\archive\Proj\sub\b.txt" });
        Assert.Equal(2, map.Count);

        // Display casing is preserved through the prefix rewrite.
        var moved = _repo.QueryTaggedFilesUnder(@"C:\archive", new[] { work.Id }, TagMatchMode.Any);
        Assert.Equal(
            new[] { @"C:\archive\Proj\a.txt", @"C:\archive\Proj\sub\b.txt" },
            moved.Select(f => f.DisplayPath).OrderBy(p => p));
    }

    [Fact]
    public void MoveToExistingTaggedPath_LeavesTargetRowIntact()
    {
        var work = _repo.CreateTag("work", null);
        var home = _repo.CreateTag("home", null);
        _repo.AssignTags(new[] { @"C:\data\a.txt" }, new[] { work.Id });
        _repo.AssignTags(new[] { @"C:\archive\a.txt" }, new[] { home.Id });

        _repo.UpdateEntryPaths(@"C:\data\a.txt", @"C:\archive\a.txt");

        var map = _repo.GetTagsForPaths(new[] { @"C:\archive\a.txt" });
        Assert.Equal("home", map.Single().Value.Single().Name);
    }
}
