using BertBrowser.Core.Services.Compare;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// What a comparison refuses before it reads anything. Each of these is a data-safety rule rather
/// than a convenience: a comparison run on the wrong pair is what makes "delete what is only on the
/// right" delete the wrong thing, and it does it silently.
/// </summary>
public sealed class CompareRefusalTests
{
    private static readonly string Left = @"C:\Work\left";
    private static readonly string Right = @"C:\Work\right";

    private static FakeProbe Probe()
    {
        var probe = new FakeProbe();
        probe.AddDirectory(Left);
        probe.AddDirectory(Right);
        return probe;
    }

    [Fact]
    public void TwoOrdinaryFoldersAreAccepted() =>
        Assert.Null(CompareRefusal.Check(Left, Right, Probe()));

    [Fact]
    public void AnEmptySideIsRefused()
    {
        Assert.NotNull(CompareRefusal.Check("", Right, Probe()));
        Assert.NotNull(CompareRefusal.Check(Left, "   ", Probe()));
    }

    [Fact]
    public void APathThatIsNotAFolderIsRefused()
    {
        var probe = Probe();
        probe.AddFile(@"C:\Work\notes.txt");

        Assert.NotNull(CompareRefusal.Check(Left, @"C:\Work\notes.txt", probe));
    }

    /// <summary>
    /// A path inside an archive is a real Windows path as far as string handling is concerned, so
    /// it canonicalizes happily and lands inside the range scan for the container's own folder.
    /// Every other subtree feature in the app refuses it by name for the same reason.
    /// </summary>
    [Fact]
    public void AFolderInsideAnArchiveIsRefused()
    {
        var probe = Probe();
        probe.AddFile(@"C:\Work\bundle.zip");
        probe.AddDirectory(@"C:\Work\bundle.zip\src");

        Assert.NotNull(CompareRefusal.Check(Left, @"C:\Work\bundle.zip\src", probe));
        Assert.NotNull(CompareRefusal.Check(@"C:\Work\bundle.zip\src", Right, probe));
    }

    [Fact]
    public void AFolderComparedWithItselfIsRefused()
    {
        Assert.NotNull(CompareRefusal.Check(Left, Left, Probe()));
        Assert.NotNull(CompareRefusal.Check(Left, @"c:\work\LEFT", Probe()));
    }

    /// <summary>Every file under the inner folder would appear on both sides at two different
    /// relative paths, and the outer folder would be "only on the left" of itself.</summary>
    [Fact]
    public void AFolderComparedWithItsOwnSubtreeIsRefused()
    {
        var probe = Probe();
        probe.AddDirectory(@"C:\Work\left\inner");

        Assert.NotNull(CompareRefusal.Check(Left, @"C:\Work\left\inner", probe));
        Assert.NotNull(CompareRefusal.Check(@"C:\Work\left\inner", Left, probe));
    }

    /// <summary>
    /// The check neither path admits to. Both look like unrelated folders and only resolving the
    /// junction shows that one is inside the other — which is exactly why the transfer planner
    /// checks containment twice, and why this does too.
    /// </summary>
    [Fact]
    public void AJunctionThatPutsOneSideInsideTheOtherIsRefused()
    {
        var probe = Probe();
        probe.AddDirectory(@"C:\Work\mirror");
        probe.AddLink(@"C:\Work\mirror", @"C:\Work\left\inner");

        Assert.NotNull(CompareRefusal.Check(Left, @"C:\Work\mirror", probe));
    }

    private sealed class FakeProbe : ICompareProbe
    {
        private readonly HashSet<string> _dirs = new(StringComparer.OrdinalIgnoreCase);
        private readonly HashSet<string> _files = new(StringComparer.OrdinalIgnoreCase);
        private readonly Dictionary<string, string> _links = new(StringComparer.OrdinalIgnoreCase);

        public void AddDirectory(string path) => _dirs.Add(path);
        public void AddFile(string path) => _files.Add(path);
        public void AddLink(string link, string target) => _links[link] = target;

        public bool DirectoryExists(string path) => _dirs.Contains(path);
        public bool FileExists(string path) => _files.Contains(path);
        public string ResolveFinalPath(string path) =>
            _links.TryGetValue(path, out var target) ? target : path;
    }
}
