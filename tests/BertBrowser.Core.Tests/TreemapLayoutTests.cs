using BertBrowser.Core.Services.DiskUsage;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The treemap geometry. Three properties make a treemap a treemap — the pieces fill the box, they
/// do not overlap, and their areas are proportional — and a fourth makes it readable, which is that
/// they are near-square rather than hairlines.
/// </summary>
public sealed class TreemapLayoutTests
{
    private const double Epsilon = 1e-6;

    private static IReadOnlyList<double> Weights(params double[] w) => w;

    // --- The three properties that make it a treemap ---

    [Theory]
    [InlineData(1)]
    [InlineData(7)]
    [InlineData(64)]
    public void EveryRectangleStaysInsideTheBounds(int count)
    {
        var weights = Enumerable.Range(1, count).Select(i => (double)(i * i)).ToList();

        foreach (var rect in TreemapLayout.Arrange(weights, 800, 500))
        {
            Assert.True(rect.X >= -Epsilon, $"{rect} starts left of the box");
            Assert.True(rect.Y >= -Epsilon, $"{rect} starts above the box");
            Assert.True(rect.Right <= 800 + Epsilon, $"{rect} runs past the right edge");
            Assert.True(rect.Bottom <= 500 + Epsilon, $"{rect} runs past the bottom edge");
        }
    }

    [Theory]
    [InlineData(1)]
    [InlineData(3)]
    [InlineData(17)]
    [InlineData(101)]
    public void RectanglesDoNotOverlap(int seed)
    {
        var random = new Random(seed);
        var weights = Enumerable.Range(0, 40).Select(_ => random.NextDouble() * 1000 + 1).ToList();

        var overlaps = FindOverlaps(TreemapLayout.Arrange(weights, 640, 400));

        Assert.True(overlaps is null, $"overlap: {overlaps}");
    }

    [Fact]
    public void AreasAreProportionalToWeights()
    {
        var weights = Weights(50, 25, 15, 10);
        var rects = TreemapLayout.Arrange(weights, 400, 300);
        var totalArea = 400d * 300d;

        foreach (var rect in rects)
        {
            var expected = totalArea * weights[rect.Index] / 100;
            Assert.Equal(expected, rect.Width * rect.Height, precision: 3);
        }
    }

    [Fact]
    public void TheWholeBoxIsCovered()
    {
        var covered = TreemapLayout.Arrange(Weights(5, 4, 3, 2, 1), 300, 200)
            .Sum(r => r.Width * r.Height);

        Assert.Equal(300d * 200d, covered, precision: 3);
    }

    // --- The property that makes it readable ---

    /// <summary>
    /// The reason for the algorithm. Slice-and-dice would satisfy every test above and still be
    /// useless: with a spread of sizes the small items become hairlines that cannot be labelled or
    /// clicked. Replace the squarifying with a single strip and this goes red.
    /// </summary>
    [Fact]
    public void SquarifiedBeatsSliceAndDice()
    {
        var weights = Weights(600, 300, 60, 20, 10, 6, 3, 1);

        var squarified = TreemapLayout.Arrange(weights, 600, 400).Max(r => r.AspectRatio);
        var sliced = SliceAndDice(weights, 600, 400).Max(r => r.AspectRatio);

        Assert.True(squarified < 6, $"worst aspect ratio was {squarified:F1}");
        Assert.True(squarified < sliced, $"squarified {squarified:F1} was no better than sliced {sliced:F1}");
    }

    // --- Degenerate input returns nothing rather than throwing ---

    [Theory]
    [InlineData(0, 100)]
    [InlineData(100, 0)]
    [InlineData(-5, 100)]
    [InlineData(double.NaN, 100)]
    [InlineData(double.PositiveInfinity, 100)]
    public void AnUnusableBoxYieldsNothing(double width, double height) =>
        Assert.Empty(TreemapLayout.Arrange(Weights(1, 2, 3), width, height));

    [Fact]
    public void NoWeightsYieldNothing() =>
        Assert.Empty(TreemapLayout.Arrange([], 100, 100));

    /// <summary>
    /// An item with no measured size has no area to claim, and giving it one would assert a share
    /// nobody established — the same rule the rest of this feature follows about unknown sizes.
    /// </summary>
    [Fact]
    public void ZeroAndNegativeAndNonFiniteWeightsGetNoRectangle()
    {
        var rects = TreemapLayout.Arrange(Weights(10, 0, -5, double.NaN, double.PositiveInfinity, 5), 200, 100);

        Assert.Equal([0, 5], rects.Select(r => r.Index).Order());
    }

    [Fact]
    public void ASingleItemFillsTheWholeBox()
    {
        var rect = Assert.Single(TreemapLayout.Arrange(Weights(42), 120, 80));

        Assert.Equal(0, rect.Index);
        Assert.Equal(120, rect.Width, precision: 6);
        Assert.Equal(80, rect.Height, precision: 6);
    }

    /// <summary>The layout sorts internally, so a caller has to be able to find its own item.</summary>
    [Fact]
    public void IndicesMapBackToTheOriginalOrder()
    {
        var rects = TreemapLayout.Arrange(Weights(1, 100, 10), 300, 200);

        // Largest first, by original position: 100 is at index 1, then 10 at 2, then 1 at 0.
        Assert.Equal([1, 2, 0], rects.Select(r => r.Index));
    }

    [Fact]
    public void EqualWeightsGetEqualAreas()
    {
        var rects = TreemapLayout.Arrange(Weights(1, 1, 1, 1), 200, 200);

        foreach (var rect in rects)
            Assert.Equal(10_000, rect.Width * rect.Height, precision: 3);
    }

    // --- Meta-test: the overlap check can actually fail ---

    /// <summary>
    /// A check that cannot fail proves nothing, so this hands the helper two rectangles that really
    /// do overlap and asserts it says so.
    /// </summary>
    [Fact]
    public void TheOverlapCheckCanFail()
    {
        var overlapping = new[]
        {
            new TreemapRect(0, 0, 0, 10, 10),
            new TreemapRect(1, 5, 5, 10, 10),
        };

        Assert.NotNull(FindOverlaps(overlapping));
    }

    private static string? FindOverlaps(IReadOnlyList<TreemapRect> rects)
    {
        for (var i = 0; i < rects.Count; i++)
        {
            for (var j = i + 1; j < rects.Count; j++)
            {
                var a = rects[i];
                var b = rects[j];

                var overlapWidth = Math.Min(a.Right, b.Right) - Math.Max(a.X, b.X);
                var overlapHeight = Math.Min(a.Bottom, b.Bottom) - Math.Max(a.Y, b.Y);

                if (overlapWidth > Epsilon && overlapHeight > Epsilon)
                    return $"{a} and {b}";
            }
        }
        return null;
    }

    /// <summary>The naive layout this algorithm exists to beat: one strip, every item across it.</summary>
    private static IReadOnlyList<TreemapRect> SliceAndDice(
        IReadOnlyList<double> weights, double width, double height)
    {
        var total = weights.Sum();
        var rects = new List<TreemapRect>();
        var x = 0.0;

        for (var i = 0; i < weights.Count; i++)
        {
            var w = width * weights[i] / total;
            rects.Add(new TreemapRect(i, x, 0, w, height));
            x += w;
        }
        return rects;
    }
}
