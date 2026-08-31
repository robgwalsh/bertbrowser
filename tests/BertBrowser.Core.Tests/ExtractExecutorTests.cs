using System.Text;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Transfer;
using SharpCompress.Common;
using SharpCompress.Writers;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ExtractExecutorTests : IDisposable
{
    private readonly string _root = Path.Combine(
        Path.GetTempPath(), $"bertbrowser-extract-{Guid.NewGuid():N}");

    private readonly SharpCompressArchiveReader _reader = new();
    private readonly ExtractPlanner _planner = new();

    private string Dest => Path.Combine(_root, "out");

    public ExtractExecutorTests()
    {
        Directory.CreateDirectory(_root);
        Directory.CreateDirectory(Dest);
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

    private ExtractOutcome Run(string archive, ArchiveIndex index, string relativeTo = "",
        IReadOnlyList<string>? entries = null,
        ExtractConflict conflict = ExtractConflict.KeepBoth,
        CancellationToken ct = default,
        IArchiveReader? reader = null)
    {
        var plan = _planner.Plan(index, archive, relativeTo, entries ?? [], Dest, conflict);
        // Rejected, not HasWork: a plan where every entry was skipped as a conflict has nothing to
        // write and is still a perfectly good plan.
        Assert.Null(plan.Rejected?.Message);
        return new ExtractExecutor(reader ?? _reader).Execute(plan, password: null, ct);
    }

    [Fact]
    public void ExtractsEveryFileWithItsContents()
    {
        var zip = Zip("a.zip", ("readme.txt", "hello"), ("src/app.js", "code"), ("src/lib/util.js", "util"));
        var index = _reader.Read(zip, null);

        var outcome = Run(zip, index);

        Assert.Equal(3, outcome.FilesWritten);
        Assert.False(outcome.Cancelled);
        Assert.Empty(outcome.Failed);
        Assert.Equal("hello", File.ReadAllText(Path.Combine(Dest, "readme.txt")));
        Assert.Equal("code", File.ReadAllText(Path.Combine(Dest, "src", "app.js")));
        Assert.Equal("util", File.ReadAllText(Path.Combine(Dest, "src", "lib", "util.js")));
    }

    [Fact]
    public void ExtractsOnlyWhatWasNamed()
    {
        var zip = Zip("a.zip", ("keep.txt", "yes"), ("skip.txt", "no"));
        var index = _reader.Read(zip, null);

        Run(zip, index, entries: ["keep.txt"]);

        Assert.True(File.Exists(Path.Combine(Dest, "keep.txt")));
        Assert.False(File.Exists(Path.Combine(Dest, "skip.txt")));
    }

    /// <summary>Extracting a folder from inside it drops the prefix, so you get its contents.</summary>
    [Fact]
    public void ExtractingFromInsideAFolderDoesNotRepeatItsName()
    {
        var zip = Zip("a.zip", ("src/app.js", "code"), ("src/lib/util.js", "util"));
        var index = _reader.Read(zip, null);

        Run(zip, index, relativeTo: "src");

        Assert.True(File.Exists(Path.Combine(Dest, "app.js")));
        Assert.True(File.Exists(Path.Combine(Dest, "lib", "util.js")));
        Assert.False(Directory.Exists(Path.Combine(Dest, "src")));
    }

    [Fact]
    public void AnEmptyFolderInTheArchiveIsCreated()
    {
        var path = Path.Combine(_root, "e.zip");
        using (var file = File.Create(path))
        using (var writer = WriterFactory.Open(file, ArchiveType.Zip, new WriterOptions(CompressionType.Deflate)))
        {
            using var source = new MemoryStream(Encoding.UTF8.GetBytes("x"));
            writer.Write("outer/inner/deep.txt", source, DateTime.Now);
        }

        var index = _reader.Read(path, null);
        Run(path, index);

        Assert.True(Directory.Exists(Path.Combine(Dest, "outer", "inner")));
    }

    [Fact]
    public void SkipLeavesWhatIsAlreadyThere()
    {
        var zip = Zip("a.zip", ("readme.txt", "from the archive"));
        File.WriteAllText(Path.Combine(Dest, "readme.txt"), "mine");
        var index = _reader.Read(zip, null);

        var outcome = Run(zip, index, conflict: ExtractConflict.Skip);

        Assert.Equal(0, outcome.FilesWritten);
        Assert.Equal("mine", File.ReadAllText(Path.Combine(Dest, "readme.txt")));
    }

    [Fact]
    public void KeepBothWritesBesideIt()
    {
        var zip = Zip("a.zip", ("readme.txt", "from the archive"));
        File.WriteAllText(Path.Combine(Dest, "readme.txt"), "mine");
        var index = _reader.Read(zip, null);

        Run(zip, index, conflict: ExtractConflict.KeepBoth);

        Assert.Equal("mine", File.ReadAllText(Path.Combine(Dest, "readme.txt")));
        Assert.Equal("from the archive", File.ReadAllText(Path.Combine(Dest, "readme (2).txt")));
    }

    /// <summary>
    /// The rule this whole class is shaped around. A cancelled extract removes exactly what it
    /// created — and an extract routinely lands in a folder the user already has files in, so
    /// clearing the destination tree the way a cancelled copy does would delete their work.
    /// </summary>
    [Fact]
    public void ACancelledExtractLeavesTheUsersOwnFilesInTheDestination()
    {
        var zip = Zip("a.zip",
            ("one.txt", "1"), ("two.txt", "2"), ("three.txt", "3"), ("four.txt", "4"));
        var mine = Path.Combine(Dest, "mine.txt");
        File.WriteAllText(mine, "do not touch");
        Directory.CreateDirectory(Path.Combine(Dest, "myfolder"));
        File.WriteAllText(Path.Combine(Dest, "myfolder", "deep.txt"), "also mine");

        var index = _reader.Read(zip, null);
        using var cts = new CancellationTokenSource();
        var stepped = new SteppedReader(_reader, afterEntry: n => { if (n >= 2) cts.Cancel(); });

        var outcome = Run(zip, index, ct: cts.Token, reader: stepped);

        Assert.True(outcome.Cancelled);
        Assert.Equal("do not touch", File.ReadAllText(mine));
        Assert.Equal("also mine", File.ReadAllText(Path.Combine(Dest, "myfolder", "deep.txt")));
    }

    /// <summary>And it takes back everything it did add, so a cancel is not half an extract.</summary>
    [Fact]
    public void ACancelledExtractTakesBackWhatItAdded()
    {
        var zip = Zip("a.zip",
            ("d/one.txt", "1"), ("d/two.txt", "2"), ("d/three.txt", "3"), ("d/four.txt", "4"));
        var index = _reader.Read(zip, null);

        using var cts = new CancellationTokenSource();
        var stepped = new SteppedReader(_reader, afterEntry: n => { if (n >= 2) cts.Cancel(); });

        var outcome = Run(zip, index, ct: cts.Token, reader: stepped);

        Assert.True(outcome.Cancelled);
        Assert.False(Directory.Exists(Path.Combine(Dest, "d")));
        Assert.Empty(Directory.GetFileSystemEntries(Dest));
    }

    /// <summary>The meta-test: the check above can actually fail.</summary>
    [Fact]
    public void TheEmptyDestinationCheckNoticesALeftoverFile()
    {
        File.WriteAllText(Path.Combine(Dest, "leftover.txt"), "x");
        Assert.NotEmpty(Directory.GetFileSystemEntries(Dest));
    }

    [Fact]
    public void ByteTotalsAreExactForAnAddressableArchive()
    {
        var zip = Zip("a.zip", ("a.txt", new string('x', 100)), ("b.txt", new string('y', 50)));
        var index = _reader.Read(zip, null);

        var plan = _planner.Plan(index, zip, "", [], Dest, ExtractConflict.KeepBoth);

        Assert.True(plan.BytesAreExact);
        Assert.Equal(150, plan.TotalBytes);

        var outcome = new ExtractExecutor(_reader).Execute(plan);
        Assert.Equal(150, outcome.BytesWritten);
    }

    [Fact]
    public void ProgressReachesTheTotalAndIsMonotonic()
    {
        var zip = Zip("a.zip", ("a.txt", new string('x', 5000)), ("b.txt", new string('y', 5000)));
        var index = _reader.Read(zip, null);
        var plan = _planner.Plan(index, zip, "", [], Dest, ExtractConflict.KeepBoth);

        var seen = new List<long>();
        var progress = new SynchronousProgress(p => seen.Add(p.BytesDone));

        new ExtractExecutor(_reader).Execute(plan, progress: progress);

        Assert.NotEmpty(seen);
        Assert.Equal(seen.OrderBy(b => b), seen);
        Assert.Equal(10000, seen[^1]);
    }

    /// <summary>Reports rather than throws, and never leaves the half-written file behind.</summary>
    [Fact]
    public void AnUnwritableDestinationIsReportedAndCostsTheOthersNothing()
    {
        var zip = Zip("a.zip", ("ok.txt", "fine"), ("blocked.txt", "nope"));
        // A directory where the file wants to be: creating the file throws, and nothing else cares.
        Directory.CreateDirectory(Path.Combine(Dest, "blocked.txt"));

        var index = _reader.Read(zip, null);
        var outcome = Run(zip, index, conflict: ExtractConflict.Skip);

        Assert.Equal("fine", File.ReadAllText(Path.Combine(Dest, "ok.txt")));
        Assert.Equal(1, outcome.FilesWritten);
    }

    /// <summary>
    /// Wraps the real reader and runs a hook between entries, so a cancel lands at a known point
    /// in milliseconds — the SteppedCopier pattern the transfer tests use.
    /// </summary>
    private sealed class SteppedReader(IArchiveReader inner, Action<int> afterEntry) : IArchiveReader
    {
        public ArchiveIndex Read(string archiveFile, string? password, CancellationToken ct = default) =>
            inner.Read(archiveFile, password, ct);

        public byte[]? ReadEntry(string archiveFile, string entryPath, long maxBytes, string? password,
            CancellationToken ct = default) =>
            inner.ReadEntry(archiveFile, entryPath, maxBytes, password, ct);

        public void ReadEntries(string archiveFile, IReadOnlyCollection<string> entryPaths,
            string? password, Action<string, Stream, long> onEntry, CancellationToken ct = default)
        {
            var n = 0;
            inner.ReadEntries(archiveFile, entryPaths, password, (key, content, size) =>
            {
                onEntry(key, content, size);
                afterEntry(++n);
                ct.ThrowIfCancellationRequested();
            }, ct);
        }
    }

    /// <summary>Progress&lt;T&gt; posts to a synchronization context; these tests want it inline.</summary>
    private sealed class SynchronousProgress(Action<TransferProgress> report)
        : IProgress<TransferProgress>
    {
        public void Report(TransferProgress value) => report(value);
    }
}
