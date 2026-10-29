using System.Text;
using BertBrowser.Core.Data;
using BertBrowser.Core.Services;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.DiskUsage;
using BertBrowser.Core.Services.Mft;
using SharpCompress.Common;
using SharpCompress.Writers;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Disk usage pointed at an archive.
/// </summary>
/// <remarks>
/// The point of these is the <em>availability</em>, not the arithmetic. Every size inside a
/// container is exact, so this is the one place the view is never approximate — and the way that
/// could regress is silently: a breakdown that asked <c>dir_size_cache</c> would miss on every
/// folder, report them all unknown, and then have <c>ClassifyBreakdown</c> blame the index.
/// </remarks>
public class DiskUsageInArchiveTests : IDisposable
{
    private readonly string _root = Path.Combine(
        Path.GetTempPath(), $"bertbrowser-duarch-{Guid.NewGuid():N}");

    private readonly Db _db;
    private readonly DiskUsageService _service;
    private readonly string _zip;

    public DiskUsageInArchiveTests()
    {
        Directory.CreateDirectory(_root);
        _zip = Zip("a.zip",
            ("readme.txt", new string('r', 41)),
            ("src/app.js", new string('a', 22)),
            ("src/lib/util.js", new string('u', 22)),
            ("docs/guide.md", new string('g', 33)));

        _db = new Db(Path.Combine(_root, "test.db"));
        _db.Migrate();

        var reader = new SharpCompressArchiveReader();
        var archives = new ArchiveAwareFileSystemService(new FileSystemService(), reader);

        _service = new DiskUsageService(
            new FsIndexRepository(_db), new DirSizeRepository(_db), archives,
            new NullMftIndexService(), archives);
    }

    public void Dispose()
    {
        
        try { Directory.Delete(_root, recursive: true); } catch { /* best effort */ }
    }

    private string Zip(string name, params (string Key, string Body)[] entries)
    {
        var path = Path.Combine(_root, name);
        using var file = File.Create(path);
        using var writer = WriterFactory.Open(file, ArchiveType.Zip, new WriterOptions(CompressionType.Deflate));
        foreach (var (key, body) in entries)
        {
            using var source = new MemoryStream(Encoding.UTF8.GetBytes(body));
            writer.Write(key, source, new DateTime(2026, 2, 3, 4, 5, 6));
        }
        return path;
    }

    /// <summary>
    /// Ready, not NoSizeData — and every folder sized. Nothing here depends on the MFT index, so
    /// there is no "not indexed yet" state for this view to be in.
    /// </summary>
    [Fact]
    public async Task ABreakdownOfAnArchiveIsReadyAndFullySized()
    {
        var breakdown = await _service.BreakdownAsync(_zip, includeHidden: true, CancellationToken.None);

        Assert.Equal(DiskUsageAvailability.Ready, breakdown.Availability);
        Assert.Equal(0, breakdown.UnknownChildCount);
        Assert.All(breakdown.Children, c => Assert.NotNull(c.SizeBytes));

        Assert.Equal(44, breakdown.Children.Single(c => c.Name == "src").SizeBytes);
        Assert.Equal(41, breakdown.Children.Single(c => c.Name == "readme.txt").SizeBytes);
        Assert.Equal(33, breakdown.Children.Single(c => c.Name == "docs").SizeBytes);
        Assert.Equal(118, breakdown.TotalBytes);
    }

    /// <summary>
    /// With every child measured there is nothing left over, so the remainder is a real number
    /// rather than the null that "some child is unknown" produces.
    /// </summary>
    [Fact]
    public async Task NothingIsUnaccountedForInAnArchive()
    {
        var breakdown = await _service.BreakdownAsync(_zip, includeHidden: true, CancellationToken.None);

        Assert.NotNull(breakdown.UnaccountedBytes);
        Assert.Equal(0, breakdown.UnaccountedBytes);
    }

    [Fact]
    public async Task DrillingIntoAFolderInsideTheArchiveWorks()
    {
        var breakdown = await _service.BreakdownAsync(
            Path.Combine(_zip, "src"), includeHidden: true, CancellationToken.None);

        Assert.Equal(DiskUsageAvailability.Ready, breakdown.Availability);
        Assert.Equal(["app.js", "lib"], breakdown.Children.Select(c => c.Name).Order().ToArray());
        Assert.Equal(22, breakdown.Children.Single(c => c.Name == "lib").SizeBytes);
    }

    /// <summary>
    /// Answered from the container's own index rather than fs_entry, which knows nothing about
    /// archive contents — and must never be taught, since its rows are PathKey-keyed.
    /// </summary>
    [Fact]
    public async Task LargestFilesComeFromTheArchiveRatherThanTheIndex()
    {
        var outcome = await _service.LargestFilesAsync(
            _zip, limit: 10, includeHidden: true, CancellationToken.None);

        Assert.Equal(DiskUsageAvailability.Ready, outcome.Availability);
        Assert.Equal(["readme.txt", "guide.md", "util.js", "app.js"],
            outcome.Files.Select(f => f.Name).ToArray());
        Assert.Equal(41, outcome.Files[0].SizeBytes);
    }

    [Fact]
    public async Task TheLimitIsHonoured()
    {
        var outcome = await _service.LargestFilesAsync(
            _zip, limit: 2, includeHidden: true, CancellationToken.None);

        Assert.Equal(2, outcome.Files.Count);
    }

    /// <summary>The hard invariant: nothing about this may write a virtual path into the index.</summary>
    [Fact]
    public async Task NoVirtualPathReachesTheIndex()
    {
        await _service.BreakdownAsync(_zip, includeHidden: true, CancellationToken.None);
        await _service.LargestFilesAsync(_zip, 10, includeHidden: true, CancellationToken.None);

        using var connection = _db.Open();
        using var command = connection.CreateCommand();
        command.CommandText = "SELECT COUNT(*) FROM fs_entry";

        Assert.Equal(0L, Convert.ToInt64(command.ExecuteScalar()));
    }

}
