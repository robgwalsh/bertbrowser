namespace BertBrowser.Core.Services.Columns;

/// <summary>
/// Turns a saved column list into the columns a list actually shows, and edits that saved list.
/// Pure — it is the whole of the thinking behind configurable columns, held where xUnit can reach it.
/// </summary>
/// <remarks>
/// <para>
/// <see cref="Resolve"/> replaces the two <c>Width = 0</c> hacks the Folder and Match columns used to
/// live behind. Those columns follow the list's <em>mode</em> rather than anyone's choice, which is
/// what an assigned zero width was saying all along; saying it here instead means a hidden column is
/// genuinely absent rather than a clickable zero-width header with a gripper in the tab order.
/// </para>
/// <para>
/// Every editing operation returns a new list rather than mutating one, and the header menu, the
/// reorder drag and the settings page all go through these functions — so the three cannot disagree
/// about what "move this column" means.
/// </para>
/// </remarks>
public static class ColumnLayoutRules
{
    /// <summary>A row realizes one cell per column, so this bounds what a hand-edited or corrupt
    /// settings file can cost. <c>SessionLayoutRules.MaxTabsPerPane</c> reasons the same way.</summary>
    public const int MaxColumns = 24;

    public const double MinWidth = 40;
    public const double MaxWidth = 2000;

    /// <summary>
    /// A width fit to hand to WPF.
    /// </summary>
    /// <remarks>
    /// <c>NaN</c> is the case that matters and it is not hypothetical: double-clicking a column
    /// gripper auto-sizes it, which sets <see cref="double.NaN"/>. Infinity and absurd numbers come
    /// from hand-edited files and from widths saved on a much wider monitor. Same input and same
    /// answer as <c>SessionLayoutRules.SaneWeight</c>: degrade to something usable, never fail.
    /// </remarks>
    public static double SaneWidth(double width, double fallback)
    {
        if (double.IsFinite(width) && width > 0) return Math.Clamp(width, MinWidth, MaxWidth);
        var safe = double.IsFinite(fallback) && fallback > 0 ? fallback : 120;
        return Math.Clamp(safe, MinWidth, MaxWidth);
    }

    /// <summary>How far one notch of the wheel moves a width, and the grid a coarse step snaps to.</summary>
    public const double WidthStep = 10;

    /// <summary>
    /// A width nudged by <paramref name="notches"/> turns of the wheel.
    /// </summary>
    /// <remarks>
    /// A coarse step <b>snaps onto the grid</b> rather than adding to whatever arbitrary number a
    /// dragged gripper left behind: from 137, up is 140. Otherwise spinning the wheel would carry
    /// that 7 along forever, and the sequence a person watches go by would never look tidy. Fine
    /// steps do not snap, because their whole purpose is reaching the number between the grid lines.
    /// Everything lands through <see cref="SaneWidth"/>, so a <c>NaN</c> from an auto-sized column
    /// is repaired here rather than becoming <c>NaN + 10</c>.
    /// </remarks>
    public static double StepWidth(double width, int notches, bool fine)
    {
        var from = SaneWidth(width, 120);
        if (notches == 0) return from;

        if (fine) return SaneWidth(from + notches, from);

        // Snap first, then step, so the notch is never spent only on the snap: 137 up is 140 and
        // 137 down is 130, both one visible move.
        var grid = notches > 0
            ? Math.Floor(from / WidthStep) * WidthStep
            : Math.Ceiling(from / WidthStep) * WidthStep;
        return SaneWidth(grid + (notches * WidthStep), from);
    }

