using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.DiskUsage;
using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Real temp database, real temp directory tree. The point of these is the seam between the two:
/// file sizes come from the enumeration and folder totals come from dir_size_cache, and a folder
/// with no cached row has to survive all the way out as null.
/// </summary>
public sealed class DiskUsageServiceTests : IDisposable
{
    private readonly string _dbPath;
    private readonly string _root;
    private readonly FsIndexRepository _index;
    private readonly DirSizeRepository _dirSizes;

    public DiskUsageServiceTests()
    {
        _dbPath = Path.Combine(Path.GetTempPath(), $"bertbrowser-test-{Guid.NewGuid():N}.db");
        var db = new Db(_dbPath);
        db.Migrate();
        _index = new FsIndexRepository(db);
        _dirSizes = new DirSizeRepository(db);

        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-du-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        foreach (var f in Directory.GetFiles(Path.GetDirectoryName(_dbPath)!, Path.GetFileName(_dbPath) + "*"))
            File.Delete(f);
        if (Directory.Exists(_root)) Directory.Delete(_root, recursive: true);
    }

    private DiskUsageService Service(IMftIndexService? mft = null) =>
        new(_index, _dirSizes, new FileSystemService(), mft ?? new NullMftIndexService());

    private string MakeFile(string name, int bytes)
    {
        var path = Path.Combine(_root, name);
        File.WriteAllBytes(path, new byte[bytes]);
        return path;
    }

    private string MakeDir(string name)
    {
        var path = Path.Combine(_root, name);
        Directory.CreateDirectory(path);
        return path;
    }

    private void CacheSize(string path, long bytes, bool incomplete = false) =>
        _dirSizes.UpsertMany([new DirSizeResult(
            PathKey.Canonicalize(path), bytes, 1, 0, incomplete, DateTime.UtcNow)]);

    /// <summary>
    /// The invariant this whole feature is shaped around: a folder nobody measured comes back as
    /// null, not 0. Anything that renders it as a number is claiming it is empty.
    /// </summary>
    [Fact]
    public async Task AnUnmeasuredFolderIsUnknown_NotZero()
    {
        MakeDir("Unmeasured");

        var breakdown = await Service().BreakdownAsync(_root, includeHidden: true, CancellationToken.None);

        var child = Assert.Single(breakdown.Children);
        Assert.Equal("Unmeasured", child.Name);
        Assert.Null(child.SizeBytes);
        Assert.Equal(1, breakdown.UnknownChildCount);
    }

    /// <summary>File sizes come from the enumeration, so the file half of a breakdown is complete
    /// on a volume nothing has ever indexed — which is what NullMftIndexService is here.</summary>
    [Fact]
    public async Task FileSizesNeedNoIndexAtAll()
    {
        MakeFile("big.bin", 4_096);
        MakeFile("small.bin", 16);

        var breakdown = await Service().BreakdownAsync(_root, includeHidden: true, CancellationToken.None);

        Assert.Equal(["big.bin", "small.bin"], breakdown.Children.Select(c => c.Name));
        Assert.Equal(4_096, breakdown.Children[0].SizeBytes);
        Assert.Equal(16, breakdown.Children[1].SizeBytes);
        Assert.Equal(DiskUsageAvailability.Ready, breakdown.Availability);
    }

    [Fact]
    public async Task CachedFolderTotalsAreUsedAndSortLargestFirst()
    {
        CacheSize(MakeDir("Small"), 500);
        CacheSize(MakeDir("Large"), 900_000);
        MakeFile("middling.bin", 1_000);

        var breakdown = await Service().BreakdownAsync(_root, includeHidden: true, CancellationToken.None);

        Assert.Equal(["Large", "middling.bin", "Small"], breakdown.Children.Select(c => c.Name));
        Assert.Equal(900_000, breakdown.Children[0].SizeBytes);
    }

    /// <summary>Unknowns sort last: they have no area to give, so they belong out of the way rather
    /// than interleaved as though they were zero-sized.</summary>
    [Fact]
    public async Task UnknownFoldersSortAfterEverythingMeasured()
    {
        MakeDir("Unmeasured");
        CacheSize(MakeDir("Measured"), 10);
        MakeFile("empty.bin", 0);

        var breakdown = await Service().BreakdownAsync(_root, includeHidden: true, CancellationToken.None);

        Assert.Equal("Unmeasured", breakdown.Children[^1].Name);
    }

