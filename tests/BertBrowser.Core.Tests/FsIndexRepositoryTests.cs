using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class FsIndexRepositoryTests : IDisposable
{
    private readonly string _dbPath;
    private readonly FsIndexRepository _repo;

    public FsIndexRepositoryTests()
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

    /// <summary>Index rows are synthetic — no real filesystem needed for repo tests.</summary>
    private static FsEntryRow Row(string displayPath, bool isDir = false, long size = 0) =>
        new(PathKey.Canonicalize(displayPath), Path.GetFileName(displayPath), isDir, size, DateTime.UtcNow);

    private static SearchQuery Q(string text) => SearchQuery.Parse(text)!;

    [Fact]
    public void Search_MatchesSubstringCaseInsensitively()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\Quarterly-Report.docx", size: 42),
            Row(@"C:\Data\notes.txt"),
        }, crawlGen: 1);

        var (hits, truncated) = _repo.Search(@"C:\data", Q("report"), cap: 100);

        var hit = Assert.Single(hits);
        Assert.Equal("Quarterly-Report.docx", hit.Name);
        Assert.Equal(42, hit.SizeBytes);
        Assert.False(hit.IsDirectory);
        Assert.False(truncated);
    }

    [Fact]
    public void Search_ScopesToSubtree_SiblingPrefixExcluded()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Foo\match.txt"),
            Row(@"C:\Foobar\match.txt"), // sibling whose name shares the prefix
        }, crawlGen: 1);

        var (hits, _) = _repo.Search(@"C:\Foo", Q("match"), cap: 100);

        var hit = Assert.Single(hits);
        Assert.Equal(@"C:\Foo\match.txt", hit.DisplayPath);
    }

    [Fact]
    public void Search_AndSemantics_AllTermsMustMatch()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\Project-Report.docx"),
            Row(@"C:\Data\Project-Plan.docx"),
            Row(@"C:\Data\Old-Report.docx"),
        }, crawlGen: 1);

        var (hits, _) = _repo.Search(@"C:\Data", Q("proj rep"), cap: 100);

        Assert.Equal("Project-Report.docx", Assert.Single(hits).Name);
    }

    [Theory]
    [InlineData("*.txt", new[] { "b.txt", "deep.txt" })]
    [InlineData("f?le", new[] { "file.bin" })]
    [InlineData("a[1]", new[] { "a[1].tmp" })]
    public void Search_WildcardsAndBrackets(string queryText, string[] expectedNames)
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\b.txt"),
            Row(@"C:\Data\sub\deep.txt"),
            Row(@"C:\Data\sub", isDir: true),
            Row(@"C:\Data\file.bin"),
            Row(@"C:\Data\a[1].tmp"),
        }, crawlGen: 1);

        var (hits, _) = _repo.Search(@"C:\Data", Q(queryText), cap: 100);

        Assert.Equal(expectedNames.OrderBy(n => n), hits.Select(h => h.Name).OrderBy(n => n));
    }

    [Fact]
    public void Search_CapsAndReportsTruncation()
    {
        _repo.UpsertEntries(Enumerable.Range(0, 5)
            .Select(i => Row($@"C:\Data\file{i}.txt")).ToList(), crawlGen: 1);

        var (hits, truncated) = _repo.Search(@"C:\Data", Q("file"), cap: 3);

        Assert.Equal(3, hits.Count);
        Assert.True(truncated);
    }

    [Fact]
    public void Search_ReconstructsRelativeDisplayPaths()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Root\Sub", isDir: true),
            Row(@"C:\Root\Sub\Deep", isDir: true),
            Row(@"C:\Root\Sub\Deep\File.txt"),
            Row(@"C:\Root\direct.txt"),
        }, crawlGen: 1);

        var (hits, _) = _repo.Search(@"C:\Root", Q("file"), cap: 100);
        var nested = hits.Single(h => h.Name == "File.txt");
        Assert.Equal(@"Sub\Deep", nested.RelativeDirDisplay);
        Assert.Equal(@"C:\Root\Sub\Deep\File.txt", nested.DisplayPath);

        var (direct, _) = _repo.Search(@"C:\Root", Q("direct"), cap: 100);
        Assert.Equal("", direct.Single().RelativeDirDisplay);
        Assert.Equal(@"C:\Root\direct.txt", direct.Single().DisplayPath);
    }

    [Fact]
    public void SweepVanished_RemovesOnlyUntouchedRowsInRange()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\stays.txt"),
            Row(@"C:\Data\vanished.txt"),
            Row(@"C:\Other\outside.txt"),
        }, crawlGen: 1);

        // A newer crawl re-touched only stays.txt.
        _repo.UpsertEntries(new[] { Row(@"C:\Data\stays.txt") }, crawlGen: 2);
        _repo.SweepVanished(PathKey.Canonicalize(@"C:\Data"), crawlGen: 2);

        Assert.Single(_repo.Search(@"C:\Data", Q("txt"), 100).Hits, h => h.Name == "stays.txt");
        Assert.Single(_repo.Search(@"C:\Other", Q("txt"), 100).Hits); // outside the swept range
    }

    [Fact]
    public void FindCoveringRoot_AncestorOrEqual_CompleteRequired()
    {
        Assert.Null(_repo.FindCoveringRoot(@"C:\Foo\Bar"));

        _repo.UpsertRoot(PathKey.Canonicalize(@"C:\Foo"), @"C:\Foo", DateTime.UtcNow, complete: false);
        Assert.Null(_repo.FindCoveringRoot(@"C:\Foo\Bar")); // incomplete crawl doesn't cover

        _repo.UpsertRoot(PathKey.Canonicalize(@"C:\Foo"), @"C:\Foo", DateTime.UtcNow, complete: true);
        var covering = _repo.FindCoveringRoot(@"C:\Foo\Bar\baz");
        Assert.Equal(PathKey.Canonicalize(@"C:\Foo"), covering!.PathKey);
        Assert.Equal(PathKey.Canonicalize(@"C:\Foo"), _repo.FindCoveringRoot(@"C:\Foo")!.PathKey); // equal counts

        Assert.Null(_repo.FindCoveringRoot(@"C:\Other"));
        Assert.Null(_repo.FindCoveringRoot(@"C:\Foobar")); // sibling prefix is not covered
    }

    [Fact]
    public void FindCoveringRoot_PrefersNonStale()
    {
        _repo.UpsertRoot(PathKey.Canonicalize(@"C:\Foo\Bar"), @"C:\Foo\Bar", DateTime.UtcNow, complete: true);
        _repo.MarkRootStale(PathKey.Canonicalize(@"C:\Foo\Bar"));
        _repo.UpsertRoot(PathKey.Canonicalize(@"C:\Foo"), @"C:\Foo", DateTime.UtcNow, complete: true);

        // The deeper root is stale; the fresh ancestor wins.
        var covering = _repo.FindCoveringRoot(@"C:\Foo\Bar\baz");
        Assert.Equal(PathKey.Canonicalize(@"C:\Foo"), covering!.PathKey);
        Assert.False(covering.Stale);
    }

    [Fact]
    public void DeleteSubtree_RemovesEntryAndDescendantsOnly()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\Sub", isDir: true),
            Row(@"C:\Data\Sub\a.txt"),
            Row(@"C:\Data\Sub\deep\b.txt"),
            Row(@"C:\Data\Subtle.txt"), // shares the name prefix; must survive
        }, crawlGen: 1);

        _repo.DeleteSubtree(PathKey.Canonicalize(@"C:\Data\Sub"));

        var (hits, _) = _repo.Search(@"C:\Data", Q("txt sub"), 100);
        Assert.Equal("Subtle.txt", Assert.Single(hits).Name);
    }

    [Fact]
    public void Rename_RewritesDescendantKeysInPlace()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\A\Old", isDir: true),
            Row(@"C:\A\Old\f1.txt"),
            Row(@"C:\A\Old\sub", isDir: true),
            Row(@"C:\A\Old\sub\f2.txt"),
            Row(@"C:\A\other.txt"),
        }, crawlGen: 1);

        _repo.Rename(
            PathKey.Canonicalize(@"C:\A\Old"),
            PathKey.Canonicalize(@"C:\A\Renamed"),
            "Renamed",
            crawlGen: 2);

        var (hits, _) = _repo.Search(@"C:\A", Q("f2"), 100);
        var hit = Assert.Single(hits);
        Assert.Equal(@"Renamed\sub", hit.RelativeDirDisplay);
        Assert.Equal(@"C:\A\Renamed\sub\f2.txt", hit.DisplayPath);

        Assert.Empty(_repo.Search(@"C:\A\Old", Q("f1"), 100).Hits);       // old subtree gone
        Assert.Single(_repo.Search(@"C:\A\Renamed", Q("f1"), 100).Hits);  // new subtree intact
        Assert.Single(_repo.Search(@"C:\A", Q("renamed"), 100).Hits);     // dir row renamed too
    }

    [Fact]
    public void Rename_CaseOnly_UpdatesDisplayNameOnly()
    {
        _repo.UpsertEntries(new[] { Row(@"C:\A\readme.txt") }, crawlGen: 1);

        var key = PathKey.Canonicalize(@"C:\A\readme.txt");
        _repo.Rename(key, key, "README.txt", crawlGen: 2);

        var (hits, _) = _repo.Search(@"C:\A", Q("readme"), 100);
        Assert.Equal("README.txt", Assert.Single(hits).Name);
    }

    [Fact]
    public void Rename_OverwritesExistingTarget()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\A\Src", isDir: true),
            Row(@"C:\A\Src\new.txt"),
            Row(@"C:\A\Dst", isDir: true),
            Row(@"C:\A\Dst\stale.txt"),
        }, crawlGen: 1);

        _repo.Rename(
            PathKey.Canonicalize(@"C:\A\Src"),
            PathKey.Canonicalize(@"C:\A\Dst"),
            "Dst",
            crawlGen: 2);

        Assert.Empty(_repo.Search(@"C:\A\Dst", Q("stale"), 100).Hits);
        Assert.Single(_repo.Search(@"C:\A\Dst", Q("new"), 100).Hits);
    }
}
