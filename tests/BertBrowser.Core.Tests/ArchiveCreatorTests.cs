using System.Text;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ArchiveCreatorTests : IDisposable
{
    private readonly string _root = Path.Combine(
        Path.GetTempPath(), $"bertbrowser-create-{Guid.NewGuid():N}");

    private readonly ArchiveCreator _creator = new();
    private readonly SharpCompressArchiveReader _reader = new();

    public ArchiveCreatorTests() => Directory.CreateDirectory(_root);

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch { /* best effort */ }
    }

    private string Make(string relative, string body)
    {
        var path = Path.Combine(_root, relative.Replace('/', Path.DirectorySeparatorChar));
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, body);
        return path;
    }

    /// <summary>The round trip: what went in comes back out, byte for byte.</summary>
    [Theory]
    [InlineData(ArchiveWriteFormat.Zip, ".zip")]
    [InlineData(ArchiveWriteFormat.Tar, ".tar")]
    [InlineData(ArchiveWriteFormat.TarGz, ".tar.gz")]
    [InlineData(ArchiveWriteFormat.TarBz2, ".tar.bz2")]
    public void WhatGoesInComesBackOut(ArchiveWriteFormat format, string suffix)
    {
        Make("stuff/readme.txt", "hello");
        Make("stuff/src/app.js", "code");

        var sources = ArchiveSourceWalk.Collect([Path.Combine(_root, "stuff")], includeHidden: true);
        var target = Path.Combine(_root, "out" + suffix);

        var outcome = _creator.Create(target, format, CompressionLevel.Normal, sources);

        Assert.True(outcome.Failed.Count == 0, string.Join(" | ", outcome.Failed));
        Assert.Equal(2, outcome.FilesWritten);
        Assert.False(outcome.Cancelled);
        Assert.True(File.Exists(target));

        var index = _reader.Read(target, null);
        Assert.True(index.Ok, index.Error);
        Assert.Equal(2, index.FileCount);
        Assert.NotNull(index.Find(@"stuff\readme.txt"));
        Assert.NotNull(index.Find(@"stuff\src\app.js"));

        var bytes = _reader.ReadEntry(target, @"stuff\readme.txt", 1024, null);
        Assert.Equal("hello", Encoding.UTF8.GetString(bytes!));
    }

    /// <summary>
    /// The three words in the dialog have to mean three things. Store and Deflate were the only two
    /// the writer ever saw for a while, so Normal and Maximum produced byte-identical archives.
    /// </summary>
    [Fact]
    public void MaximumAsksTheDeflaterForMoreThanNormalDoes()
    {
        var normal = ArchiveCreator.OptionsFor(ArchiveWriteFormat.Zip, CompressionLevel.Normal);
        var maximum = ArchiveCreator.OptionsFor(ArchiveWriteFormat.Zip, CompressionLevel.Maximum);

        Assert.Equal(SharpCompress.Common.CompressionType.Deflate, normal.CompressionType);
        Assert.Equal(SharpCompress.Common.CompressionType.Deflate, maximum.CompressionType);
        Assert.Equal(9, maximum.CompressionLevel); // deflate's best
        Assert.True(maximum.CompressionLevel > normal.CompressionLevel,
            "Maximum has to try harder than Normal, or the dialog is offering a distinction that does not exist.");
    }

    /// <summary>The same thing measured: on compressible data, Maximum is never larger than Normal, and Store is larger than both.</summary>
    [Fact]
    public void MaximumIsNoLargerThanNormalAndStoreIsLargerThanBoth()
    {
        var rng = new Random(7);
        var words = new[] { "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta" };
        var text = new StringBuilder();
        for (var i = 0; i < 60_000; i++) text.Append(words[rng.Next(words.Length)]).Append(rng.Next(100)).Append(' ');
        Make("stuff/corpus.txt", text.ToString());
        var sources = ArchiveSourceWalk.Collect([Path.Combine(_root, "stuff")], includeHidden: true);

        long SizeAt(CompressionLevel level, string name)
        {
            var target = Path.Combine(_root, name);
            _creator.Create(target, ArchiveWriteFormat.Zip, level, sources);
            return new FileInfo(target).Length;
        }

        var store = SizeAt(CompressionLevel.Store, "store.zip");
        var normal = SizeAt(CompressionLevel.Normal, "normal.zip");
        var maximum = SizeAt(CompressionLevel.Maximum, "maximum.zip");

        Assert.True(normal < store);
        Assert.True(maximum <= normal);
    }

    /// <summary>A stored zip is still readable — it is the right answer for already-compressed data.</summary>
    [Fact]
    public void StoreProducesAReadableArchive()
    {
        Make("stuff/a.txt", "hello");
        var sources = ArchiveSourceWalk.Collect([Path.Combine(_root, "stuff")], includeHidden: true);
        var target = Path.Combine(_root, "stored.zip");

        _creator.Create(target, ArchiveWriteFormat.Zip, CompressionLevel.Store, sources);

        var index = _reader.Read(target, null);
        Assert.True(index.Ok);
        Assert.Equal(1, index.FileCount);
    }

    /// <summary>
    /// The reason it writes to a .bertbrowser-partial name first: a cancel must not leave a
    /// truncated file under the name every other tool on the machine will try to open.
    /// </summary>
    [Fact]
    public void ACancelledCreateLeavesNothingBehind()
    {
        for (var i = 0; i < 40; i++) Make($"stuff/f{i}.txt", new string('x', 40_000));
        var sources = ArchiveSourceWalk.Collect([Path.Combine(_root, "stuff")], includeHidden: true);
        var target = Path.Combine(_root, "cancelled.zip");

        using var cts = new CancellationTokenSource();
        var seen = 0;
        var progress = new SynchronousProgress(_ => { if (++seen > 3) cts.Cancel(); });

        var outcome = _creator.Create(
            target, ArchiveWriteFormat.Zip, CompressionLevel.Normal, sources, cts.Token, progress);

        Assert.True(outcome.Cancelled);
        Assert.False(File.Exists(target));
        Assert.False(File.Exists(target + ArchiveCreator.PartialSuffix));
        Assert.Empty(Directory.GetFiles(_root, "*" + ArchiveCreator.PartialSuffix));
    }

    /// <summary>The meta-test: the "nothing left behind" check can actually fail.</summary>
    [Fact]
    public void TheLeftoverCheckNoticesAPartialFile()
    {
        File.WriteAllText(Path.Combine(_root, "x.zip" + ArchiveCreator.PartialSuffix), "half");
        Assert.NotEmpty(Directory.GetFiles(_root, "*" + ArchiveCreator.PartialSuffix));
    }

    [Fact]
    public void ProgressReachesTheTotalAndIsMonotonic()
    {
        Make("stuff/a.txt", new string('x', 20_000));
        Make("stuff/b.txt", new string('y', 20_000));
        var sources = ArchiveSourceWalk.Collect([Path.Combine(_root, "stuff")], includeHidden: true);

        var seen = new List<long>();
        var progress = new SynchronousProgress(p => seen.Add(p.BytesDone));

        _creator.Create(Path.Combine(_root, "p.zip"), ArchiveWriteFormat.Zip,
            CompressionLevel.Normal, sources, progress: progress);

        Assert.NotEmpty(seen);
        Assert.Equal(seen.OrderBy(b => b), seen);
        Assert.Equal(40_000, seen[^1]);
    }

    /// <summary>The browse setting decides what goes in, so the archive matches what was on show.</summary>
    [Fact]
    public void HiddenFilesFollowTheBrowseSetting()
    {
        Make("stuff/visible.txt", "a");
        var hidden = Make("stuff/secret.txt", "b");
        File.SetAttributes(hidden, FileAttributes.Hidden);

        var without = ArchiveSourceWalk.Collect([Path.Combine(_root, "stuff")], includeHidden: false);
        var with = ArchiveSourceWalk.Collect([Path.Combine(_root, "stuff")], includeHidden: true);

        Assert.Single(without);
        Assert.Equal(2, with.Count);
    }

    [Fact]
    public void ASelectedFolderKeepsItsOwnNameAtTheTop()
    {
        Make("stuff/deep/a.txt", "x");
        var sources = ArchiveSourceWalk.Collect([Path.Combine(_root, "stuff")], includeHidden: true);

        Assert.Equal("stuff/deep/a.txt", sources.Single().EntryName);
    }

    [Fact]
    public void SeveralLooseFilesGoInSideBySide()
    {
        var a = Make("a.txt", "1");
        var b = Make("b.txt", "2");

        var sources = ArchiveSourceWalk.Collect([a, b], includeHidden: true);

        Assert.Equal(["a.txt", "b.txt"], sources.Select(s => s.EntryName).Order().ToArray());
    }

    [Theory]
    [InlineData(".7z")]
    [InlineData(".rar")]
    [InlineData(".tar.xz")]
    public void FormatsThatCannotBeWrittenSayWhyByName(string suffix)
    {
        var why = ArchiveWriteRules.WhyNotWritable(suffix);

        Assert.NotNull(why);
        Assert.Contains("read", why!, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void EveryOfferedFormatReallyProducesSomethingReadable()
    {
        Make("stuff/a.txt", "hello");
        var sources = ArchiveSourceWalk.Collect([Path.Combine(_root, "stuff")], includeHidden: true);

        foreach (var info in ArchiveWriteRules.Formats)
        {
            var target = Path.Combine(_root, "each-" + info.Format + info.Suffix);
            var outcome = _creator.Create(target, info.Format, CompressionLevel.Normal, sources);

            Assert.Empty(outcome.Failed);
            Assert.True(_reader.Read(target, null).Ok, $"{info.Label} did not read back");
            // And the suffix it advertises is one the browse table recognises, or the archive it
            // just made could not be entered.
            Assert.True(ArchiveFormats.IsArchiveName("x" + info.Suffix), info.Suffix);
        }
    }

    private sealed class SynchronousProgress(Action<TransferProgress> report)
        : IProgress<TransferProgress>
    {
        public void Report(TransferProgress value) => report(value);
    }
}