    [Fact]
    public async Task IncompleteTotalsArePropagated()
    {
        CacheSize(MakeDir("Partial"), 42, incomplete: true);

        var breakdown = await Service().BreakdownAsync(_root, includeHidden: true, CancellationToken.None);

        Assert.True(Assert.Single(breakdown.Children).Incomplete);
    }

    [Fact]
    public async Task HiddenChildrenAreExcludedWhenNotRequested()
    {
        var hidden = MakeFile("hidden.bin", 8);
        File.SetAttributes(hidden, File.GetAttributes(hidden) | FileAttributes.Hidden);
        MakeFile("visible.bin", 8);

        var all = await Service().BreakdownAsync(_root, includeHidden: true, CancellationToken.None);
        var visible = await Service().BreakdownAsync(_root, includeHidden: false, CancellationToken.None);

        Assert.Equal(2, all.Children.Count);
        Assert.Equal("visible.bin", Assert.Single(visible.Children).Name);
    }

    /// <summary>An unknown child makes the remainder incomputable, and the breakdown has to carry
    /// that through rather than quietly counting it as nothing.</summary>
    [Fact]
    public async Task UnaccountedIsNullWhenAChildIsUnknown()
    {
        CacheSize(_root, 10_000);
        MakeDir("Unmeasured");

        var breakdown = await Service().BreakdownAsync(_root, includeHidden: true, CancellationToken.None);

        Assert.Equal(10_000, breakdown.TotalBytes);
        Assert.Null(breakdown.UnaccountedBytes);
    }

    [Fact]
    public async Task UnaccountedIsTheRemainderWhenEverythingIsMeasured()
    {
        CacheSize(_root, 10_000);
        CacheSize(MakeDir("Sub"), 6_000);
        MakeFile("loose.bin", 1_000);

        var breakdown = await Service().BreakdownAsync(_root, includeHidden: true, CancellationToken.None);

        Assert.Equal(3_000, breakdown.UnaccountedBytes);
    }

    // --- Largest files ---

    [Fact]
    public async Task LargestFilesComesBackOrderedWithRealSizes()
    {
        _index.UpsertEntries(
        [
            new FsEntryRow(PathKey.Canonicalize(@"C:\Data\a.bin"), "a.bin", false, 10, DateTime.UtcNow),
            new FsEntryRow(PathKey.Canonicalize(@"C:\Data\b.bin"), "b.bin", false, 9_000, DateTime.UtcNow),
        ], crawlGen: 1);

        var outcome = await Service(new FakeIndexedMft()).LargestFilesAsync(
            @"C:\Data", limit: 10, includeHidden: true, CancellationToken.None);

        Assert.Equal(DiskUsageAvailability.Ready, outcome.Availability);
        Assert.Equal("b.bin", outcome.Files[0].Name);
    }

    /// <summary>
    /// The sizeless FSCTL_ENUM_USN_DATA shape, end to end: rows exist and every one is zero. The
    /// service must hand back nothing at all rather than leaving each caller to re-decide whether
    /// a screenful of "0 B" is worth drawing.
    /// </summary>
    [Fact]
    public async Task ASizelessVolumeYieldsNoFilesRatherThanAListOfZeros()
    {
        _index.UpsertEntries(
        [
            new FsEntryRow(PathKey.Canonicalize(@"C:\Data\a.bin"), "a.bin", false, 0, DateTime.UtcNow),
            new FsEntryRow(PathKey.Canonicalize(@"C:\Data\b.bin"), "b.bin", false, 0, DateTime.UtcNow),
        ], crawlGen: 1);

        var outcome = await Service(new FakeIndexedMft()).LargestFilesAsync(
            @"C:\Data", limit: 10, includeHidden: true, CancellationToken.None);

        Assert.Equal(DiskUsageAvailability.NoSizeData, outcome.Availability);
        Assert.Empty(outcome.Files);
    }

    [Fact]
    public async Task NothingIndexedIsReportedAsSuch_SoTheViewCanOfferARetry()
    {
        var outcome = await Service().LargestFilesAsync(
            @"C:\Data", limit: 10, includeHidden: true, CancellationToken.None);

        Assert.Equal(DiskUsageAvailability.NotIndexed, outcome.Availability);
        Assert.Empty(outcome.Files);
    }

    /// <summary>Reports every volume complete — the "index is ready" half of the matrix that
    /// NullMftIndexService deliberately never covers.</summary>
    private sealed class FakeIndexedMft : IMftIndexService
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
