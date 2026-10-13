namespace BertBrowser.Core.Services.CommandBars;

public enum BarTargetKind
{
    /// <summary>The row of top-level menus.</summary>
    MenuBar,

    /// <summary>The title bar's toolbar.</summary>
    TitleBar,

    /// <summary>A toolbar row under the title bar, by index.</summary>
    Row,
}

/// <summary>Which bar of an arrangement is meant.</summary>
public readonly record struct BarTarget(BarTargetKind Kind, int Row = 0)
{
    public static BarTarget MenuBar => new(BarTargetKind.MenuBar);
    public static BarTarget TitleBar => new(BarTargetKind.TitleBar);
    public static BarTarget ForRow(int row) => new(BarTargetKind.Row, row);
}

/// <summary>
/// Changes to an arrangement, each returning a new one. A place in it is a bar and a path of
/// indices down through its submenus — <c>(MenuBar, [0, 1])</c> is the second item of the first
/// menu, which had better be a submenu — so the editor's flat list is always "the items here".
/// </summary>
/// <remarks>
/// Nothing here throws for a place that does not exist: the arrangement can be replaced from a menu
/// or the palette while the editor is open on it, and an edit aimed at what is no longer there
/// should do nothing rather than take the settings page down.
/// </remarks>
public static class CommandBarEdit
{
    /// <summary>The items at a place, or empty when there is no such place.</summary>
    public static IReadOnlyList<BarItem> ItemsAt(CommandBarSet set, BarTarget target, IReadOnlyList<int> path)
    {
        var items = Root(set, target);
        foreach (var index in path)
        {
            if (items is null || index < 0 || index >= items.Count || !items[index].IsMenu) return [];
            items = items[index].Items ?? [];
        }
        return items ?? [];
    }

    /// <summary>Whether the place exists: the bar is there, and every step of the path is a submenu.</summary>
    public static bool Exists(CommandBarSet set, BarTarget target, IReadOnlyList<int> path)
    {
        var items = Root(set, target);
        foreach (var index in path)
        {
            if (items is null || index < 0 || index >= items.Count || !items[index].IsMenu) return false;
            items = items[index].Items ?? [];
        }
        return items is not null;
    }

    /// <summary>The names on the way down: the bar's, then each submenu's.</summary>
    public static IReadOnlyList<string> Trail(CommandBarSet set, BarTarget target, IReadOnlyList<int> path)
    {
        var trail = new List<string> { NameOf(target) };
        var items = Root(set, target);
        foreach (var index in path)
        {
            if (items is null || index < 0 || index >= items.Count || !items[index].IsMenu) break;
            trail.Add(AccessKeys.Strip(items[index].Caption ?? ""));
            items = items[index].Items ?? [];
        }
        return trail;
    }

    public static string NameOf(BarTarget target) => target.Kind switch
    {
        BarTargetKind.MenuBar => "Menu bar",
        BarTargetKind.TitleBar => "Title bar toolbar",
        _ => $"Toolbar row {target.Row + 1}",
    };

    /// <summary>Whether an item may sit at a place. Only one rule: the menu bar itself holds
    /// menus and nothing else — a command there would be a heading that runs when clicked.</summary>
    public static bool Accepts(BarTarget target, IReadOnlyList<int> path, BarItem item) =>
        target.Kind != BarTargetKind.MenuBar || path.Count > 0 || item.IsMenu;

    public static CommandBarSet Insert(
        CommandBarSet set, BarTarget target, IReadOnlyList<int> path, int index, BarItem item)
    {
        if (!Exists(set, target, path) || !Accepts(target, path, item)) return set;
        if (item.IsMenu && path.Count + 1 > CommandBarRules.MaxDepth) return set;

        var items = ItemsAt(set, target, path).ToList();
        items.Insert(Math.Clamp(index, 0, items.Count), item);
        return WithItemsAt(set, target, path, items);
    }

    public static CommandBarSet Remove(CommandBarSet set, BarTarget target, IReadOnlyList<int> path, int index)
    {
        var items = ItemsAt(set, target, path).ToList();
        if (index < 0 || index >= items.Count) return set;

        items.RemoveAt(index);
        return WithItemsAt(set, target, path, items);
    }

    /// <summary>Moves an item to the slot it should end up in, counted after it has been taken out.</summary>
    public static CommandBarSet Move(CommandBarSet set, BarTarget target, IReadOnlyList<int> path, int from, int to)
    {
        var items = ItemsAt(set, target, path).ToList();
        if (from < 0 || from >= items.Count) return set;

        var item = items[from];
        items.RemoveAt(from);
        items.Insert(Math.Clamp(to, 0, items.Count), item);
        return WithItemsAt(set, target, path, items);
    }

    public static CommandBarSet Replace(
        CommandBarSet set, BarTarget target, IReadOnlyList<int> path, int index, BarItem item)
    {
        var items = ItemsAt(set, target, path).ToList();
        if (index < 0 || index >= items.Count) return set;

        items[index] = item;
        return WithItemsAt(set, target, path, items);
    }

    public static CommandBarSet AddRow(CommandBarSet set) =>
        set with { Rows = [.. set.Rows, new ToolbarSpec([])] };

    public static CommandBarSet RemoveRow(CommandBarSet set, int row) =>
        row < 0 || row >= set.Rows.Count ? set : set with { Rows = [.. set.Rows.Where((_, i) => i != row)] };

    /// <summary>Whether a toolbar's buttons carry their names. False for the menu bar, which has
    /// no buttons.</summary>
    public static bool ShowsText(CommandBarSet set, BarTarget target) => Toolbar(set, target)?.ShowText ?? false;

    public static CommandBarSet WithShowText(CommandBarSet set, BarTarget target, bool showText) =>
        Toolbar(set, target) is { } toolbar ? WithToolbar(set, target, toolbar with { ShowText = showText }) : set;

    private static IReadOnlyList<BarItem>? Root(CommandBarSet set, BarTarget target) => target.Kind switch
    {
        BarTargetKind.MenuBar => set.Menus,
        _ => Toolbar(set, target)?.Items,
    };

    private static ToolbarSpec? Toolbar(CommandBarSet set, BarTarget target) => target.Kind switch
    {
        BarTargetKind.TitleBar => set.TitleBar,
        BarTargetKind.Row when target.Row >= 0 && target.Row < set.Rows.Count => set.Rows[target.Row],
        _ => null,
    };

    private static CommandBarSet WithToolbar(CommandBarSet set, BarTarget target, ToolbarSpec toolbar) =>
        target.Kind == BarTargetKind.TitleBar
            ? set with { TitleBar = toolbar }
            : set with { Rows = [.. set.Rows.Select((r, i) => i == target.Row ? toolbar : r)] };

    private static CommandBarSet WithItemsAt(
        CommandBarSet set, BarTarget target, IReadOnlyList<int> path, IReadOnlyList<BarItem> items)
    {
        if (Root(set, target) is not { } root) return set;

        var replaced = Descend(root, path, 0, items);
        return target.Kind == BarTargetKind.MenuBar
            ? set with { Menus = replaced }
            : WithToolbar(set, target, Toolbar(set, target)! with { Items = replaced });
    }

    private static IReadOnlyList<BarItem> Descend(
        IReadOnlyList<BarItem> here, IReadOnlyList<int> path, int depth, IReadOnlyList<BarItem> items)
    {
        if (depth == path.Count) return items;

        var index = path[depth];
        if (index < 0 || index >= here.Count || !here[index].IsMenu) return here;

        var copy = here.ToArray();
        copy[index] = here[index] with { Items = Descend(here[index].Items ?? [], path, depth + 1, items) };
        return copy;
    }
}
