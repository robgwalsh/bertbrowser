using BertBrowser.Core.Services.Archives;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ArchivePathTests
{
    /// <summary>Every path the tests below call an archive.</summary>
    private static readonly Func<string, bool> RealZips = p =>
        p.Equals(@"C:\stuff\a.zip", StringComparison.OrdinalIgnoreCase) ||
        p.Equals(@"C:\stuff\backup.tar.gz", StringComparison.OrdinalIgnoreCase) ||
        p.Equals(@"C:\stuff\outer.zip", StringComparison.OrdinalIgnoreCase);

    [Theory]
    [InlineData(@"C:\stuff\a.zip\src", @"C:\stuff\a.zip", "src")]
    [InlineData(@"C:\stuff\a.zip\src\lib\util.js", @"C:\stuff\a.zip", @"src\lib\util.js")]
    [InlineData(@"C:\stuff\backup.tar.gz\etc", @"C:\stuff\backup.tar.gz", "etc")]
    public void SplitsIntoTheContainerAndTheEntry(string path, string archive, string entry)
    {
        var parsed = ArchivePath.Parse(path, RealZips);

        Assert.NotNull(parsed);
        Assert.Equal(archive, parsed!.Value.ArchiveFile);
        Assert.Equal(entry, parsed.Value.EntryPath);
    }

    [Fact]
    public void AnOrdinaryFolderIsNotAnArchivePath()
    {
        Assert.Null(ArchivePath.Parse(@"C:\Users\Rob\Documents", RealZips));
    }

    /// <summary>
    /// The reason existence is a delegate rather than something the parser decides for itself.
    /// A folder may perfectly well be named "photos.zip".
    /// </summary>
    [Fact]
    public void AFolderNamedLikeAnArchiveIsNotOne()
    {
        Assert.Null(ArchivePath.Parse(@"C:\stuff\photos.zip\holiday", _ => false));
    }

    /// <summary>
    /// The archive file itself parses, with an empty entry path — that is what navigating into one
    /// looks like before you have gone anywhere inside it.
    /// </summary>
    [Fact]
    public void TheArchiveItselfIsItsOwnRoot()
    {
        // The container has no trailing segment, so Parse cannot see it; the root is composed.
        var root = new ArchivePath(@"C:\stuff\a.zip", "");

        Assert.True(root.IsRoot);
        Assert.Null(root.Parent);
        Assert.Equal(@"C:\stuff\a.zip", root.ToString());
    }

    /// <summary>
    /// Zip Slip, spelled as a path rather than as an entry key. GetFullPath would turn this into
    /// C:\Windows — a real folder — so the refusal has to happen on the raw string.
    /// </summary>
    [Theory]
    [InlineData(@"C:\stuff\a.zip\..\..\Windows")]
    [InlineData(@"C:\stuff\a.zip\src\..\..\..\Windows\System32")]
    [InlineData(@"C:\stuff\a.zip\.\src")]
    public void APathThatClimbsOutOfTheArchiveIsRefused(string path)
    {
        Assert.False(ArchivePath.IsAcceptable(path));
        Assert.Null(ArchivePath.Parse(path, RealZips));
    }

    /// <summary>
    /// A zip inside a zip would have to be written out before it could be opened, which means
    /// creating a file nobody asked for.
    /// </summary>
    [Fact]
    public void ANestedArchiveIsRefusedRatherThanEntered()
    {
        Assert.Null(ArchivePath.Parse(@"C:\stuff\outer.zip\inner.zip", RealZips));
    }

    /// <summary>
    /// Shortest prefix wins: "outer.zip" is a file on disk and "outer.zip\inner.zip" is not, so the
    /// inner name is an entry. Anything else would misparse it as a second container.
    /// </summary>
    [Fact]
    public void TheOutermostArchiveWins()
    {
        var parsed = ArchivePath.Parse(@"C:\stuff\outer.zip\inner.zip\notes.txt", RealZips);

        Assert.NotNull(parsed);
        Assert.Equal(@"C:\stuff\outer.zip", parsed!.Value.ArchiveFile);
        Assert.Equal(@"inner.zip\notes.txt", parsed.Value.EntryPath);
    }

    [Theory]
    [InlineData(@"C:\stuff\a.zip\src", true)]
    [InlineData(@"C:\stuff\backup.tar.gz\etc\hosts", true)]
    [InlineData(@"C:\Users\Rob\Documents", false)]
    [InlineData(@"C:\stuff\a.zip", true)]
    [InlineData("", false)]
    public void LooksVirtualIsAPureSegmentScan(string path, bool expected)
    {
        Assert.Equal(expected, ArchivePath.LooksVirtual(path));
    }

    /// <summary>The archive file is its own root — an empty entry path, and a real folder above it.</summary>
    [Fact]
    public void TheArchiveFileParsesAsItsOwnRoot()
    {
        var parsed = ArchivePath.Parse(@"C:\stuff\a.zip", RealZips);

        Assert.NotNull(parsed);
        Assert.Equal(@"C:\stuff\a.zip", parsed!.Value.ArchiveFile);
        Assert.True(parsed.Value.IsRoot);
        Assert.Equal("", parsed.Value.EntryPath);
    }

    /// <summary>
    /// The navigation gate calls this on every path typed into the box, so it must never reach the
    /// disk for an ordinary folder.
    /// </summary>
    [Fact]
    public void LooksVirtualNeverConsultsTheProbe()
    {
        var asked = false;
        ArchivePath.Parse(@"C:\Users\Rob\Documents", _ => { asked = true; return true; });

        Assert.False(ArchivePath.LooksVirtual(@"C:\Users\Rob\Documents"));
        Assert.False(asked);
    }

    [Fact]
    public void ParentWalksUpAndThenStops()
    {
        var deep = new ArchivePath(@"C:\stuff\a.zip", @"src\lib\util.js");

        var lib = deep.Parent;
        Assert.Equal(@"src\lib", lib!.Value.EntryPath);

        var src = lib.Value.Parent;
        Assert.Equal("src", src!.Value.EntryPath);

        var root = src.Value.Parent;
        Assert.True(root!.Value.IsRoot);
        Assert.Null(root.Value.Parent);
    }

    [Theory]
    [InlineData(@"C:\stuff\a.zip", "", @"C:\stuff\a.zip")]
    [InlineData(@"C:\stuff\a.zip", "src", @"C:\stuff\a.zip\src")]
    [InlineData(@"C:\stuff\a.zip", @"\src\lib\", @"C:\stuff\a.zip\src\lib")]
    public void ComposeRoundTrips(string archive, string entry, string expected)
    {
        Assert.Equal(expected, ArchivePath.Compose(archive, entry));
    }

    /// <summary>Deferred to NavigationRequest.IsAcceptablePath rather than restated.</summary>
    [Theory]
    [InlineData(@"a.zip\src")]              // relative
    [InlineData(@"\\.\PhysicalDrive0\x")]   // device path
    [InlineData("C:\\stuff\\a.zip\\sr\u0000c")]
    [InlineData(@"C:\stuff\a.zip\sr*c")]
    public void UnacceptablePathsAreRefused(string path)
    {
        Assert.False(ArchivePath.IsAcceptable(path));
    }
}
