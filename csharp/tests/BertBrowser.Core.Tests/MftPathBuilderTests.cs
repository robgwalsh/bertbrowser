using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class MftPathBuilderTests
{
    private const ulong Root = 0x0005000000000005UL;

    private static Dictionary<ulong, MftNode> Tree() => new()
    {
        [10] = new MftNode("Users", Root, IsDirectory: true, Attributes: 0),
        [11] = new MftNode("Rob", 10, IsDirectory: true, Attributes: 0),
        [12] = new MftNode("report.txt", 11, IsDirectory: false, Attributes: 0),
    };

    [Fact]
    public void Resolve_BuildsFullPathFromParentChain()
    {
        var cache = new Dictionary<ulong, (string, bool)>();

        Assert.True(MftPathBuilder.TryResolve(Tree(), 12, @"C:\", cache, out var path, out var hidden));

        Assert.Equal(@"C:\Users\Rob\report.txt", path);
        Assert.False(hidden);
    }

    [Fact]
    public void Resolve_DirectChildOfRoot()
    {
        var map = new Dictionary<ulong, MftNode>
        {
            [20] = new MftNode("pagefile.sys", Root, IsDirectory: false, Attributes: FileAttributes.Hidden),
        };

        Assert.True(MftPathBuilder.TryResolve(map, 20, @"C:\", new(), out var path, out var hidden));

        Assert.Equal(@"C:\pagefile.sys", path);
        Assert.True(hidden);
    }

    [Fact]
    public void Resolve_InheritsHiddenFromAncestor()
    {
        var map = Tree();
        map[11] = map[11] with { Attributes = FileAttributes.Hidden }; // hide "Rob"

        Assert.True(MftPathBuilder.TryResolve(map, 12, @"C:\", new(), out _, out var hidden));

        Assert.True(hidden); // file itself isn't hidden, but its ancestor is
    }

    [Fact]
    public void Resolve_BrokenChainFails()
    {
        var map = new Dictionary<ulong, MftNode>
        {
            [12] = new MftNode("orphan.txt", 999 /* missing parent */, IsDirectory: false, Attributes: 0),
        };

        Assert.False(MftPathBuilder.TryResolve(map, 12, @"C:\", new(), out _, out _));
    }

    [Fact]
    public void Resolve_PopulatesDirectoryCacheForAncestors()
    {
        var cache = new Dictionary<ulong, (string Path, bool Hidden)>();

        MftPathBuilder.TryResolve(Tree(), 12, @"C:\", cache, out _, out _);

        Assert.Equal(@"C:\Users", cache[10].Path);
        Assert.Equal(@"C:\Users\Rob", cache[11].Path);
        Assert.False(cache.ContainsKey(12)); // the file itself is not a directory
    }
}
