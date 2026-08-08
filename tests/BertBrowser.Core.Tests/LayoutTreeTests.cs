using BertBrowser.Core.Layout;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The pane layout algebra. These invariants are the reason the tree lives in Core at all: the
/// WPF host just renders whatever shape comes out, so anything wrong here shows up as a layout
/// that quietly degenerates after a few splits and closes rather than as an exception.
/// </summary>
public class LayoutTreeTests
{
    private static LayoutLeaf<string> Leaf(string name) => new(name);

    private static string[] Names(ILayoutNode<string> root) =>
        LayoutTree.Leaves(root).Select(l => l.Value).ToArray();

    // --- Split ---

    [Theory]
    [InlineData(SplitOrientation.Vertical)]
    [InlineData(SplitOrientation.Horizontal)]
    public void Split_LoneLeaf_BecomesSplitOfTwoEqualPanes(SplitOrientation orientation)
    {
        var a = Leaf("a");

        var root = LayoutTree.Split<string>(a, a, orientation, "b", out var b);

        var split = Assert.IsType<LayoutSplit<string>>(root);
        Assert.Equal(orientation, split.Orientation);
        Assert.Equal([a, b], split.Children);
        Assert.Equal(a.Weight, b.Weight);
        Assert.True(a.Weight > 0);
    }

    [Fact]
    public void Split_NewPaneGoesImmediatelyAfterTheOneItSplit()
    {
        var a = Leaf("a");
        var root = LayoutTree.Split<string>(a, a, SplitOrientation.Vertical, "b", out _);
        root = LayoutTree.Split(root, a, SplitOrientation.Vertical, "c", out _);

        Assert.Equal(["a", "c", "b"], Names(root));
    }

    [Fact]
    public void Split_SameOrientationAsParent_AddsSiblingInsteadOfNesting()
    {
        var a = Leaf("a");
        var root = LayoutTree.Split<string>(a, a, SplitOrientation.Vertical, "b", out var b);

        root = LayoutTree.Split(root, b, SplitOrientation.Vertical, "c", out _);

        var split = Assert.IsType<LayoutSplit<string>>(root);
        Assert.Equal(3, split.Children.Count);
        Assert.All(split.Children, child => Assert.IsType<LayoutLeaf<string>>(child));
    }

    [Fact]
    public void Split_DifferentOrientationFromParent_NestsAndLeavesSiblingCountAlone()
    {
        var a = Leaf("a");
        var root = LayoutTree.Split<string>(a, a, SplitOrientation.Vertical, "b", out var b);

        root = LayoutTree.Split(root, b, SplitOrientation.Horizontal, "c", out _);

        var split = Assert.IsType<LayoutSplit<string>>(root);
        Assert.Equal(2, split.Children.Count);
        var nested = Assert.IsType<LayoutSplit<string>>(split.Children[1]);
        Assert.Equal(SplitOrientation.Horizontal, nested.Orientation);
        Assert.Equal(["a", "b", "c"], Names(root));
    }

    [Fact]
    public void Split_TakesHalfOfTheTargetsSpaceAndLeavesSiblingsAlone()
    {
        var a = Leaf("a");
        var root = LayoutTree.Split<string>(a, a, SplitOrientation.Vertical, "b", out var b);
        a.Weight = 3;
        b.Weight = 1;

        LayoutTree.Split(root, a, SplitOrientation.Vertical, "c", out var c);

        Assert.Equal(1.5, a.Weight);
        Assert.Equal(1.5, c.Weight);
        Assert.Equal(1, b.Weight); // the pane that wasn't split must not move
    }

    // --- Close ---

    [Fact]
    public void Close_LastRemainingPane_IsRefused()
    {
        var a = Leaf("a");
        Assert.Null(LayoutTree.Close<string>(a, a));
    }

    [Fact]
    public void Close_CollapsesASplitLeftWithOneChild()
    {
        var a = Leaf("a");
        var root = LayoutTree.Split<string>(a, a, SplitOrientation.Vertical, "b", out var b);

        var closed = LayoutTree.Close(root, b);

        Assert.Same(a, closed);
    }

    [Fact]
    public void Close_GivesTheFreedSpaceToTheNeighbour()
    {
        var a = Leaf("a");
        var root = LayoutTree.Split<string>(a, a, SplitOrientation.Vertical, "b", out var b);
        root = LayoutTree.Split(root, b, SplitOrientation.Vertical, "c", out var c);
        a.Weight = 2;
        b.Weight = 1;
        c.Weight = 1;

        root = LayoutTree.Close(root, c)!;

        Assert.Equal(["a", "b"], Names(root));
        Assert.Equal(2, b.Weight); // c's share went to the pane beside it
        Assert.Equal(2, a.Weight); // and nowhere else
    }

