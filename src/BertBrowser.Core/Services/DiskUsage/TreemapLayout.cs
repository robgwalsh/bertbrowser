namespace BertBrowser.Core.Services.DiskUsage;

/// <summary>A rectangle in layout space, and which input it belongs to.</summary>
/// <param name="Index">Position in the list handed to <see cref="TreemapLayout.Arrange"/>. The
/// layout sorts internally, so this is how a caller finds its own item again.</param>
public readonly record struct TreemapRect(int Index, double X, double Y, double Width, double Height)
{
    public double Right => X + Width;
    public double Bottom => Y + Height;

    /// <summary>How far from square. 1 is a square; larger is worse, in either direction.</summary>
    public double AspectRatio =>
        Width <= 0 || Height <= 0 ? double.PositiveInfinity : Math.Max(Width / Height, Height / Width);
}

/// <summary>
/// Squarified treemap packing: divides a rectangle into areas proportional to a set of weights,
/// keeping each piece as close to square as it can.
/// </summary>
/// <remarks>
/// <para>
/// The naive alternative — slice-and-dice, laying every item across in one direction — produces
/// areas that are just as proportional and completely unreadable: with any spread of sizes the
/// small items become hairlines a pixel wide that cannot be labelled, clicked, or compared.
/// Squarifying is what makes the picture worth drawing, so <c>TreemapLayoutTests</c> holds it to a
/// worst-aspect-ratio bound that a strip layout fails.
/// </para>
/// <para>
/// Pure and UI-free, in Core, for the reason the rest of this folder is: the geometry is the part
/// worth testing, and it can be tested without a window.
/// </para>
/// </remarks>
public static class TreemapLayout
{
    /// <summary>
    /// Packs <paramref name="weights"/> into a <paramref name="width"/> × <paramref name="height"/>
    /// rectangle at the origin, largest first.
    /// </summary>
    /// <returns>
    /// One rectangle per positive weight, in descending weight order. Weights that are zero,
    /// negative or not finite get no rectangle at all — an item with no measured size has no area
    /// to claim, and drawing it one would assert a share nobody established.
    /// </returns>
    public static IReadOnlyList<TreemapRect> Arrange(
        IReadOnlyList<double> weights, double width, double height)
    {
        ArgumentNullException.ThrowIfNull(weights);

        if (weights.Count == 0 || !IsUsable(width) || !IsUsable(height)) return [];

        var items = new List<(int Index, double Weight)>(weights.Count);
        for (var i = 0; i < weights.Count; i++)
        {
            if (double.IsFinite(weights[i]) && weights[i] > 0)
                items.Add((i, weights[i]));
        }
        if (items.Count == 0) return [];

        items.Sort((a, b) => b.Weight.CompareTo(a.Weight));

        var total = items.Sum(i => i.Weight);
        var scale = width * height / total;

        var result = new List<TreemapRect>(items.Count);
        Squarify(items, scale, new Frame(0, 0, width, height), result);
        return result;
    }

    private static bool IsUsable(double value) => double.IsFinite(value) && value > 0;

    /// <summary>The rectangle still to be filled.</summary>
    private readonly record struct Frame(double X, double Y, double Width, double Height)
    {
        /// <summary>Rows are laid along the shorter side, which is what keeps them near-square.</summary>
        public double Shorter => Math.Min(Width, Height);
        public bool Horizontal => Width >= Height;
    }

    /// <remarks>
    /// Iterative rather than recursive: a folder can hold tens of thousands of children, and this
    /// would otherwise recurse once per row.
    /// </remarks>
    private static void Squarify(
        List<(int Index, double Weight)> items, double scale, Frame frame, List<TreemapRect> output)
    {
        var next = 0;
        while (next < items.Count && IsUsable(frame.Width) && IsUsable(frame.Height))
        {
            // Grow a row one item at a time for as long as doing so makes its worst rectangle
            // squarer. The moment adding another would make it worse, the row is finished.
            var row = new List<(int Index, double Weight)>();
            var rowArea = 0.0;
            var worst = double.PositiveInfinity;

            while (next < items.Count)
            {
                var area = items[next].Weight * scale;
                var candidate = WorstAspect(rowArea + area, area, MinArea(row, area), frame.Shorter);

                if (row.Count > 0 && candidate > worst) break;

                row.Add(items[next]);
                rowArea += area;
                worst = candidate;
                next++;
            }

            frame = PlaceRow(row, rowArea, frame, output);
        }
    }

    private static double MinArea(List<(int Index, double Weight)> row, double candidateArea) =>
        row.Count == 0 ? candidateArea : Math.Min(candidateArea, row[^1].Weight);

    /// <summary>
    /// The worst aspect ratio a row would have. The row's thickness is its area divided by the side
    /// it runs along, so the extremes come from its largest and smallest members.
    /// </summary>
    private static double WorstAspect(double rowArea, double largest, double smallest, double side)
    {
        if (rowArea <= 0 || side <= 0) return double.PositiveInfinity;

        var thickness = rowArea / side;
        if (thickness <= 0) return double.PositiveInfinity;

        var widest = largest / thickness;
        var narrowest = smallest / thickness;
        if (widest <= 0 || narrowest <= 0) return double.PositiveInfinity;

        return Math.Max(
            Math.Max(thickness / widest, widest / thickness),
            Math.Max(thickness / narrowest, narrowest / thickness));
    }

    /// <summary>Lays one finished row along the frame's shorter side and returns what is left.</summary>
    private static Frame PlaceRow(
        List<(int Index, double Weight)> row, double rowArea, Frame frame, List<TreemapRect> output)
    {
        if (row.Count == 0) return frame with { Width = 0, Height = 0 };

        var side = frame.Shorter;
        var thickness = Math.Min(rowArea / side, frame.Horizontal ? frame.Width : frame.Height);

        var offset = 0.0;
        var rowTotal = row.Sum(r => r.Weight);

        for (var i = 0; i < row.Count; i++)
        {
            // The last piece takes whatever is left, so rounding never leaves a seam.
            var length = i == row.Count - 1
                ? side - offset
                : side * (row[i].Weight / rowTotal);

            output.Add(frame.Horizontal
                ? new TreemapRect(row[i].Index, frame.X, frame.Y + offset, thickness, length)
                : new TreemapRect(row[i].Index, frame.X + offset, frame.Y, length, thickness));

            offset += length;
        }

        return frame.Horizontal
            ? frame with { X = frame.X + thickness, Width = frame.Width - thickness }
            : frame with { Y = frame.Y + thickness, Height = frame.Height - thickness };
    }
}