    /// <summary>
    /// The columns to show: the saved list, cleaned up, with the mode-driven ones put in place.
    /// </summary>
    /// <param name="user">
    /// What was saved. <b>Null means never configured</b> and ships
    /// <see cref="ColumnCatalog.Defaults"/>; an <b>empty list is honoured as empty</b> — unlike
    /// <c>NewFileTypes</c>, because the Name rule in <see cref="Normalize"/> means an empty layout is
    /// still a usable one, and one rule beats two.
    /// </param>
    public static IReadOnlyList<ResolvedColumn> Resolve(
        IReadOnlyList<ColumnSetting>? user, bool isFlattened, bool showsContentMatches)
    {
        var settings = Normalize(user);
        var resolved = new List<ResolvedColumn>(settings.Count + 2);

        foreach (var setting in settings)
        {
            var spec = ColumnCatalog.TryGet(setting.Id)!; // Normalize dropped anything without one
            resolved.Add(new ResolvedColumn(spec, SaneWidth(setting.Width, spec.DefaultWidth)));
        }

        // Folder sits between the name and everything else, where the old XAML put it, and only in a
        // flattened result: in a directory listing every row has the same folder, the one you are in.
        var at = 1;
        if (isFlattened)
            resolved.Insert(Math.Min(at++, resolved.Count), Injected(ColumnCatalog.RelativePath));

        // And Match only when the search actually read file contents — keyed on that and never on
        // IsFlattened, which every search sets. An empty column appearing whenever you type into the
        // box would read as a rendering fault.
        if (showsContentMatches)
            resolved.Insert(Math.Min(at, resolved.Count), Injected(ColumnCatalog.Match));

        return resolved;

        static ResolvedColumn Injected(string id)
        {
            var spec = ColumnCatalog.TryGet(id)!;
            return new ResolvedColumn(spec, spec.DefaultWidth, Injected: true);
        }
    }

    /// <summary>
    /// The saved list, made usable: unrenderable ids dropped, duplicates collapsed, injected ids
    /// removed, Name first, and no more than <see cref="MaxColumns"/> of them.
    /// </summary>
    public static IReadOnlyList<ColumnSetting> Normalize(IReadOnlyList<ColumnSetting>? user)
    {
        var source = user ?? ColumnCatalog.Defaults();
        var kept = new List<ColumnSetting>(Math.Min(source.Count, MaxColumns));
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        foreach (var setting in source)
        {
            var id = setting.Id?.Trim() ?? "";

            // Folder and Match are placed by Resolve, never chosen. One arriving from a hand-edited
            // file would otherwise appear twice, or appear in a listing where it means nothing.
            if (id.Length == 0 || ColumnCatalog.IsInjected(id)) continue;
            if (ColumnCatalog.TryGet(id) is not { } spec) continue;
            if (!seen.Add(spec.Id)) continue;

            kept.Add(new ColumnSetting(spec.Id, SaneWidth(setting.Width, spec.DefaultWidth)));
            if (kept.Count == MaxColumns) break;
        }

        // Name carries the icon and is what a row is identified by; a list without it, or one with it
        // scrolled off the left of a horizontally scrolling view, is unusable. Explorer does not let
        // you move Name either. Enforced here rather than only on load, because the reorder drag comes
        // back through Normalize too: GridView.AllowsColumnReorder has always let Name be dragged
        // away, and persisting the layout is what would have made that stick.
        var index = IndexOf(kept, ColumnCatalog.Name);
        if (index > 0)
        {
            var name = kept[index];
            kept.RemoveAt(index);
            kept.Insert(0, name);
        }
        else if (index < 0)
        {
            var spec = ColumnCatalog.TryGet(ColumnCatalog.Name)!;
            if (kept.Count == MaxColumns) kept.RemoveAt(kept.Count - 1);
            kept.Insert(0, new ColumnSetting(spec.Id, spec.DefaultWidth));
        }

        return kept;
    }

    /// <summary>Adds a column at the end, or removes it. Name cannot be removed.</summary>
    public static IReadOnlyList<ColumnSetting> Toggle(IReadOnlyList<ColumnSetting>? user, string id, bool on)
    {
        var kept = Copy(Normalize(user));
        var index = IndexOf(kept, id);

        if (!on)
        {
            // Index 0 is Name, which Normalize would only put straight back.
            if (index > 0) kept.RemoveAt(index);
            return Normalize(kept);
        }

        if (index >= 0 || ColumnCatalog.TryGet(id) is not { } spec) return Normalize(kept);
        kept.Add(new ColumnSetting(spec.Id, spec.DefaultWidth));
        return Normalize(kept);
    }

