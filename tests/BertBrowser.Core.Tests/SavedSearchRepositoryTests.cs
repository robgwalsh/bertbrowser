using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class SavedSearchRepositoryTests : IDisposable
{
    private readonly string _dbPath;
    private readonly SavedSearchRepository _repo;

    public SavedSearchRepositoryTests()
    {
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{Guid.NewGuid():N}.db");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new SavedSearchRepository(db);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
    }

    [Fact]
    public void SaveThenGetAllRoundTripsEveryField()
    {
        _repo.Save(new SavedSearch("Big videos", "ext:mp4 size:>1gb", SavedSearchScope.Folder, @"C:\Videos"));

        var s = Assert.Single(_repo.GetAll());
        Assert.Equal("Big videos", s.Name);
        Assert.Equal("ext:mp4 size:>1gb", s.Query);
        Assert.Equal(SavedSearchScope.Folder, s.Scope);
        Assert.Equal(@"C:\Videos", s.ScopePath);
    }

    [Fact]
    public void NullScopePathRoundTrips()
    {
        _repo.Save(new SavedSearch("Recent", "dm:today", SavedSearchScope.ThisPc, null));

        var s = Assert.Single(_repo.GetAll());
        Assert.Equal(SavedSearchScope.ThisPc, s.Scope);
        Assert.Null(s.ScopePath);
    }

    [Fact]
    public void ScopePathIsStoredWithDisplayCasingAndNoTrailingSeparator()
    {
        _repo.Save(new SavedSearch("Pinned", "report", SavedSearchScope.Folder, @"C:\Work\Reports\"));

        Assert.Equal(@"C:\Work\Reports", Assert.Single(_repo.GetAll()).ScopePath);
    }

    [Fact]
    public void ExistsIsCaseInsensitive()
    {
        _repo.Save(new SavedSearch("Big Videos", "ext:mp4", SavedSearchScope.CurrentFolder, null));

        Assert.True(_repo.Exists("big videos"));
        Assert.False(_repo.Exists("small videos"));
    }

    [Fact]
    public void SaveUnderAnExistingNameInAnyCaseReplacesRatherThanDuplicating()
    {
        _repo.Save(new SavedSearch("Docs", "ext:doc", SavedSearchScope.CurrentFolder, null));
        _repo.Save(new SavedSearch("DOCS", "ext:docx", SavedSearchScope.ThisPc, null));

        var s = Assert.Single(_repo.GetAll());
        Assert.Equal("ext:docx", s.Query);
        Assert.Equal(SavedSearchScope.ThisPc, s.Scope);
    }

    [Fact]
    public void RenameMovesTheRow()
    {
        _repo.Save(new SavedSearch("Old", "ext:txt", SavedSearchScope.CurrentFolder, null));

        Assert.True(_repo.Rename("Old", "New"));

        var s = Assert.Single(_repo.GetAll());
        Assert.Equal("New", s.Name);
        Assert.Equal("ext:txt", s.Query);
    }

    [Fact]
    public void RenameOntoAnotherRowIsRefusedAndChangesNothing()
    {
        _repo.Save(new SavedSearch("A", "ext:a", SavedSearchScope.CurrentFolder, null));
        _repo.Save(new SavedSearch("B", "ext:b", SavedSearchScope.CurrentFolder, null));

        Assert.False(_repo.Rename("A", "b"));

        var all = _repo.GetAll();
        Assert.Equal(2, all.Count);
        Assert.Equal("ext:a", all.Single(s => s.Name == "A").Query);
        Assert.Equal("ext:b", all.Single(s => s.Name == "B").Query);
    }

    [Fact]
    public void RenameThatOnlyChangesCaseSucceeds()
    {
        _repo.Save(new SavedSearch("big videos", "ext:mp4", SavedSearchScope.CurrentFolder, null));

        Assert.True(_repo.Rename("big videos", "Big Videos"));

        Assert.Equal("Big Videos", Assert.Single(_repo.GetAll()).Name);
    }

    [Fact]
    public void RenameOfAMissingRowReturnsFalse()
    {
        Assert.False(_repo.Rename("Nope", "Still nope"));
    }

    [Fact]
    public void GetAllIsOrderedByNameIgnoringCase()
    {
        _repo.Save(new SavedSearch("zebra", "ext:z", SavedSearchScope.CurrentFolder, null));
        _repo.Save(new SavedSearch("Apple", "ext:a", SavedSearchScope.CurrentFolder, null));
        _repo.Save(new SavedSearch("mango", "ext:m", SavedSearchScope.CurrentFolder, null));

        Assert.Equal(["Apple", "mango", "zebra"], _repo.GetAll().Select(s => s.Name));
    }

    [Fact]
    public void RemoveDeletesTheRow()
    {
        _repo.Save(new SavedSearch("Gone", "ext:g", SavedSearchScope.CurrentFolder, null));

        _repo.Remove("gone");

        Assert.False(_repo.Exists("Gone"));
        Assert.Empty(_repo.GetAll());
    }
}
