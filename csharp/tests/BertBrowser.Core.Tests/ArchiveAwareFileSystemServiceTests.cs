using System.Text;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Archives;
using SharpCompress.Common;
using SharpCompress.Writers;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ArchiveAwareFileSystemServiceTests : IDisposable
{
    private readonly string _root = Path.Combine(
        Path.GetTempPath(), $"bertbrowser-archfs-{Guid.NewGuid():N}");

    private readonly RecordingFileSystem _inner = new();
    private readonly ArchiveAwareFileSystemService _service;

    public ArchiveAwareFileSystemServiceTests()
    {
        Directory.CreateDirectory(_root);
        _service = new ArchiveAwareFileSystemService(_inner, new SharpCompressArchiveReader());
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

    /// <summary>The reason five callers needed no changes: an ordinary path is simply passed on.</summary>
    [Fact]
    public void AnOrdinaryPathGoesStraightToTheInnerService()
    {
        _service.ListDirectory(@"C:\Users\Rob\Documents");

        Assert.Equal([@"C:\Users\Rob\Documents"], _inner.Listed);
    }

    /// <summary>And a virtual one never reaches it, so nothing tries to stat a path that is not there.</summary>
    [Fact]
    public void AVirtualPathNeverReachesTheInnerService()
    {
        var zip = Zip("a.zip", ("src/app.js", "x"));

        _service.ListDirectory(Path.Combine(zip, "src"));

        Assert.Empty(_inner.Listed);
    }

    [Fact]
    public void ListsTheArchiveRootAsIfItWereAFolder()
    {
        var zip = Zip("a.zip", ("readme.txt", "hello"), ("src/app.js", "x"));

        var rows = _service.ListDirectory(zip);

        Assert.Equal(["readme.txt", "src"], rows.Select(r => r.Name).Order().ToArray());
        Assert.Equal(Path.Combine(zip, "src"), rows.Single(r => r.Name == "src").FullPath);
        Assert.True(rows.Single(r => r.Name == "src").IsDirectory);
    }

    [Fact]
    public void ListsAFolderInsideTheArchive()
    {
        var zip = Zip("a.zip", ("src/app.js", "x"), ("src/lib/util.js", "y"));

        var rows = _service.ListDirectory(Path.Combine(zip, "src"));

        Assert.Equal(["app.js", "lib"], rows.Select(r => r.Name).Order().ToArray());
    }

    /// <summary>
    /// A directory inside an archive carries its exact size, which is what stops the file list
    /// asking dir_size_cache about a path that could never have a row.
    /// </summary>
    [Fact]
    public void AFolderInsideAnArchiveCarriesItsExactSizeRatherThanMinusOne()
    {
        var zip = Zip("a.zip", ("src/a.txt", new string('x', 40)), ("src/b.txt", new string('y', 60)));

        var src = _service.ListDirectory(zip).Single();

        Assert.True(src.IsDirectory);
        Assert.Equal(100, src.SizeBytes);
    }

    /// <summary>
    /// IEntry.Attrib holds a DOS byte or a Unix mode depending on the writing tool, and "Show
    /// hidden items" filters on this. Map it and most of a tarball disappears under the default
    /// setting; the payoff for getting it right is only a ghosted icon.
    /// </summary>
    [Fact]
    public void NothingInsideAnArchiveIsHidden()
    {
        var zip = Zip("a.zip", (".gitignore", "node_modules"), (".config/settings.json", "{}"));

        var rows = _service.ListDirectory(zip);

        Assert.Equal([".config", ".gitignore"], rows.Select(r => r.Name).Order().ToArray());
        Assert.All(rows, r => Assert.False(r.Attributes.HasFlag(FileAttributes.Hidden)));
    }

    [Fact]
    public void AnEntryWithNoTimestampRendersBlankRatherThanSixteenOhOne()
    {
        var zip = Zip("a.zip", ("src/app.js", "x"));

        // "src" is synthesized from a path prefix, so it carries no timestamp of its own.
        var src = _service.ListDirectory(zip).Single();

        Assert.Equal(default, src.ModifiedUtc);
    }

    [Fact]
    public void ADamagedArchiveThrowsTheErrorTheFileListAlreadyRenders()
    {
        var path = Path.Combine(_root, "broken.zip");
        File.WriteAllText(path, "this is not a zip file at all");

        var ex = Assert.Throws<IOException>(() => _service.ListDirectory(path));
        Assert.Contains("archive", ex.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void AFolderThatIsNotInTheArchiveIsNotFound()
    {
        var zip = Zip("a.zip", ("src/app.js", "x"));

        Assert.Throws<DirectoryNotFoundException>(
            () => _service.ListDirectory(Path.Combine(zip, "nope")));
    }

    [Fact]
    public void CanListAnswersForBothKindsOfPath()
    {
        var zip = Zip("a.zip", ("src/app.js", "x"));

        Assert.True(_service.CanList(_root));
        Assert.True(_service.CanList(zip));
        Assert.True(_service.CanList(Path.Combine(zip, "src")));
        Assert.False(_service.CanList(Path.Combine(zip, "nope")));
        Assert.False(_service.CanList(Path.Combine(_root, "missing")));
    }

    /// <summary>
    /// A damaged archive is still navigable: the list shows the banner. Refusing at the gate would
    /// mean an archive open on the UI thread, which is what the two-stage gate exists to avoid.
    /// </summary>
    [Fact]
    public void ADamagedArchiveIsStillNavigableSoTheBannerCanExplain()
    {
        var path = Path.Combine(_root, "broken.zip");
        File.WriteAllText(path, "not a zip");

        Assert.True(_service.CanList(path));
    }

    /// <summary>
    /// The probe stays consistent with the listing rather than being made to lie in order to keep
    /// archives out of the folder tree. What actually keeps them out is structural: the tree builds
    /// nodes from ListDirectory filtered on IsDirectory, and an archive is a file.
    /// </summary>
    [Fact]
    public void ProbeAnswersForTheArchiveFileTheWayTheListingDoes()
    {
        var zip = Zip("a.zip", ("src/app.js", "x"));

        Assert.True(_service.ProbeSubdirectories(zip).Any);
        Assert.Empty(_inner.Probed);
    }

    /// <summary>The structural guarantee itself: an archive is never a directory row.</summary>
    [Fact]
    public void AnArchiveIsAFileInItsParentListingSoTheTreeCannotNodeIt()
    {
        Zip("a.zip", ("src/app.js", "x"));
        var real = new FileSystemService();

        var row = real.ListDirectory(_root).Single(e => e.Name == "a.zip");

        Assert.False(row.IsDirectory);
    }

    [Fact]
    public void ProbeAnswersFromTheIndexInsideAnArchive()
    {
        var zip = Zip("a.zip", ("src/lib/util.js", "x"), ("src/app.js", "y"));

        var presence = _service.ProbeSubdirectories(Path.Combine(zip, "src"));

        Assert.True(presence.Any);
        Assert.True(presence.AnyVisible);
    }

    [Fact]
    public void DrivesAlwaysComeFromTheInnerService()
    {
        _service.GetDrives();
        Assert.Equal(1, _inner.DriveCalls);
    }

    /// <summary>Records what it was asked, and answers nothing.</summary>
    private sealed class RecordingFileSystem : IFileSystemService
    {
        public List<string> Listed { get; } = [];
        public List<string> Probed { get; } = [];
        public int DriveCalls { get; private set; }

        public IReadOnlyList<FileEntry> ListDirectory(string path)
        {
            Listed.Add(path);
            return [];
        }

        public SubdirectoryPresence ProbeSubdirectories(string path)
        {
            Probed.Add(path);
            return default;
        }

        public IReadOnlyList<DriveInfo> GetDrives()
        {
            DriveCalls++;
            return [];
        }
    }
}
