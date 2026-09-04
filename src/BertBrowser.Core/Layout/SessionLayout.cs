using System.Text.Json.Serialization;
using BertBrowser.Core.Services.Archives;

namespace BertBrowser.Core.Layout;

/// <summary>
/// A saved pane arrangement, in a shape that survives being written to settings.json and read back.
/// </summary>
/// <remarks>
/// <para>
/// The mirror of <see cref="ILayoutNode{T}"/> rather than the thing itself: that tree is generic
/// over live view models and its leaves hold objects with running work in them, none of which can
/// be serialised. This carries only what a session needs to be rebuilt — where the splits are, how
/// the space was divided, and what each pane had open.
/// </para>
/// <para>
/// A node is a split or a pane, never both, which is why <see cref="Children"/> and
/// <see cref="Tabs"/> are nullable and exactly one of them is set. That is looser than the live
/// tree's two types, and deliberately: hand-edited or half-written JSON has to be survivable, so
/// <see cref="IsUsable"/> decides what is a layout rather than the deserialiser throwing.
/// </para>
/// </remarks>
public sealed class SessionLayout
{
    /// <summary>Null for a pane; set for a split.</summary>
    public SplitOrientation? Orientation { get; set; }

    /// <summary>This node's share of its parent, as a star weight.</summary>
    public double Weight { get; set; } = 1;

    /// <summary>A split's children, in on-screen order. Null for a pane.</summary>
    public List<SessionLayout>? Children { get; set; }

    /// <summary>A pane's open directories, in tab order. Null for a split.</summary>
    public List<SessionTab>? Tabs { get; set; }

    /// <summary>Which of <see cref="Tabs"/> was showing.</summary>
    public int ActiveTabIndex { get; set; }

    /// <summary>True when this node was the pane the window chrome was following.</summary>
    public bool IsActivePane { get; set; }

    [JsonIgnore]
    public bool IsSplit => Children is { Count: > 0 };
}

/// <summary>One open directory, and how it was being shown.</summary>
public sealed class SessionTab
{
    public string Path { get; set; } = "";

    /// <summary>The sort column's name. A string rather than the enum so an unknown value from a
    /// newer or hand-edited file falls back instead of failing the whole layout.</summary>
    public string? SortBy { get; set; }

    public bool SortDescending { get; set; }

    /// <summary>The columns this tab was showing, when they were arranged here rather than taken
    /// from the saved default. <b>Null means "whatever the default is"</b> and is what an untouched
    /// tab saves — so changing the default still reaches it on the next launch.
    /// <see cref="Services.Columns.ColumnLayoutRules.Normalize"/> cleans it up on the way back in,
    /// so an unusable entry degrades rather than failing the layout, exactly as
    /// <see cref="SortBy"/> does.</summary>
    public List<Services.Columns.ColumnSetting>? Columns { get; set; }
}

/// <summary>
/// Turning a saved layout back into something safe to rebuild from.
/// </summary>
/// <remarks>
/// Everything here is defensive, because every input is either from a previous version of this app
/// or from a person editing settings.json. A layout that cannot be honoured must degrade to the
/// ordinary single-pane start rather than throw — a session that will not open is far worse than
/// one that opens somewhere unexpected.
/// </remarks>
public static class SessionLayoutRules
{
    /// <summary>More panes than anyone arranges deliberately; a guard against a file that claims
    /// thousands and would spend the whole startup building them.</summary>
    public const int MaxPanes = 32;

    /// <summary>Tabs per pane, same reasoning.</summary>
    public const int MaxTabsPerPane = 64;

    /// <summary>
    /// Drops what no longer exists and returns a layout worth rebuilding, or null when nothing is
    /// left.
    /// </summary>
    /// <param name="node">The saved node.</param>
    /// <param name="exists">Whether a path is still a directory — injected so this stays pure.</param>
    /// <remarks>
    /// A pane whose every tab has gone (an unplugged drive, a deleted project) is dropped, and a
    /// split left with one child collapses into it — the same rules
    /// <see cref="LayoutTree.Close{T}"/> enforces on the live tree, applied here so a restored
    /// arrangement cannot start out in a state the live tree forbids.
    /// </remarks>
    public static SessionLayout? Prune(SessionLayout? node, Func<string, bool> exists)
    {
        ArgumentNullException.ThrowIfNull(exists);
        if (node is null) return null;

        if (!node.IsSplit)
        {
            var tabs = (node.Tabs ?? [])
                .Where(t => t.Path is { Length: > 0 } && exists(t.Path))
                .Take(MaxTabsPerPane)
                .ToList();

            if (tabs.Count == 0) return null;

            node.Tabs = tabs;
            node.ActiveTabIndex = Math.Clamp(node.ActiveTabIndex, 0, tabs.Count - 1);
            node.Children = null;
            node.Weight = SaneWeight(node.Weight);
            return node;
        }

        var kept = new List<SessionLayout>();
        foreach (var child in node.Children!)
        {
            if (Prune(child, exists) is { } survivor) kept.Add(survivor);
        }

        if (kept.Count == 0) return null;

        // A split of one is not a split. Hoisting rather than keeping an empty level is what stops
        // a restored layout carrying splitters with nothing on one side.
        if (kept.Count == 1)
        {
            var only = kept[0];
            only.Weight = SaneWeight(node.Weight);
            return only;
        }

        node.Children = kept;
        node.Tabs = null;
        node.Weight = SaneWeight(node.Weight);
        return node;
    }

    /// <summary>Whether a pruned layout is worth restoring at all.</summary>
    public static bool IsUsable(SessionLayout? node) =>
        node is not null && CountPanes(node) is > 0 and <= MaxPanes;

    public static int CountPanes(SessionLayout node)
    {
        ArgumentNullException.ThrowIfNull(node);
        if (!node.IsSplit) return 1;

        var total = 0;
        foreach (var child in node.Children!)
            total += CountPanes(child);
        return total;
    }

    /// <summary>Every pane, in on-screen order — the same order
    /// <see cref="LayoutTree.Leaves{T}"/> walks.</summary>
    public static IEnumerable<SessionLayout> Panes(SessionLayout node)
    {
        ArgumentNullException.ThrowIfNull(node);
        if (!node.IsSplit)
        {
            yield return node;
            yield break;
        }

        foreach (var child in node.Children!)
        {
            foreach (var pane in Panes(child))
                yield return pane;
        }
    }

    /// <summary>A weight of zero, NaN or infinity would give a Grid a column it can never lay out,
    /// so anything unusable becomes an even share.</summary>
    private static double SaneWeight(double weight) =>
        double.IsFinite(weight) && weight > 0 ? weight : 1;

    /// <summary>Whether a saved tab's path is still somewhere worth reopening — a real directory,
    /// or somewhere inside an archive. The one definition <see cref="Prune"/>'s callers share, so
    /// restoring at launch and restoring a named workspace can't drift apart on what "still
    /// exists" means.</summary>
    public static bool PathExists(string path) =>
        Directory.Exists(path) || ArchivePath.Parse(path, File.Exists) is not null;
}
