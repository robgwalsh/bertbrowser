using BertBrowser.Core.Services.Archives;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ArchiveIndexBuilderTests
{
    private static RawArchiveEntry File(string key, long size = 100, long packed = 40) =>
        new(key, size, packed, new DateTime(2026, 1, 2, 3, 4, 5), IsDirectory: false);

    private static RawArchiveEntry Dir(string key, DateTime? modified = null) =>
        new(key, 0, 0, modified, IsDirectory: true);

    private static ArchiveIndex Build(params RawArchiveEntry[] entries) =>
        ArchiveIndexBuilder.Build(entries, ArchiveCapabilities.Unknown);

    /// <summary>
    /// The case that matters most: zips routinely carry no folder entries at all. Trusting the
    /// explicit ones would leave "src" in every path and reachable from nowhere.
    /// </summary>
    [Fact]
    public void SrcIsReachableInAZipThatOmitsItsFolderEntries()
    {
        var index = Build(File(@"src/lib/util.js"), File("readme.txt"));

        Assert.NotNull(index.Find("src"));
        Assert.True(index.Find("src")!.IsDirectory);
        Assert.NotNull(index.Find(@"src\lib"));

        var top = index.Children("")!;
        Assert.Equal(["readme.txt", "src"], top.Select(n => n.Name).Order().ToArray());

        var lib = index.Children(@"src\lib")!;
        Assert.Equal(["util.js"], lib.Select(n => n.Name).ToArray());
    }

    [Fact]
    public void SeparatorsBecomeWindowsSeparators()
    {
        var index = Build(File("a/b/c.txt"));

        Assert.NotNull(index.Find(@"a\b\c.txt"));
        Assert.Equal(@"a\b\c.txt", index.Find(@"a\b\c.txt")!.Path);
    }

    /// <summary>An explicit directory entry decorates a node; it must never be what creates one.</summary>
    [Fact]
    public void AnExplicitFolderEntryOnlySuppliesItsTimestamp()
    {
        var stamp = new DateTime(2020, 5, 6, 7, 8, 9);
        var index = Build(File(@"src/lib/util.js"), Dir("src/", stamp));

        Assert.Equal(stamp, index.Find("src")!.Modified);
        // The implicitly created one carries none, and renders blank rather than 1601.
        Assert.Null(index.Find(@"src\lib")!.Modified);
    }

    [Fact]
    public void AnEmptyFolderEntryIsStillAFolder()
    {
        var index = Build(Dir("empty/"));

        var node = index.Find("empty");
        Assert.NotNull(node);
        Assert.True(node!.IsDirectory);
        Assert.Empty(index.Children("empty")!);
        Assert.Equal(0, node.SizeBytes);
    }

    /// <summary>
    /// Zip Slip. Refused at read time so the entry never exists to be extracted — which makes the
    /// extractor's own check a second line rather than the only one.
    /// </summary>
    [Theory]
    [InlineData(@"../../Windows/System32/evil.dll")]
    [InlineData(@"..\..\Windows\System32\evil.dll")]
    [InlineData(@"good/../../../evil.dll")]
    [InlineData(@"/etc/passwd")]
    [InlineData(@"\Windows\evil.dll")]
    [InlineData(@"C:\Windows\evil.dll")]
    [InlineData(@"\\server\share\evil.dll")]
    public void AnEntryThatEscapesTheRootIsRefused(string key)
    {
        var index = Build(File(key), File("safe.txt"));

        Assert.Equal(1, index.FileCount);
        Assert.Equal(1, index.RefusedCount);
        Assert.Equal(["safe.txt"], index.Children("")!.Select(n => n.Name).ToArray());
    }

    [Fact]
    public void AnEntryWithNoNameIsRefusedRatherThanCrashing()
    {
        var index = ArchiveIndexBuilder.Build(
            [new RawArchiveEntry(null, 10, 5, null, false), File("ok.txt")],
            ArchiveCapabilities.Unknown);

        Assert.Equal(1, index.FileCount);
        Assert.Equal(1, index.RefusedCount);
    }

    /// <summary>Directory sizes are the exact recursive sum — nothing is walked to get them.</summary>
    [Fact]
    public void FolderSizesAreTheExactRecursiveSum()
    {
        var index = Build(
            File(@"src/a.txt", size: 100, packed: 30),
            File(@"src/lib/b.txt", size: 250, packed: 70),
            File("top.txt", size: 7, packed: 7));

        Assert.Equal(350, index.Find("src")!.SizeBytes);
        Assert.Equal(250, index.Find(@"src\lib")!.SizeBytes);
        Assert.Equal(357, index.Root.SizeBytes);
        Assert.Equal(100, index.Find("src")!.CompressedBytes);
    }

    /// <summary>
    /// An empty folder inside an archive really is zero bytes. That is a fact, not a missing row,
    /// which is why the never-zero rule that governs dir_size_cache does not apply here.
    /// </summary>
    [Fact]
    public void AnEmptyFolderIsZeroBytesAndThatIsAFact()
    {
        var index = Build(Dir("empty/"));
        Assert.Equal(0, index.Find("empty")!.SizeBytes);
    }

    [Fact]
    public void DuplicateKeysKeepTheLastAndCountTheRest()
    {
        var index = Build(File("dup.txt", size: 1), File("dup.txt", size: 999));

        Assert.Equal(1, index.FileCount);
        Assert.Equal(1, index.RefusedCount);
        Assert.Equal(999, index.Find("dup.txt")!.SizeBytes);
        Assert.Single(index.Children("")!);
    }

    /// <summary>The directory wins: it is reachable and has contents the file would hide.</summary>
    [Fact]
    public void ADirectoryBeatsAFileOfTheSameName()
    {
        var index = Build(File("thing"), File("thing/inner.txt"));

        Assert.True(index.Find("thing")!.IsDirectory);
        Assert.Equal(["inner.txt"], index.Children("thing")!.Select(n => n.Name).ToArray());
        Assert.Equal(1, index.RefusedCount);
    }

    [Fact]
    public void TheEntryCapRefusesRatherThanHalfBuilding()
    {
        var many = Enumerable.Range(0, 20).Select(i => File($"f{i}.txt")).ToArray();

        var index = ArchiveIndexBuilder.Build(many, ArchiveCapabilities.Unknown, maxEntries: 10);

        Assert.False(index.Ok);
        Assert.Equal(ArchiveFailure.TooManyEntries, index.Failure);
        Assert.Contains("10", index.Error);
    }

    [Fact]
    public void ChildrenOfSomethingThatIsNotADirectoryIsNull()
    {
        var index = Build(File("a.txt"));

        Assert.Null(index.Children("a.txt"));
        Assert.Null(index.Children("nope"));
        Assert.NotNull(index.Children(""));
    }

    [Fact]
    public void EntriesComeBackInNameOrder()
    {
        var index = Build(File("zebra.txt"), File("apple.txt"), File("Mango.txt"));

        Assert.Equal(["apple.txt", "Mango.txt", "zebra.txt"],
            index.Children("")!.Select(n => n.Name).ToArray());
    }

    /// <summary>Windows drops a trailing dot or space, so such a key would not round-trip.</summary>
    [Theory]
    [InlineData("bad./x.txt")]
    [InlineData("bad /x.txt")]
    public void ASegmentWindowsWouldRewriteIsCleanedRatherThanTrusted(string key)
    {
        var index = Build(File(key));

        Assert.NotNull(index.Find(@"bad\x.txt"));
    }

    [Fact]
    public void ASymlinkEntryIsListedButCarriesItsTarget()
    {
        var index = ArchiveIndexBuilder.Build(
            [new RawArchiveEntry("link", 0, 0, null, false, LinkTarget: "/etc/passwd")],
            ArchiveCapabilities.Unknown);

        Assert.Equal("/etc/passwd", index.Find("link")!.LinkTarget);
    }

    [Fact]
    public void CapabilitiesRideAlongUnchanged()
    {
        var caps = new ArchiveCapabilities(SequentialOnly: true, IsEncrypted: true, IsComplete: false);

        var index = ArchiveIndexBuilder.Build([File("a.txt")], caps);

        Assert.Equal(caps, index.Capabilities);
    }
}
