using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Compare;
using BertBrowser.Core.Services.Delete;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Reading a whole subtree out of the index to pair against another one. Everything here is about
/// the relative key: it is the string two sides are matched on, so a key that is off by a separator
/// pairs nothing with nothing and the comparison quietly reports the two folders share no files.
/// </summary>
public sealed class FsIndexRepositorySubtreeTests : IDisposable
{
    private readonly string _dbPath;
    private readonly FsIndexRepository _repo;

    public FsIndexRepositorySubtreeTests()
    {
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{Guid.NewGuid():N}.db");
        var db = new Db(_dbPath);
        db.Migrate();
        _repo = new FsIndexRepository(db);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
    }

    private static readonly DateTime Noon = new(2026, 3, 14, 12, 0, 0, DateTimeKind.Utc);

    private static FsEntryRow Row(string displayPath, bool isDir = false, long size = 0, bool hidden = false) =>
        new(PathKey.Canonicalize(displayPath), Path.GetFileName(displayPath), isDir, size, Noon, hidden);

    private IReadOnlyList<FsSubtreeRow> Read(string root, bool includeHidden = true, int cap = 1000) =>
        _repo.Subtree(root, includeHidden, cap).Rows;

    [Fact]
    public void KeysAreRelativeToTheRoot_AndTheRootItselfIsNotAmongThem()
    {
        _repo.UpsertEntries(
        [
            Row(@"C:\Data", isDir: true),
            Row(@"C:\Data\src", isDir: true),
            Row(@"C:\Data\src\Main.cs", size: 42),
        ], crawlGen: 1);

        var rows = Read(@"C:\Data");

        Assert.Equal(["SRC", @"SRC\MAIN.CS"], rows.Select(r => r.RelativeKey));
        Assert.Equal(42, rows[1].SizeBytes);
        Assert.Equal(Noon, rows[1].ModifiedUtc);
    }

    /// <summary>
    /// A drive root canonicalizes with its trailing separator kept, so slicing the wrong number of
    /// characters off leaves every key starting with one — and nothing ever pairs.
    /// </summary>
    [Fact]
    public void ADriveRootDoesNotLeaveALeadingSeparatorOnItsChildren()
    {
        _repo.UpsertEntries([Row(@"C:\notes.txt"), Row(@"C:\sub", isDir: true)], crawlGen: 1);

        Assert.Equal(["NOTES.TXT", "SUB"], Read(@"C:\").Select(r => r.RelativeKey).Order());
    }

    /// <summary>The key is uppercased so the two sides meet; the name keeps the casing the user
    /// will be shown, and it is the only display text a row carries.</summary>
    [Fact]
    public void TheKeyIsUppercasedAndTheNameKeepsItsCasing()
    {
        _repo.UpsertEntries(
        [
            Row(@"C:\Data\Source", isDir: true),
            Row(@"C:\Data\Source\ReadMe.md"),
        ], crawlGen: 1);

        var row = Read(@"C:\Data").Single(r => !r.IsDirectory);

        Assert.Equal(@"SOURCE\README.MD", row.RelativeKey);
        Assert.Equal("ReadMe.md", row.Name);
    }

    [Fact]
    public void HiddenRowsAreDroppedOnRequest()
    {
        _repo.UpsertEntries(
        [
            Row(@"C:\Data\seen.txt"),
            Row(@"C:\Data\secret.txt", hidden: true),
        ], crawlGen: 1);

        Assert.Equal(2, Read(@"C:\Data").Count);
        Assert.Equal("SEEN.TXT", Assert.Single(Read(@"C:\Data", includeHidden: false)).RelativeKey);
    }

    /// <summary>A file in a delete's holding folder is still on disk — that is what makes Ctrl+Z
    /// work — but it has been deleted as far as the user is concerned. Left in, it would show up as
    /// "only on this side" and be offered for deletion a second time.</summary>
    [Fact]
    public void TheExcludePredicateDropsAWholeHeldSubtree()
    {
        _repo.UpsertEntries(
        [
            Row(@"C:\Data\keep.txt"),
            Row(@"C:\Data\.bertbrowser-trash", isDir: true),
            Row(@"C:\Data\.bertbrowser-trash\delete-1\gone.txt"),
        ], crawlGen: 1);

        var rows = _repo.Subtree(@"C:\Data", includeHidden: true, cap: 1000,
            exclude: DeleteExecutor.IsHeldPath).Rows;

        Assert.Equal("KEEP.TXT", Assert.Single(rows).RelativeKey);
    }

    /// <summary>A ceiling that reported nothing would be worse than one that reports half, and a
    /// ceiling that reported half as if it were everything would be worse than both.</summary>
    [Fact]
    public void TheCapTruncatesRatherThanThrowing()
    {
        _repo.UpsertEntries(
            [.. Enumerable.Range(0, 10).Select(i => Row($@"C:\Data\f{i:00}.txt"))], crawlGen: 1);

        var (rows, truncated) = _repo.Subtree(@"C:\Data", includeHidden: true, cap: 4);

        Assert.True(truncated);
        Assert.Equal(4, rows.Count);
        Assert.False(_repo.Subtree(@"C:\Data", includeHidden: true, cap: 10).Truncated);
    }

    /// <summary>Ancestors are strict prefixes, so the table's own order already hands every parent
    /// over before its children. The display path is built on that and nothing else.</summary>
    [Fact]
    public void RowsArriveWithEveryParentBeforeItsChildren()
    {
        _repo.UpsertEntries(
        [
            Row(@"C:\Data\b", isDir: true),
            Row(@"C:\Data\b\deep", isDir: true),
            Row(@"C:\Data\b\deep\x.txt"),
            Row(@"C:\Data\a.txt"),
        ], crawlGen: 1);

        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var row in Read(@"C:\Data"))
        {
            var cut = row.RelativeKey.LastIndexOf(CompareKeys.Separator);
            if (cut > 0) Assert.Contains(row.RelativeKey[..cut], seen);
            seen.Add(row.RelativeKey);
        }

        Assert.Equal(4, seen.Count);
    }
}
