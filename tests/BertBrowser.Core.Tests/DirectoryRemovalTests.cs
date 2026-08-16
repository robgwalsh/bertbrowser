using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Erasing a tree, on real files. The reason this exists at all is a junction: swap
/// <see cref="DirectoryRemoval.RemoveTree"/> for <c>Directory.Delete(recursive: true)</c> and every
/// junction test below goes red — the first because that call throws, and the rest because of what
/// it has already destroyed by the time it does.
/// </summary>
public sealed class DirectoryRemovalTests : IDisposable
{
    private readonly string _root;

    public DirectoryRemovalTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-rm-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        try
        {
            DirectoryRemoval.RemoveTree(_root);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
        }
    }

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

    private string P(params string[] parts) => Path.Combine([_root, .. parts]);

    [Fact]
    public void AnOrdinaryTreeGoesEntirely()
    {
        var tree = Dir("tree");
        File_("a", "tree", "a.txt");
        File_("b", "tree", "one", "b.txt");
        File_("c", "tree", "one", "two", "three", "c.txt");

        DirectoryRemoval.RemoveTree(tree);

        Assert.False(Directory.Exists(tree));
    }

    [Fact]
    public void ATreeContainingAJunctionGoesEntirely_AndTheTargetSurvives()
    {
        var outside = Dir("outside");
        var bystander = File_("keep me", "outside", "bystander.txt");

        var tree = Dir("tree");
        File_("a", "tree", "a.txt");
        File_("b", "tree", "deep", "b.txt");
        Assert.True(TryCreateJunction(P("tree", "deep", "link"), outside), "junction was not created");

        DirectoryRemoval.RemoveTree(tree);

        Assert.False(Directory.Exists(tree), "the tree survived a removal that reported no error");
        Assert.True(Directory.Exists(outside), "the junction was followed and its target removed");
        Assert.Equal("keep me", File.ReadAllText(bystander));
    }

    [Fact]
    public void AJunctionGivenDirectlyIsRemovedAsOneEntry()
    {
        var outside = Dir("outside");
        var bystander = File_("keep me", "outside", "bystander.txt");
        var link = P("link");
        Assert.True(TryCreateJunction(link, outside), "junction was not created");

        DirectoryRemoval.RemoveTree(link);

        Assert.False(Directory.Exists(link));
        Assert.Equal("keep me", File.ReadAllText(bystander));
    }

    /// <summary>
    /// Meta-test: proves the junctions above are real and traversable, so the assertions are passing
    /// because the link was declined rather than because it never worked.
    /// </summary>
    [Fact]
    public void MetaAJunctionIsRealAndLeadsSomewhere()
    {
        var outside = Dir("outside");
        File_("keep me", "outside", "bystander.txt");
        Dir("tree");
        Assert.True(TryCreateJunction(P("tree", "link"), outside), "junction was not created");

        Assert.True(DirectoryRemoval.IsLink(new DirectoryInfo(P("tree", "link"))));
        Assert.Contains(
            "bystander.txt",
            Directory.EnumerateFiles(P("tree"), "*", SearchOption.AllDirectories)
                .Select(Path.GetFileName));
    }

    [Fact]
    public void AMissingTreeThrowsRatherThanPretendingToHaveWorked()
    {
        Assert.ThrowsAny<IOException>(() => DirectoryRemoval.RemoveTree(P("never-existed")));
    }

    /// <summary>
    /// A directory junction, which — unlike a symlink — an ordinary account may create. Shelling out
    /// because .NET exposes no API for one; <c>IndexCrawlerTests</c> does the same.
    /// </summary>
    private static bool TryCreateJunction(string link, string target)
    {
        var psi = new System.Diagnostics.ProcessStartInfo("cmd.exe",
            $"/c mklink /J \"{link}\" \"{target}\"")
        {
            CreateNoWindow = true,
            UseShellExecute = false,
        };
        using (var p = System.Diagnostics.Process.Start(psi)!)
            p.WaitForExit();

        return Directory.Exists(link) &&
            (File.GetAttributes(link) & FileAttributes.ReparsePoint) != 0;
    }
}