    /// <summary>
    /// The layout after a trip through the "More columns…" picker, whose answer is the whole set of
    /// shell properties that should be showing.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Removals first and then additions, both through <see cref="Toggle"/>, so a column that was
    /// already there <b>keeps its place and its width</b> rather than being taken out and put back
    /// at the end — opening the picker and pressing OK changes nothing at all.
    /// </para>
    /// <para>
    /// Built-in columns are not the picker's business and are left exactly as they are: it lists the
    /// property system, and a built-in missing from <paramref name="chosen"/> means "not offered
    /// here", never "remove it".
    /// </para>
    /// </remarks>
    public static IReadOnlyList<ColumnSetting> ApplyPicked(
        IReadOnlyList<ColumnSetting>? user, IReadOnlyList<string> chosen)
    {
        var wanted = new HashSet<string>(chosen, StringComparer.OrdinalIgnoreCase);
        var layout = Normalize(user);

        foreach (var setting in layout)
        {
            if (ColumnCatalog.TryGet(setting.Id) is { Kind: ColumnKind.ShellProperty }
                && !wanted.Contains(setting.Id))
                layout = Toggle(layout, setting.Id, on: false);
        }

        foreach (var id in chosen)
            layout = Toggle(layout, id, on: true);

        return layout;
    }

    /// <summary>Moves a column to a position. An out-of-range target clamps rather than throws.</summary>
    public static IReadOnlyList<ColumnSetting> Move(IReadOnlyList<ColumnSetting>? user, string id, int targetIndex)
    {
        var kept = Copy(Normalize(user));
        var index = IndexOf(kept, id);
        if (index < 0) return Normalize(kept);

        var target = Math.Clamp(targetIndex, 0, kept.Count - 1);
        if (target == index) return Normalize(kept);

        var moved = kept[index];
        kept.RemoveAt(index);
        kept.Insert(target, moved);
        return Normalize(kept);
    }

    public static IReadOnlyList<ColumnSetting> SetWidth(IReadOnlyList<ColumnSetting>? user, string id, double width)
    {
        var kept = Copy(Normalize(user));
        var index = IndexOf(kept, id);
        if (index < 0) return kept;

        var spec = ColumnCatalog.TryGet(id)!;
        kept[index].Width = SaneWidth(width, spec.DefaultWidth);
        return kept;
    }

    /// <summary>
    /// The saved list rewritten to match the order the view now has, after a header was dragged.
    /// </summary>
    /// <remarks>
    /// The live collection holds the injected columns as well, which the model must never learn
    /// about — so they are dropped here rather than at the call site, and widths are carried across
    /// from the settings the ids came from. Name is put back at the front, which is what visually
    /// snaps a dragged Name column home.
    /// </remarks>
    public static IReadOnlyList<ColumnSetting> CaptureOrder(
        IReadOnlyList<string> liveIds, IReadOnlyList<ColumnSetting>? user)
    {
        var widths = new Dictionary<string, double>(StringComparer.OrdinalIgnoreCase);
        foreach (var setting in Normalize(user))
            widths[setting.Id] = setting.Width;

        var rebuilt = new List<ColumnSetting>(liveIds.Count);
        foreach (var id in liveIds)
        {
            if (ColumnCatalog.IsInjected(id)) continue;
            if (ColumnCatalog.TryGet(id) is not { } spec) continue;
            rebuilt.Add(new ColumnSetting(
                spec.Id, widths.TryGetValue(spec.Id, out var width) ? width : spec.DefaultWidth));
        }
        return Normalize(rebuilt);
    }

    private static List<ColumnSetting> Copy(IReadOnlyList<ColumnSetting> settings) =>
        settings.Select(c => c.Copy()).ToList();

    private static int IndexOf(List<ColumnSetting> settings, string id) =>
        settings.FindIndex(c => string.Equals(c.Id, id, StringComparison.OrdinalIgnoreCase));
}
