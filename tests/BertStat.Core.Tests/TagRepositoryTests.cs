using BertStat.Core.Data;
using Xunit;

namespace BertStat.Core.Tests;

public sealed class TagRepositoryTests : IDisposable
{
    private readonly string _dbPath;
    private readonly TagRepository _repo;

    public TagRepositoryTests()
    {
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertstat-test-{Guid.NewGuid():N}.db");
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
    public void CreateAndListTags()
    {
        _repo.CreateTag("work", "#FF0000");
        _repo.CreateTag("archive", null);

        var tags = _repo.GetAllTags();
        Assert.Equal(new[] { "archive", "work" }, tags.Select(t => t.Name));
        Assert.Equal("#FF0000", tags.Single(t => t.Name == "work").Color);
    }

    [Fact]
    public void AssignAndReadBack()
    {
        var work = _repo.CreateTag("work", null);
        _repo.AssignTags(new[] { @"C:\data\a.txt", @"C:\data\b.txt" }, new[] { work.Id });

        var map = _repo.GetTagsForPaths(new[] { @"C:\DATA\A.TXT", @"C:\data\b.txt", @"C:\data\c.txt" });
        Assert.Equal(2, map.Count);
        Assert.Contains(@"C:\DATA\A.TXT", map.Keys);
        Assert.Equal("work", map[@"C:\DATA\A.TXT"].Single().Name);
    }

    [Fact]
    public void UnassignLastTag_RemovesOrphanFileRow()
    {
        var work = _repo.CreateTag("work", null);
        _repo.AssignTags(new[] { @"C:\data\a.txt" }, new[] { work.Id });
        _repo.UnassignTags(new[] { @"C:\data\a.txt" }, new[] { work.Id });

        var map = _repo.GetTagsForPaths(new[] { @"C:\data\a.txt" });
        Assert.Empty(map);
        Assert.Equal(0, _repo.GetTagUsageCount(work.Id));
    }

    [Fact]
    public void DeleteTag_CascadesAssignments()
    {
        var work = _repo.CreateTag("work", null);
        var keep = _repo.CreateTag("keep", null);
        _repo.AssignTags(new[] { @"C:\data\a.txt" }, new[] { work.Id, keep.Id });

        _repo.DeleteTag(work.Id);

        var map = _repo.GetTagsForPaths(new[] { @"C:\data\a.txt" });
        Assert.Equal("keep", map[@"C:\DATA\A.TXT"].Single().Name);
    }

    [Fact]
    public void QueryTaggedFilesUnder_AnyMode_RespectsSubtree()
    {
        var work = _repo.CreateTag("work", null);
        var fun = _repo.CreateTag("fun", null);
        _repo.AssignTags(new[] { @"C:\root\sub\a.txt" }, new[] { work.Id });
        _repo.AssignTags(new[] { @"C:\root\deep\er\b.txt" }, new[] { fun.Id });
        _repo.AssignTags(new[] { @"C:\rootbeer\c.txt" }, new[] { work.Id });   // sibling prefix trap
        _repo.AssignTags(new[] { @"C:\other\d.txt" }, new[] { work.Id });

        var result = _repo.QueryTaggedFilesUnder(@"C:\root", new[] { work.Id, fun.Id }, TagMatchMode.Any);

        Assert.Equal(2, result.Count);
        Assert.Contains(result, f => f.DisplayPath.EndsWith(@"sub\a.txt"));
        Assert.Contains(result, f => f.DisplayPath.EndsWith(@"er\b.txt"));
    }

    [Fact]
    public void QueryTaggedFilesUnder_AllMode_RequiresEveryTag()
    {
        var work = _repo.CreateTag("work", null);
        var fun = _repo.CreateTag("fun", null);
        _repo.AssignTags(new[] { @"C:\root\both.txt" }, new[] { work.Id, fun.Id });
        _repo.AssignTags(new[] { @"C:\root\workonly.txt" }, new[] { work.Id });

        var result = _repo.QueryTaggedFilesUnder(@"C:\root", new[] { work.Id, fun.Id }, TagMatchMode.All);

        Assert.Single(result);
        Assert.EndsWith("both.txt", result[0].DisplayPath);
    }

    [Fact]
    public void QueryTaggedFilesUnder_ShowsAllTagsOfMatch_NotJustSelected()
    {
        var work = _repo.CreateTag("work", null);
        var fun = _repo.CreateTag("fun", null);
        _repo.AssignTags(new[] { @"C:\root\a.txt" }, new[] { work.Id, fun.Id });

        var result = _repo.QueryTaggedFilesUnder(@"C:\root", new[] { work.Id }, TagMatchMode.Any);

        Assert.Equal(new[] { "fun", "work" }, result.Single().Tags);
    }

    [Fact]
    public void GetTagCountsUnder_CountsOnlySubtree()
    {
        var work = _repo.CreateTag("work", null);
        _repo.AssignTags(new[] { @"C:\root\a.txt", @"C:\root\s\b.txt", @"C:\other\c.txt" }, new[] { work.Id });

        var counts = _repo.GetTagCountsUnder(@"C:\root");
        Assert.Equal(2, counts[work.Id]);
    }

    [Fact]
    public void TagNames_CaseInsensitivelyUnique()
    {
        _repo.CreateTag("Work", null);
        Assert.Throws<Microsoft.Data.Sqlite.SqliteException>(() => _repo.CreateTag("work", null));
    }
}
