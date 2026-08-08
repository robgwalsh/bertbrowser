namespace BertBrowser.Core.Layout;

/// <summary>How a split arranges its children.</summary>
public enum SplitOrientation
{
    /// <summary>Children sit side by side, separated by vertical splitter bars.</summary>
    Vertical,

    /// <summary>Children are stacked, separated by horizontal splitter bars.</summary>
    Horizontal,
}

/// <summary>A node in a pane layout: either a pane (<see cref="LayoutLeaf{T}"/>) or a division of
/// space between several nodes (<see cref="LayoutSplit{T}"/>).</summary>
/// <remarks>
/// Weights are mutable and deliberately un-observable: a splitter drag writes the measured star
/// values straight back, and the one thing that has to react to a <em>structural</em> change (the
/// view host) is told by a single event on the owner rather than by watching the tree.
/// </remarks>
public interface ILayoutNode<T>
{
    /// <summary>This node's share of its parent's space, as a star weight. Always &gt; 0.</summary>
    double Weight { get; set; }
}

public sealed class LayoutLeaf<T>(T value) : ILayoutNode<T>
{
    public T Value { get; } = value;
    public double Weight { get; set; } = 1;
}

public sealed class LayoutSplit<T> : ILayoutNode<T>
{
    public LayoutSplit(SplitOrientation orientation, IEnumerable<ILayoutNode<T>> children)
    {
        Orientation = orientation;
        Children = children.ToList();
    }

    public SplitOrientation Orientation { get; }
    public List<ILayoutNode<T>> Children { get; }
    public double Weight { get; set; } = 1;
}

/// <summary>
/// The pane layout algebra: split a pane in two, close one, and enumerate what is left. Pure and
/// UI-free so the invariants that keep the layout sane — no split with fewer than two children, no
/// split nested directly inside a split of the same orientation, at least one pane always open —
/// can be tested rather than eyeballed.
/// </summary>
public static class LayoutTree
{
    /// <summary>Splits <paramref name="target"/>, putting a new pane beside (or below) it and
    /// returning the new root. The new pane takes half of the target's space, so the rest of the
    /// layout does not move.</summary>
    /// <remarks>Splitting inside a parent that already runs the same way appends a sibling rather
    /// than nesting, which is what keeps three side-by-side panes one flat row of splitters instead
    /// of a lopsided tree.</remarks>
    public static ILayoutNode<T> Split<T>(
        ILayoutNode<T> root, LayoutLeaf<T> target, SplitOrientation orientation, T newValue,
        out LayoutLeaf<T> inserted)
    {
        ArgumentNullException.ThrowIfNull(root);
        ArgumentNullException.ThrowIfNull(target);

        inserted = new LayoutLeaf<T>(newValue);

        var parent = FindParent(root, target);
        var half = target.Weight / 2;
        target.Weight = half;
        inserted.Weight = half;

        if (parent is not null && parent.Orientation == orientation)
        {
            parent.Children.Insert(parent.Children.IndexOf(target) + 1, inserted);
            return root;
        }

        var split = new LayoutSplit<T>(orientation, [target, inserted]) { Weight = half * 2 };
        // The new split stands exactly where the target did, so it inherits the target's share.
        target.Weight = 1;
        inserted.Weight = 1;

        if (parent is null) return split;
        parent.Children[parent.Children.IndexOf(target)] = split;
        return root;
    }

    /// <summary>Removes <paramref name="target"/> and returns the new root, or null when it is the
    /// last pane (closing the only pane is refused — the window always shows something).</summary>
    public static ILayoutNode<T>? Close<T>(ILayoutNode<T> root, LayoutLeaf<T> target)
    {
        ArgumentNullException.ThrowIfNull(root);
        ArgumentNullException.ThrowIfNull(target);

        if (ReferenceEquals(root, target)) return null;

        var parent = FindParent(root, target);
        if (parent is null) return root; // not in this tree

        var index = parent.Children.IndexOf(target);
        parent.Children.RemoveAt(index);

        // The freed space goes to whichever neighbour the closed pane was touching.
        var neighbour = parent.Children[Math.Min(index, parent.Children.Count - 1)];
        neighbour.Weight += target.Weight;

        if (parent.Children.Count > 1) return Normalize(root);

        // A split with one child is not a split any more: hoist that child into its place.
        var survivor = parent.Children[0];
        survivor.Weight = parent.Weight;

        var grandparent = FindParent(root, parent);
        if (grandparent is null) return Normalize(survivor);

        grandparent.Children[grandparent.Children.IndexOf(parent)] = survivor;
        return Normalize(root);
    }

    /// <summary>Every pane, in the order they appear on screen (left to right, top to bottom).
    /// This order is what "focus the next pane" walks.</summary>
    public static IEnumerable<LayoutLeaf<T>> Leaves<T>(ILayoutNode<T> root)
    {
        switch (root)
        {
            case LayoutLeaf<T> leaf:
                yield return leaf;
                break;
            case LayoutSplit<T> split:
                foreach (var child in split.Children)
                {
                    foreach (var leaf in Leaves(child))
                        yield return leaf;
                }
                break;
        }
    }

    public static LayoutLeaf<T>? FindLeaf<T>(ILayoutNode<T> root, T value) =>
        Leaves(root).FirstOrDefault(l => EqualityComparer<T>.Default.Equals(l.Value, value));

    /// <summary>The pane <paramref name="step"/> positions after <paramref name="from"/> in
    /// document order, wrapping around. <paramref name="step"/> may be negative.</summary>
    public static LayoutLeaf<T> NextLeaf<T>(ILayoutNode<T> root, LayoutLeaf<T> from, int step)
    {
        var leaves = Leaves(root).ToList();
        if (leaves.Count == 0) return from;

        var index = leaves.IndexOf(from);
        if (index < 0) return leaves[0];

        var next = ((index + step) % leaves.Count + leaves.Count) % leaves.Count;
        return leaves[next];
    }

    public static LayoutSplit<T>? FindParent<T>(ILayoutNode<T> root, ILayoutNode<T> child)
    {
        if (root is not LayoutSplit<T> split) return null;
        if (split.Children.Contains(child)) return split;

        foreach (var candidate in split.Children)
        {
            if (FindParent(candidate, child) is { } found) return found;
        }
        return null;
    }

    /// <summary>Folds a split into its parent when the two run the same way, which can happen once
    /// a close leaves a nested split with a single sibling. Without it the tree would keep
    /// pointless levels that make the splitters behave inconsistently.</summary>
    private static ILayoutNode<T> Normalize<T>(ILayoutNode<T> root)
    {
        if (root is not LayoutSplit<T> split) return root;

        for (var i = 0; i < split.Children.Count; i++)
        {
            var child = Normalize(split.Children[i]);
            split.Children[i] = child;

            if (child is not LayoutSplit<T> nested || nested.Orientation != split.Orientation)
                continue;

            // Redistribute the nested split's share across the children being hoisted.
            var total = nested.Children.Sum(c => c.Weight);
            if (total <= 0) total = nested.Children.Count;
            foreach (var grandchild in nested.Children)
                grandchild.Weight = nested.Weight * grandchild.Weight / total;

            split.Children.RemoveAt(i);
            split.Children.InsertRange(i, nested.Children);
            i += nested.Children.Count - 1;
        }

        return split.Children.Count == 1 ? Hoist(split) : split;
    }

    private static ILayoutNode<T> Hoist<T>(LayoutSplit<T> split)
    {
        var survivor = split.Children[0];
        survivor.Weight = split.Weight;
        return survivor;
    }
}