    [Fact]
    public void Close_FlattensANestedSplitThatEndsUpMatchingItsParent()
    {
        // a | (b over (c | d)) — closing b leaves a same-orientation split inside a split.
        var a = Leaf("a");
        var root = LayoutTree.Split<string>(a, a, SplitOrientation.Vertical, "b", out var b);
        root = LayoutTree.Split(root, b, SplitOrientation.Horizontal, "c", out var c);
        root = LayoutTree.Split(root, c, SplitOrientation.Vertical, "d", out _);

        root = LayoutTree.Close(root, b)!;

        var split = Assert.IsType<LayoutSplit<string>>(root);
        Assert.Equal(SplitOrientation.Vertical, split.Orientation);
        Assert.Equal(["a", "c", "d"], Names(root));
        Assert.All(split.Children, child => Assert.IsType<LayoutLeaf<string>>(child));
    }

    // --- Traversal ---

    [Fact]
    public void Leaves_AreInDocumentOrder()
    {
        var a = Leaf("a");
        var root = LayoutTree.Split<string>(a, a, SplitOrientation.Vertical, "b", out var b);
        root = LayoutTree.Split(root, b, SplitOrientation.Horizontal, "c", out _);
        root = LayoutTree.Split(root, a, SplitOrientation.Vertical, "d", out _);

        Assert.Equal(["a", "d", "b", "c"], Names(root));
    }

    [Fact]
    public void NextLeaf_WrapsInBothDirections()
    {
        var a = Leaf("a");
        var root = LayoutTree.Split<string>(a, a, SplitOrientation.Vertical, "b", out var b);
        root = LayoutTree.Split(root, b, SplitOrientation.Vertical, "c", out var c);

        Assert.Same(b, LayoutTree.NextLeaf(root, a, 1));
        Assert.Same(a, LayoutTree.NextLeaf(root, c, 1));
        Assert.Same(c, LayoutTree.NextLeaf(root, a, -1));
    }

    [Fact]
    public void FindLeaf_LocatesByValue()
    {
        var a = Leaf("a");
        var root = LayoutTree.Split<string>(a, a, SplitOrientation.Vertical, "b", out var b);

        Assert.Same(b, LayoutTree.FindLeaf(root, "b"));
        Assert.Null(LayoutTree.FindLeaf(root, "nope"));
    }

    // --- Property test: random split/close sequences never degenerate ---

    [Theory]
    [InlineData(1)]
    [InlineData(2)]
    [InlineData(3)]
    [InlineData(4)]
    [InlineData(5)]
    public void RandomSplitsAndCloses_PreserveEveryInvariant(int seed)
    {
        var random = new Random(seed);
        var first = Leaf("p0");
        ILayoutNode<string> root = first;
        var next = 1;
        var expected = 1;

        for (var step = 0; step < 200; step++)
        {
            var leaves = LayoutTree.Leaves(root).ToList();
            var target = leaves[random.Next(leaves.Count)];

            if (leaves.Count > 1 && random.Next(3) == 0)
            {
                root = LayoutTree.Close(root, target)!;
                Assert.NotNull(root);
                expected--;
            }
            else
            {
                var orientation = random.Next(2) == 0
                    ? SplitOrientation.Vertical
                    : SplitOrientation.Horizontal;
                root = LayoutTree.Split(root, target, orientation, $"p{next++}", out _);
                expected++;
            }

            Assert.Equal(expected, LayoutTree.Leaves(root).Count());
            AssertWellFormed(root, parentOrientation: null);
            Assert.Equal(
                LayoutTree.Leaves(root).Select(l => l.Value).Distinct().Count(),
                LayoutTree.Leaves(root).Count());
        }
    }

    /// <summary>Every split holds at least two children, no split is nested directly inside a split
    /// running the same way, and no pane has a zero or negative share.</summary>
    private static void AssertWellFormed(ILayoutNode<string> node, SplitOrientation? parentOrientation)
    {
        Assert.True(node.Weight > 0, $"non-positive weight {node.Weight}");
        if (node is not LayoutSplit<string> split) return;

        Assert.True(split.Children.Count >= 2, $"split with {split.Children.Count} child(ren)");
        Assert.NotEqual(parentOrientation, split.Orientation);
        foreach (var child in split.Children)
            AssertWellFormed(child, split.Orientation);
    }
}
