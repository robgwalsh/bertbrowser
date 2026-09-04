using BertBrowser.Core.Data;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Models;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class SavedWorkspaceRepositoryTests : IDisposable
{
    private readonly string _dbPath;
    private readonly SavedWorkspaceRepository _repo;

    public SavedWorkspaceRepositoryTests()
    {
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{Guid.NewGuid():N}.db");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new SavedWorkspaceRepository(db);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
    }

    private static SessionLayout OnePane(params string[] paths) => new()
    {
        Tabs = paths.Select(p => new SessionTab { Path = p }).ToList(),
    };

    [Fact]
    public void SaveThenGetAllRoundTripsTheLayout()
    {
        _repo.Save(new SavedWorkspace("Work", OnePane(@"C:\Work", @"C:\Work\Reports")));

        var w = Assert.Single(_repo.GetAll());
        Assert.Equal("Work", w.Name);
        Assert.Equal(2, w.Layout.Tabs!.Count);
        Assert.Equal(@"C:\Work", w.Layout.Tabs![0].Path);
        Assert.Equal(@"C:\Work\Reports", w.Layout.Tabs![1].Path);
    }

    [Fact]
    public void ExistsIsCaseInsensitive()
    {
        _repo.Save(new SavedWorkspace("Dev Setup", OnePane(@"C:\Dev")));

        Assert.True(_repo.Exists("dev setup"));
        Assert.False(_repo.Exists("something else"));
    }

    [Fact]
    public void SaveUnderAnExistingNameInAnyCaseReplacesRatherThanDuplicating()
    {
        _repo.Save(new SavedWorkspace("Docs", OnePane(@"C:\Docs")));
        _repo.Save(new SavedWorkspace("DOCS", OnePane(@"C:\Docs2")));

        var w = Assert.Single(_repo.GetAll());
        Assert.Equal(@"C:\Docs2", w.Layout.Tabs![0].Path);
    }

    [Fact]
    public void RenameMovesTheRow()
    {
        _repo.Save(new SavedWorkspace("Old", OnePane(@"C:\A")));

        Assert.True(_repo.Rename("Old", "New"));

        var w = Assert.Single(_repo.GetAll());
        Assert.Equal("New", w.Name);
        Assert.Equal(@"C:\A", w.Layout.Tabs![0].Path);
    }

    [Fact]
    public void RenameOntoAnotherRowIsRefusedAndChangesNothing()
    {
        _repo.Save(new SavedWorkspace("A", OnePane(@"C:\A")));
        _repo.Save(new SavedWorkspace("B", OnePane(@"C:\B")));

        Assert.False(_repo.Rename("A", "b"));

        var all = _repo.GetAll();
        Assert.Equal(2, all.Count);
    }

    [Fact]
    public void RenameThatOnlyChangesCaseSucceeds()
    {
        _repo.Save(new SavedWorkspace("dev setup", OnePane(@"C:\Dev")));

        Assert.True(_repo.Rename("dev setup", "Dev Setup"));

        Assert.Equal("Dev Setup", Assert.Single(_repo.GetAll()).Name);
    }

    [Fact]
    public void RenameOfAMissingRowReturnsFalse()
    {
        Assert.False(_repo.Rename("Nope", "Still nope"));
    }

    [Fact]
    public void GetAllIsOrderedByNameIgnoringCase()
    {
        _repo.Save(new SavedWorkspace("zebra", OnePane(@"C:\Z")));
        _repo.Save(new SavedWorkspace("Apple", OnePane(@"C:\A")));
        _repo.Save(new SavedWorkspace("mango", OnePane(@"C:\M")));

        Assert.Equal(["Apple", "mango", "zebra"], _repo.GetAll().Select(w => w.Name));
    }

    [Fact]
    public void RemoveDeletesTheRow()
    {
        _repo.Save(new SavedWorkspace("Gone", OnePane(@"C:\Gone")));

        _repo.Remove("gone");

        Assert.False(_repo.Exists("Gone"));
        Assert.Empty(_repo.GetAll());
    }

    [Fact]
    public void ARowWithUnparsableLayoutJsonIsSkippedRatherThanFailingTheWholeList()
    {
        _repo.Save(new SavedWorkspace("Good", OnePane(@"C:\Good")));

        // Corrupt the second row directly, as a hand-edited DB or a future format change would.
        var db = new Db(_dbPath);
        using (var conn = db.Open())
        using (var cmd = conn.CreateCommand())
        {
            cmd.CommandText =
                "INSERT INTO saved_workspace(name, layout_json, added_utc) VALUES ('Bad', 'not json', @now);";
            cmd.Parameters.AddWithValue("@now", DateTime.UtcNow.ToString("O"));
            cmd.ExecuteNonQuery();
        }

        var w = Assert.Single(_repo.GetAll());
        Assert.Equal("Good", w.Name);
    }
}
