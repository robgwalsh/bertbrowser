using BertBrowser.Core.Data;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Compare;
using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The test that makes the two ways of listing a folder impossible to drift apart.
/// </summary>
/// <remarks>
/// <para>
/// A comparison reads each side either from the index or by walking the disk, and the two sides
/// choose independently — so on any given run one folder may come from SQLite and the other from
/// the filesystem. Every case below builds one real tree, indexes it, and compares it against its
/// twin twice: once with both sides forced through the index, once with both forced through a
/// walk. The verdicts and the counts must be identical.
/// </para>
/// <para>
/// This is the regression guard for the failure that would otherwise be invisible: an indexed
/// drive and an unindexed one disagreeing about what "same" means, and therefore about what a
/// sync is allowed to delete. A wrong answer here does not throw, does not fail a build, and
/// shows up as a folder full of files that quietly went away.
/// </para>
/// </remarks>
public sealed class FolderCompareAgreementTests : IDisposable
{
    private readonly string _root;
    private readonly string _dbPath;
    private readonly Db _db;
    private readonly FsIndexRepository _repo;

    public FolderCompareAgreementTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-cmp-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);

        _dbPath = Path.Combine(_root, "index.db");
        _db = new Db(_dbPath);
        _db.Migrate();
        _repo = new FsIndexRepository(_db);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        try
        {
            Directory.Delete(_root, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    // --- helpers ---

    private string Dir(params string[] parts)
    {
        var path = Path.Combine([_root, .. parts]);
        Directory.CreateDirectory(path);
        return path;
    }

    private string File_(string content, params string[] parts)
    {
        var path = Path.Combine([_root, .. parts]);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content);
        return path;
    }

    /// <summary>Indexes both sides and compares them twice, asserting the two sources agree, then
    /// hands back the shared answer so a case can also say what it should be.</summary>
    private async Task<CompareResult> AgreeAsync(string left, string right, bool includeHidden = true)
    {
        var crawler = new IndexCrawler(_repo);
        Assert.True(await crawler.CrawlAsync(left, CancellationToken.None));
        Assert.True(await crawler.CrawlAsync(right, CancellationToken.None));

        var fromIndex = await Service(indexed: true)
            .CompareAsync(left, right, includeHidden, CancellationToken.None);
        var fromDisk = await Service(indexed: false)
            .CompareAsync(left, right, includeHidden, CancellationToken.None);

        Assert.Equal(CompareSourceKind.Index, fromIndex.LeftSource);
        Assert.Equal(CompareSourceKind.Index, fromIndex.RightSource);
        Assert.Equal(CompareSourceKind.Walk, fromDisk.LeftSource);
        Assert.Equal(CompareSourceKind.Walk, fromDisk.RightSource);

        AssertSameAnswer(fromIndex.Result, fromDisk.Result);
        return fromIndex.Result;
    }

    private FolderCompareService Service(bool indexed) =>
        new(_repo, indexed ? new AlwaysIndexed() : new NullMftIndexService());

    private static void AssertSameAnswer(CompareResult index, CompareResult walk)
    {
        Assert.Equal(
            index.ByRelativeKey.OrderBy(p => p.Key, StringComparer.Ordinal),
            walk.ByRelativeKey.OrderBy(p => p.Key, StringComparer.Ordinal));

        Assert.Equal(index.SameCount, walk.SameCount);
        Assert.Equal(index.DifferenceCount, walk.DifferenceCount);
        Assert.Equal(index.UnknownCount, walk.UnknownCount);
    }

    // --- the cases, chosen where the two sources will actually drift ---

    [Fact]
    public async Task TwoIdenticalTreesAgree()
    {
        foreach (var side in new[] { "left", "right" })
        {
            File_("hello", side, "readme.md");
            File_("code", side, "src", "main.cs");
            Dir(side, "src", "nested");
        }

        var result = await AgreeAsync(Dir("left"), Dir("right"));

        Assert.False(result.AnyDifference);
    }

    [Fact]
    public async Task AFileOnlyOnOneSideAgrees()
    {
        File_("a", "left", "shared.txt");
        File_("a", "right", "shared.txt");
        File_("x", "left", "extra.txt");

        var result = await AgreeAsync(Dir("left"), Dir("right"));

        Assert.Equal(CompareVerdict.LeftOnly, result.For("EXTRA.TXT"));
    }

    /// <summary>
    /// A hidden file with hidden items turned off. The walk declines to descend a hidden folder at
    /// all; the index filters on a stored flag that already means "this or an ancestor". If those
    /// two ever stop lining up, one source sees files the other cannot and every one of them reads
    /// as "only on this side".
    /// </summary>
    [Fact]
    public async Task AHiddenSubtreeIsSkippedTheSameWayByBothSources()
    {
        File_("a", "left", "seen.txt");
        File_("a", "right", "seen.txt");

        var hidden = Dir("left", "secret");
        File_("s", "left", "secret", "inner.txt");
        File.SetAttributes(hidden, File.GetAttributes(hidden) | FileAttributes.Hidden);

        var result = await AgreeAsync(Dir("left"), Dir("right"), includeHidden: false);

        Assert.False(result.AnyDifference);
        Assert.DoesNotContain("SECRET", result.ByRelativeKey.Keys);
        Assert.DoesNotContain(@"SECRET\INNER.TXT", result.ByRelativeKey.Keys);
    }

    /// <summary>
    /// The index stores a timestamp as a round-trip string and parses it back; the walk hands over
    /// raw ticks. Both must land on the same instant, or every file in an indexed folder reads as
    /// newer or older than its identical twin by a fraction of a second.
    /// </summary>
    [Fact]
    public async Task SubSecondTimestampsSurviveTheRoundTripThroughTheIndex()
    {
        var stamp = new DateTime(2026, 3, 14, 12, 0, 0, DateTimeKind.Utc).AddTicks(1_234_567);

        foreach (var side in new[] { "left", "right" })
            File.SetLastWriteTimeUtc(File_("same", side, "a.txt"), stamp);

        var result = await AgreeAsync(Dir("left"), Dir("right"));

        Assert.Equal(CompareVerdict.Same, result.For("A.TXT"));
    }

    /// <summary>An empty folder is a row in the index and an entry in the walk, and it is the one
    /// thing a listing keyed on files alone would drop.</summary>
    [Fact]
    public async Task AnEmptyFolderIsAnEntryToBothSources()
    {
        // Both sides carry a file with bytes in it so the index stays the chosen source: a side
        // holding nothing measurable is indistinguishable from an unmeasured volume, and falls to
        // the walk on purpose.
        File_("anchor", "left", "anchor.txt");
        File_("anchor", "right", "anchor.txt");
        Dir("left", "empty");

        var result = await AgreeAsync(Dir("left"), Dir("right"));

        Assert.Equal(CompareVerdict.LeftOnly, result.For("EMPTY"));
    }

    /// <summary>
    /// A file sitting in a delete's holding folder is still on disk — that is what makes Ctrl+Z
    /// work — but it has been deleted as far as the user is concerned. Left in by either source it
    /// would show as "only on this side" and be offered for deletion a second time.
    /// </summary>
    [Fact]
    public async Task AHeldDeleteIsInvisibleToBothSources()
    {
        File_("a", "left", "kept.txt");
        File_("a", "right", "kept.txt");
        File_("gone", "left", ".bertbrowser-trash", "delete-1", "removed.txt");

        var result = await AgreeAsync(Dir("left"), Dir("right"));

        Assert.False(result.AnyDifference);
        Assert.DoesNotContain(result.ByRelativeKey.Keys, k => k.Contains("REMOVED"));
    }

    /// <summary>
    /// Nothing is compared by content, so two files of the same length whose bytes differ are
    /// "same" once their timestamps match — and both sources have to say so, since a walk that
    /// reached for the bytes and an index that could not would part company immediately.
    /// </summary>
    [Fact]
    public async Task SameLengthDifferentBytesReadsTheSameToBothSources()
    {
        var stamp = new DateTime(2026, 1, 2, 3, 4, 5, DateTimeKind.Utc);
        File.SetLastWriteTimeUtc(File_("aaaa", "left", "a.txt"), stamp);
        File.SetLastWriteTimeUtc(File_("bbbb", "right", "a.txt"), stamp);

        Assert.Equal(CompareVerdict.Same, (await AgreeAsync(Dir("left"), Dir("right"))).For("A.TXT"));
    }

    [Fact]
    public async Task AFileAgainstAFolderOfTheSameNameAgrees()
    {
        File_("x", "left", "thing");
        File_("y", "right", "thing", "inner.txt");

        var result = await AgreeAsync(Dir("left"), Dir("right"));

        Assert.Equal(CompareVerdict.Differs, result.For("THING"));
    }

    /// <summary>Reports every volume indexed, so the service takes its SQLite path. The other half
    /// of the matrix is <see cref="NullMftIndexService"/>, which reports none.</summary>
    private sealed class AlwaysIndexed : IMftIndexService
    {
        public void Start() { }
        public bool AnyIndexed => true;
        public bool IsBuilding => false;
        public IReadOnlyCollection<string> BuildingDrives => [];
        public bool IsIndexed(string pathKey) => true;
        public string StatusText => "";
        public bool CanRetry => false;
        public void Retry() { }
        public event Action<string>? IndexRefreshed { add { } remove { } }
        public event Action? StatusChanged { add { } remove { } }
        public void Dispose() { }
    }
}
