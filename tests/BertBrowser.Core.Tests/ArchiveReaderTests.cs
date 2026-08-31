using System.Text;
using BertBrowser.Core.Services.Archives;
using SharpCompress.Common;
using SharpCompress.Writers;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The reader against real containers.
/// </summary>
/// <remarks>
/// The fixtures are written with SharpCompress itself, which makes most of these round-trip tests
/// rather than compatibility tests — say so rather than pretending otherwise. What they do pin, and
/// what no round trip could fake, is the <b>failure</b> behaviour: damaged bytes, a wrong password
/// and a file that only claims to be an archive all have to come back as an index carrying a
/// message, because the whole browsing surface rests on that never throwing.
/// </remarks>
public class ArchiveReaderTests : IDisposable
{
    private readonly string _root = Path.Combine(
        Path.GetTempPath(), $"bertbrowser-archread-{Guid.NewGuid():N}");

    private readonly SharpCompressArchiveReader _reader = new();

    public ArchiveReaderTests() => Directory.CreateDirectory(_root);

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch { /* best effort */ }
    }

    private string Write(ArchiveType type, CompressionType compression, string name,
        params (string Key, string Body)[] entries)
    {
        var path = Path.Combine(_root, name);
        using var file = File.Create(path);
        using var writer = WriterFactory.Open(file, type, new WriterOptions(compression));
        foreach (var (key, body) in entries)
        {
            var bytes = Encoding.UTF8.GetBytes(body);
            using var source = new MemoryStream(bytes);
            writer.Write(key, source, new DateTime(2026, 2, 3, 4, 5, 6));
        }
        return path;
    }

    private string Zip(string name, params (string Key, string Body)[] entries) =>
        Write(ArchiveType.Zip, CompressionType.Deflate, name, entries);

    [Fact]
    public void ReadsAZipIntoATree()
    {
        var path = Zip("a.zip",
            ("readme.txt", "hello"),
            ("src/app.js", "console.log(1)"),
            ("src/lib/util.js", "export {}"));

        var index = _reader.Read(path, password: null);

        Assert.True(index.Ok);
        Assert.Equal(3, index.FileCount);
        Assert.Equal(["readme.txt", "src"], index.Children("")!.Select(n => n.Name).ToArray());
        Assert.Equal(["app.js", "lib"], index.Children("src")!.Select(n => n.Name).ToArray());
        Assert.Equal("export {}".Length, index.Find(@"src\lib\util.js")!.SizeBytes);
    }

    [Fact]
    public void FoldersGetTheirExactRecursiveSize()
    {
        var path = Zip("sizes.zip", ("d/a.txt", new string('x', 100)), ("d/e/b.txt", new string('y', 50)));

        var index = _reader.Read(path, password: null);

        Assert.Equal(150, index.Find("d")!.SizeBytes);
        Assert.Equal(50, index.Find(@"d\e")!.SizeBytes);
    }

    [Fact]
    public void ReadsATarGzWithoutRandomAccess()
    {
        var path = Write(ArchiveType.Tar, CompressionType.GZip, "b.tar.gz",
            ("etc/hosts", "127.0.0.1"), ("etc/motd", "hi"));

        var index = _reader.Read(path, password: null);

        Assert.True(index.Ok);
        Assert.Equal(2, index.FileCount);
        Assert.Equal(["hosts", "motd"], index.Children("etc")!.Select(n => n.Name).ToArray());
    }

    /// <summary>
    /// Through the random-access API this one throws InvalidOperationException outright, which is
    /// why the format table decides which API to use rather than the reader guessing.
    /// </summary>
    [Fact]
    public void ReadsATarBz2()
    {
        var path = Write(ArchiveType.Tar, CompressionType.BZip2, "d.tar.bz2", ("a/b.txt", "body"));

        var index = _reader.Read(path, password: null);

        Assert.True(index.Ok);
        Assert.Equal(["b.txt"], index.Children("a")!.Select(n => n.Name).ToArray());
        Assert.True(index.Capabilities.SequentialOnly);
    }

    /// <summary>A gzipped single file carries the inner name, so syslog.gz browses to syslog.</summary>
    [Fact]
    public void AGzippedFileBrowsesToItsInnerName()
    {
        var path = Write(ArchiveType.GZip, CompressionType.GZip, "syslog.gz", ("syslog", "line one"));

        var index = _reader.Read(path, password: null);

        Assert.True(index.Ok);
        Assert.Equal(["syslog"], index.Children("")!.Select(n => n.Name).ToArray());
    }

    /// <summary>
    /// A container that is not what its name claims is refused rather than browsed. Without this a
    /// zip renamed to .7z would open and list, which is a quiet lie about what the file is.
    /// </summary>
    [Fact]
    public void AZipWearingAnotherSuffixIsRefused()
    {
        var zip = Zip("real.zip", ("a.txt", "x"));
        var lying = Path.Combine(_root, "real.7z");
        File.Move(zip, lying);

        var index = _reader.Read(lying, password: null);

        Assert.False(index.Ok);
        Assert.Equal(ArchiveFailure.Damaged, index.Failure);
    }

    [Fact]
    public void AFileWithNoArchiveSuffixIsNeverOpened()
    {
        var path = Path.Combine(_root, "notes.txt");
        File.WriteAllText(path, "hello");

        var index = _reader.Read(path, password: null);

        Assert.False(index.Ok);
    }

    [Fact]
    public void ReadsAPlainTar()
    {
        var path = Write(ArchiveType.Tar, CompressionType.None, "c.tar", ("only.txt", "x"));

        var index = _reader.Read(path, password: null);

        Assert.True(index.Ok);
        Assert.Equal(1, index.FileCount);
    }

    /// <summary>
    /// The harness's ordinary fixture writes 512,000 bytes of filler and calls it archive.zip, and
    /// three scripts list that folder today. Anything that parses archives has to survive it.
    /// </summary>
    [Fact]
    public void AFileThatOnlyClaimsToBeAnArchiveIsAMessageRatherThanAThrow()
    {
        var path = Path.Combine(_root, "archive.zip");
        File.WriteAllBytes(path, new byte[512_000]);

        var index = _reader.Read(path, password: null);

        Assert.False(index.Ok);
        Assert.Equal(ArchiveFailure.Damaged, index.Failure);
        Assert.NotNull(index.Error);
    }

    [Fact]
    public void ATruncatedArchiveIsAMessageRatherThanAThrow()
    {
        var path = Zip("torn.zip", ("a.txt", new string('q', 5000)));
        var bytes = File.ReadAllBytes(path);
        File.WriteAllBytes(path, bytes[..(bytes.Length / 3)]);

        var index = _reader.Read(path, password: null);

        Assert.False(index.Ok);
        Assert.NotNull(index.Error);
    }

    [Fact]
    public void AMissingArchiveIsAMessageRatherThanAThrow()
    {
        var index = _reader.Read(Path.Combine(_root, "nope.zip"), password: null);

        Assert.False(index.Ok);
        Assert.Equal(ArchiveFailure.Unreadable, index.Failure);
    }

    [Fact]
    public void AnEmptyArchiveReadsAsEmptyRatherThanBroken()
    {
        var path = Zip("empty.zip");

        var index = _reader.Read(path, password: null);

        Assert.True(index.Ok);
        Assert.Equal(0, index.FileCount);
        Assert.Empty(index.Children("")!);
    }

    [Fact]
    public void ReadsOneEntrysBytes()
    {
        var path = Zip("read.zip", ("src/app.js", "console.log(1)"));

        var bytes = _reader.ReadEntry(path, @"src\app.js", 1024, password: null);

        Assert.NotNull(bytes);
        Assert.Equal("console.log(1)", Encoding.UTF8.GetString(bytes!));
    }

    /// <summary>
    /// The budget is what makes a decompression bomb harmless: pulling a megabyte out of a stream
    /// that would have produced ten gigabytes costs a megabyte.
    /// </summary>
    [Fact]
    public void ReadingAnEntryStopsAtTheBudget()
    {
        var path = Zip("big.zip", ("huge.txt", new string('z', 200_000)));

        var bytes = _reader.ReadEntry(path, "huge.txt", 4096, password: null);

        Assert.NotNull(bytes);
        Assert.Equal(4096, bytes!.Length);
    }

    [Fact]
    public void ReadingAnEntryThatIsNotThereIsNull()
    {
        var path = Zip("read.zip", ("a.txt", "x"));

        Assert.Null(_reader.ReadEntry(path, "missing.txt", 1024, password: null));
    }

    /// <summary>
    /// The point of the sharing flags. This app's own rename, move and delete executors are what a
    /// held handle would block, so browsing into an archive must not lock it.
    /// </summary>
    [Fact]
    public void ReadingDoesNotHoldTheArchiveOpen()
    {
        var path = Zip("free.zip", ("a.txt", "x"));

        var index = _reader.Read(path, password: null);
        Assert.True(index.Ok);
        _reader.ReadEntry(path, "a.txt", 1024, password: null);

        var moved = Path.Combine(_root, "moved.zip");
        File.Move(path, moved);          // would throw if a handle were still open
        File.Delete(moved);

        Assert.False(File.Exists(moved));
    }

    [Fact]
    public void TheEntryCapIsReportedRatherThanHalfBuilt()
    {
        var many = Enumerable.Range(0, 30).Select(i => ($"f{i}.txt", "x")).ToArray();
        var path = Zip("many.zip", many);

        var index = new SharpCompressArchiveReader(maxEntries: 10).Read(path, password: null);

        Assert.False(index.Ok);
        Assert.Equal(ArchiveFailure.TooManyEntries, index.Failure);
    }

    /// <summary>
    /// Zip Slip through the real reader, not just the builder: a container really can carry this
    /// key, and the entry must not exist to be extracted.
    /// </summary>
    [Fact]
    public void AnEntryNamedDotDotEvilNeverReachesTheIndex()
    {
        var path = Zip("slip.zip", ("../../evil.dll", "pwned"), ("safe.txt", "ok"));

        var index = _reader.Read(path, password: null);

        Assert.True(index.Ok);
        Assert.Equal(1, index.FileCount);
        Assert.Equal(["safe.txt"], index.Children("")!.Select(n => n.Name).ToArray());
        Assert.True(index.RefusedCount >= 1);
        Assert.Null(_reader.ReadEntry(path, "evil.dll", 1024, password: null));
    }
}
