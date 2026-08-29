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
    private static FsEntryRow Row(string displayPath, bool isDir = false, long size = 0, bool hidden = false) =>
        new(PathKey.Canonicalize(displayPath), Path.GetFileName(displayPath), isDir, size, DateTime.UtcNow, hidden);

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
    public void Search_ExcludesHiddenUnlessRequested()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\report-visible.txt"),
            Row(@"C:\Data\report-secret.txt", hidden: true),
        }, crawlGen: 1);

        // Default (includeHidden: true) surfaces both, flagging the hidden one.
        var (all, _) = _repo.Search(@"C:\Data", Q("report"), cap: 100);
        Assert.Equal(2, all.Count);
        Assert.True(Assert.Single(all, h => h.Name == "report-secret.txt").Hidden);

        // includeHidden: false drops the hidden row entirely.
        var (visible, _) = _repo.Search(@"C:\Data", Q("report"), cap: 100, includeHidden: false);
        var hit = Assert.Single(visible);
        Assert.Equal("report-visible.txt", hit.Name);
        Assert.False(hit.Hidden);
    }

    [Fact]
    public void SearchGlobal_SpansDrivesWithFullPaths()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data", isDir: true),
            Row(@"C:\Data\report.txt", size: 5),
            Row(@"C:\readme-report.md"),        // direct child of the drive root
            Row(@"D:\Backup", isDir: true),
            Row(@"D:\Backup\report-old.txt"),
            Row(@"C:\Data\unrelated.txt"),
        }, crawlGen: 1);

        var (hits, truncated) = _repo.SearchGlobal(Q("report"), cap: 100);

        Assert.False(truncated);
        var paths = hits.Select(h => h.DisplayPath).OrderBy(p => p).ToList();
        Assert.Equal(new[]
        {
            @"C:\Data\report.txt",
            @"C:\readme-report.md",
            @"D:\Backup\report-old.txt",
        }, paths);

        // The Folder column carries the full parent path (Everything-style), not a relative one.
        var deep = Assert.Single(hits, h => h.Name == "report.txt");
        Assert.Equal(@"C:\Data", deep.RelativeDirDisplay);
        var atRoot = Assert.Single(hits, h => h.Name == "readme-report.md");
        Assert.Equal(@"C:\", atRoot.RelativeDirDisplay);
    }

    [Fact]
    public void SearchGlobal_HonorsHiddenAndTruncation()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\a-report.txt"),
            Row(@"C:\b-report.txt"),
            Row(@"C:\c-report.txt", hidden: true),
        }, crawlGen: 1);

        var (visible, _) = _repo.SearchGlobal(Q("report"), cap: 100, includeHidden: false);
        Assert.Equal(2, visible.Count);
        Assert.DoesNotContain(visible, h => h.Name == "c-report.txt");

        var (capped, truncated) = _repo.SearchGlobal(Q("report"), cap: 1);
        Assert.True(truncated);
        Assert.Single(capped);
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

    // --- LargestFiles (disk-space explorer) ---

    [Fact]
    public void LargestFiles_OrdersBySizeDescendingAndRespectsLimit()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\small.bin", size: 10),
            Row(@"C:\Data\huge.bin", size: 3_000),
            Row(@"C:\Data\medium.bin", size: 200),
        }, crawlGen: 1);

        var files = _repo.LargestFiles(@"C:\Data", limit: 2);

        Assert.Equal(["huge.bin", "medium.bin"], files.Select(f => f.Name));
        Assert.Equal(3_000, files[0].SizeBytes);
    }

    /// <summary>
    /// The half-open-bounds guarantee, stated as a test: a scope of "C:\Data" reaches neither
    /// neighbour whose name merely starts with it.
    /// </summary>
    /// <remarks>
    /// Both siblings are load-bearing, and one of them is easy to get wrong. Bounds are
    /// <c>["C:\DATA\", "C:\DATA]")</c>, so <c>C:\DATAX</c> is turned away by the <em>lower</em>
    /// bound alone ('X' sorts below '\') — a test using only that one still passes with the upper
    /// bound deleted. <c>C:\DATA_OLD</c> is the case only the upper bound catches, since '_' sorts
    /// above ']'. Drop either half of the range and this goes red.
    /// </remarks>
    [Fact]
    public void LargestFiles_ScopedToTheSubtree()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\big.bin", size: 10_000_000_000),
            Row(@"C:\Datax\bigger.bin", size: 20_000_000_000),
            Row(@"C:\Data_old\biggest.bin", size: 30_000_000_000),
        }, crawlGen: 1);

        var files = _repo.LargestFiles(@"C:\Data", limit: 10);

        Assert.Equal("big.bin", Assert.Single(files).Name);
    }

    /// <summary>
    /// Folder totals live in dir_size_cache; a row here is only ever a file's own bytes. The query
    /// must exclude directories itself rather than relying on the indexer writing them as zero.
    /// </summary>
    [Fact]
    public void LargestFiles_ExcludesDirectories()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\Sub", isDir: true, size: 9_000_000),
            Row(@"C:\Data\file.bin", size: 5),
        }, crawlGen: 1);

        var files = _repo.LargestFiles(@"C:\Data", limit: 10);

        Assert.Equal("file.bin", Assert.Single(files).Name);
    }

    [Fact]
    public void LargestFiles_ExcludesHiddenUnlessRequested()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\visible.bin", size: 10),
            Row(@"C:\Data\secret.bin", size: 5_000, hidden: true),
        }, crawlGen: 1);

        Assert.Equal(2, _repo.LargestFiles(@"C:\Data", limit: 10).Count);

        var visibleOnly = _repo.LargestFiles(@"C:\Data", limit: 10, includeHidden: false);
        Assert.Equal("visible.bin", Assert.Single(visibleOnly).Name);
    }

    /// <summary>Display paths are reassembled from ancestor rows, so original casing survives a
    /// table that only stores uppercase keys.</summary>
    [Fact]
    public void LargestFiles_ReconstructsDisplayPaths()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\Sub", isDir: true),
            Row(@"C:\Data\Sub\deep.bin", size: 77),
        }, crawlGen: 1);

        var file = Assert.Single(_repo.LargestFiles(@"C:\Data", limit: 10));

        Assert.Equal("Sub", file.RelativeDirDisplay);
        Assert.Equal(@"C:\Data\Sub\deep.bin", file.DisplayPath);
    }

    [Fact]
    public void LargestFiles_WholePc_SpansVolumesAndCarriesFullPaths()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data", isDir: true),
            Row(@"C:\Data\c-file.bin", size: 100),
            Row(@"D:\Media", isDir: true),
            Row(@"D:\Media\d-file.bin", size: 900),
        }, crawlGen: 1);

        var files = _repo.LargestFiles(rootPath: null, limit: 10);

        Assert.Equal(["d-file.bin", "c-file.bin"], files.Select(f => f.Name));
        Assert.Equal(@"D:\Media\d-file.bin", files[0].DisplayPath);
        Assert.Equal(@"D:\Media", files[0].RelativeDirDisplay);
    }

    [Fact]
    public void LargestFiles_NonPositiveLimitReturnsNothingRatherThanEverything()
    {
        _repo.UpsertEntries(new[] { Row(@"C:\Data\file.bin", size: 5) }, crawlGen: 1);

        Assert.Empty(_repo.LargestFiles(@"C:\Data", limit: 0));
    }

    // --- DuplicateCandidates: the shortlist a duplicate scan starts from ---

    /// <summary>Two files of one length are the shortlist; the odd one out is not on it.</summary>
    [Fact]
    public void DuplicateCandidates_ReturnsOnlyCollidingLengths()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\a.bin", size: 4096),
            Row(@"C:\Data\b.bin", size: 4096),
            Row(@"C:\Data\lonely.bin", size: 999),
        }, crawlGen: 1);

        var shortlist = _repo.DuplicateCandidates(@"C:\Data", minSizeBytes: 1, includeHidden: true);

        Assert.Equal(2, shortlist.Files.Count);
        Assert.All(shortlist.Files, f => Assert.Equal(4096, f.SizeBytes));
    }

    /// <summary>
    /// Directories are stored with size 0 and would otherwise all collide with each other. The
    /// query excludes them outright rather than relying on the floor to hide them.
    /// </summary>
    [Fact]
    public void DuplicateCandidates_NeverReturnsDirectories()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\One", isDir: true),
            Row(@"C:\Data\Two", isDir: true),
            Row(@"C:\Data\a.bin", size: 4096),
            Row(@"C:\Data\b.bin", size: 4096),
        }, crawlGen: 1);

        var shortlist = _repo.DuplicateCandidates(@"C:\Data", minSizeBytes: 1, includeHidden: true);

        Assert.Equal(2, shortlist.Files.Count);
        Assert.All(shortlist.Files, f => Assert.False(f.IsDirectory));
    }

    [Fact]
    public void DuplicateCandidates_HonoursTheSizeFloor()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\small-a.bin", size: 10),
            Row(@"C:\Data\small-b.bin", size: 10),
            Row(@"C:\Data\big-a.bin", size: 5000),
            Row(@"C:\Data\big-b.bin", size: 5000),
        }, crawlGen: 1);

        var shortlist = _repo.DuplicateCandidates(@"C:\Data", minSizeBytes: 1000, includeHidden: true);

        Assert.Equal(2, shortlist.Files.Count);
        Assert.All(shortlist.Files, f => Assert.Equal(5000, f.SizeBytes));
    }

    /// <summary>
    /// A floor of zero would sweep in every file the sizeless build path could not measure and
    /// compare them all against each other, which is the one shape this feature must never take.
    /// </summary>
    [Fact]
    public void DuplicateCandidates_ZeroLengthFilesAreNeverCandidates()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\empty-a.bin", size: 0),
            Row(@"C:\Data\empty-b.bin", size: 0),
        }, crawlGen: 1);

        Assert.Empty(_repo.DuplicateCandidates(@"C:\Data", minSizeBytes: 0, includeHidden: true).Files);
    }

    [Fact]
    public void DuplicateCandidates_ExcludesHiddenUnlessRequested()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\visible.bin", size: 4096),
            Row(@"C:\Data\secret.bin", size: 4096, hidden: true),
        }, crawlGen: 1);

        Assert.Equal(2, _repo.DuplicateCandidates(@"C:\Data", 1, includeHidden: true).Files.Count);

        // With the hidden row gone the other has nothing to collide with, so the pair vanishes
        // rather than leaving a group of one.
        Assert.Empty(_repo.DuplicateCandidates(@"C:\Data", 1, includeHidden: false).Files);
    }

    /// <summary>
    /// The exclusion runs in both passes. Applying it only to the second would let an excluded file
    /// prop up a size group that then comes back with a single member — a "duplicate" of nothing.
    /// </summary>
    [Fact]
    public void DuplicateCandidates_AnExcludedFileCannotPropUpAGroup()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\keep.bin", size: 4096),
            Row(@"C:\Data\skip.bin", size: 4096),
        }, crawlGen: 1);

        var shortlist = _repo.DuplicateCandidates(
            @"C:\Data", 1, includeHidden: true,
            exclude: key => key.EndsWith(@"\SKIP.BIN", StringComparison.Ordinal));

        Assert.Empty(shortlist.Files);
    }

    [Fact]
    public void DuplicateCandidates_ScopesToTheSubtree()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\a.bin", size: 4096),
            Row(@"C:\Data\b.bin", size: 4096),
            Row(@"C:\Other\a.bin", size: 4096),
        }, crawlGen: 1);

        var scoped = _repo.DuplicateCandidates(@"C:\Data", 1, includeHidden: true);
        Assert.Equal(2, scoped.Files.Count);

        var global = _repo.DuplicateCandidates(null, 1, includeHidden: true);
        Assert.Equal(3, global.Files.Count);
    }

    /// <summary>
    /// Full display paths are not stored, so each hit's is rebuilt from its ancestors' rows — the
    /// same reconstruction the search paths do, which is why they share one function.
    /// </summary>
    [Fact]
    public void DuplicateCandidates_RebuildsDisplayPathsFromAncestors()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\Nested", isDir: true),
            Row(@"C:\Data\Nested\Report.pdf", size: 4096),
            Row(@"C:\Data\Report.pdf", size: 4096),
        }, crawlGen: 1);

        var shortlist = _repo.DuplicateCandidates(@"C:\Data", 1, includeHidden: true);

        var nested = Assert.Single(shortlist.Files, f => f.RelativeDirDisplay.Length > 0);
        Assert.Equal("Nested", nested.RelativeDirDisplay);
        Assert.Equal(@"C:\Data\Nested\Report.pdf", nested.DisplayPath);
    }

    /// <summary>
    /// The two scope counts are the evidence behind the availability verdict, and they describe
    /// what the index holds rather than what this caller asked to see — so they are taken before
    /// both the floor and the exclusion.
    /// </summary>
    [Fact]
    public void DuplicateCandidates_CountsWhatTheIndexHolds_NotWhatWasAskedFor()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\tiny.bin", size: 1),
            Row(@"C:\Data\a.bin", size: 4096),
            Row(@"C:\Data\b.bin", size: 4096),
        }, crawlGen: 1);

        var shortlist = _repo.DuplicateCandidates(
            @"C:\Data", minSizeBytes: 4096, includeHidden: true, exclude: _ => true);

        Assert.Empty(shortlist.Files);
        Assert.Equal(3, shortlist.FilesInScope);
        Assert.Equal(3, shortlist.SizedFilesInScope);
    }

    /// <summary>
    /// The shape that makes the whole feature refuse to run: rows in scope, not one with a length.
    /// That is the FSCTL_ENUM_USN_DATA build, and it has to be distinguishable from an empty
    /// folder — which is what the second count is for.
    /// </summary>
    [Fact]
    public void DuplicateCandidates_ASizelessIndexIsRowsWithNoLengths()
    {
        _repo.UpsertEntries(new[]
        {
            Row(@"C:\Data\a.bin", size: 0),
            Row(@"C:\Data\b.bin", size: 0),
        }, crawlGen: 1);

        var sizeless = _repo.DuplicateCandidates(@"C:\Data", 1, includeHidden: true);
        Assert.Equal(2, sizeless.FilesInScope);
        Assert.Equal(0, sizeless.SizedFilesInScope);

        var empty = _repo.DuplicateCandidates(@"C:\Nowhere", 1, includeHidden: true);
        Assert.Equal(0, empty.FilesInScope);
        Assert.Equal(0, empty.SizedFilesInScope);
    }
}
