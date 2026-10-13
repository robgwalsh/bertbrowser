using BertBrowser.Core.Services.ShellMenu;

namespace BertBrowser.Core.Services.CommandBars;

/// <summary>A command as it stands this instant, as far as a menu or a toolbar needs to know.</summary>
/// <param name="Name">Its catalogue name (or a custom command's own), plain text.</param>
/// <param name="Gesture">Its shortcut in force, as text; empty when it has none.</param>
/// <param name="Unavailable">Why it cannot run now, or null when it can.</param>
/// <param name="LiveCaption">Words that depend on the moment — "Delete 3 items…", "Remove
/// bookmark" — plain text, or null when the command reads the same always.</param>
public sealed record BarCommandState(
    string Name, string? Icon, string Gesture, string? Unavailable, bool IsChecked = false, string? LiveCaption = null);

/// <summary>What the rules ask of the running app in order to lay a bar out.</summary>
public interface IBarSource
{
    /// <summary>Null for an id nothing answers to — a command from a later build, or one of the
    /// user's own since deleted. Such an item is kept in the arrangement and simply not shown.</summary>
    BarCommandState? Command(string id);

    bool ListHasRows(string listId);
}

public enum BarRowKind
{
    Command,
    Separator,
    Menu,

    /// <summary>Stands where a list's rows go; the App fills them in.</summary>
    List,

    /// <summary>The greyed line an empty list leaves ("No saved workspaces").</summary>
    Placeholder,
}

/// <summary>One thing to draw. Only what shows is ever a row: hidden separators, unknown commands
/// and empty submenus have already gone.</summary>
/// <param name="Caption">With its access key marked (<see cref="AccessKeys"/>) on a menu; plain
/// on a toolbar.</param>
/// <param name="Name">The command's full name, for a tooltip, whatever the caption was cut to.</param>
public sealed record BarRow(
    BarRowKind Kind,
    string? Id,
    string Caption,
    string Name,
    string? Icon,
    string Gesture,
    bool IsEnabled,
    string? Reason,
    bool IsChecked,
    bool ShowText,
    IReadOnlyList<BarRow> Children);

/// <summary>
/// The decisions behind the menu bar and the toolbars, kept pure: what an arrangement read back
/// from disk may contain, which commands it leaves out, and what a menu shows right now.
/// </summary>
public static class CommandBarRules
{
    /// <summary>How deep submenus go. A menu under a menu under a menu under a menu is already
    /// further than anybody follows; this is here so a hand-edited file cannot recurse for ever.</summary>
    public const int MaxDepth = 4;

    /// <summary>The commands that carry a list of their own when they sit on a toolbar: Undo and
    /// Redo are split buttons, as they were when they were fixed there.</summary>
    public static string? DropdownFor(string commandId) => commandId switch
    {
        "edit.undo" => CommandBarLists.UndoSteps,
        "edit.redo" => CommandBarLists.RedoSteps,
        _ => null,
    };

    // --- What an arrangement may hold ---

    /// <summary>
    /// An arrangement as read from the settings file, made safe to walk: nothing null, nothing
    /// nested past <see cref="MaxDepth"/>, no menu without a name and no command without an id.
    /// A command or a kind this build has never heard of is <em>kept</em> — it may be a later
    /// build's, and dropping it would lose it on the way back.
    /// </summary>
    public static CommandBarSet Clean(CommandBarSet? set)
    {
        if (set is null) return CommandBarDefaults.Default;

        return new CommandBarSet(
            set.Revision,
            [.. CleanItems(set.Menus, 1).Where(m => m.IsMenu)],
            CleanToolbar(set.TitleBar),
            [.. (set.Rows ?? []).Where(r => r is not null).Select(CleanToolbar)]);
    }

    private static ToolbarSpec CleanToolbar(ToolbarSpec? toolbar) =>
        new(CleanItems(toolbar?.Items, 1), toolbar?.ShowText ?? false);

    private static IReadOnlyList<BarItem> CleanItems(IReadOnlyList<BarItem>? items, int depth)
    {
        var clean = new List<BarItem>();
        foreach (var item in items ?? [])
        {
            if (item is null || string.IsNullOrWhiteSpace(item.Kind)) continue;

            var caption = string.IsNullOrWhiteSpace(item.Caption) ? null : item.Caption.Trim();
            switch (item.Kind)
            {
                case BarItemKinds.Separator:
                    clean.Add(BarItem.Separator);
                    break;
                case BarItemKinds.Menu:
                    if (caption is null || depth > MaxDepth) continue;
                    clean.Add(item with { Id = null, Caption = caption, Items = CleanItems(item.Items, depth + 1) });
                    break;
                case BarItemKinds.Command or BarItemKinds.List:
                    if (string.IsNullOrWhiteSpace(item.Id)) continue;
                    clean.Add(item with { Id = item.Id.Trim(), Caption = caption, Items = null });
                    break;
                default:
                    clean.Add(item);
                    break;
            }
        }
        return clean;
    }

    /// <summary>Whether two arrangements are the same arrangement, whatever revision each was
    /// made against. Records compare their lists by reference, so this walks them.</summary>
    public static bool Same(CommandBarSet a, CommandBarSet b) =>
        Same(a.Menus, b.Menus) && Same(a.TitleBar, b.TitleBar) &&
        a.Rows.Count == b.Rows.Count && a.Rows.Zip(b.Rows).All(pair => Same(pair.First, pair.Second));

    private static bool Same(ToolbarSpec a, ToolbarSpec b) => a.ShowText == b.ShowText && Same(a.Items, b.Items);

    private static bool Same(IReadOnlyList<BarItem>? a, IReadOnlyList<BarItem>? b)
    {
        a ??= [];
        b ??= [];
        return a.Count == b.Count && a.Zip(b).All(pair =>
            pair.First.Kind == pair.Second.Kind && pair.First.Id == pair.Second.Id &&
            pair.First.Caption == pair.Second.Caption && pair.First.ShowText == pair.Second.ShowText &&
            Same(pair.First.Items, pair.Second.Items));
    }

    /// <summary>Whether this is the arrangement the app ships with — in which case nothing need
    /// be stored, and a later build's additions arrive by themselves.</summary>
    public static bool IsDefault(CommandBarSet set) => Same(set, CommandBarDefaults.Default);

    /// <summary>Every item of the arrangement, menus and toolbars alike, at any depth.</summary>
    public static IEnumerable<BarItem> Flatten(CommandBarSet set) =>
        Flatten(set.Menus).Concat(Flatten(set.TitleBar.Items)).Concat(set.Rows.SelectMany(r => Flatten(r.Items)));

    public static IEnumerable<BarItem> Flatten(IEnumerable<BarItem>? items)
    {
        foreach (var item in items ?? [])
        {
            yield return item;
            foreach (var child in Flatten(item.Items)) yield return child;
        }
    }

    /// <summary>How many times each command is placed, anywhere.</summary>
    public static IReadOnlyDictionary<string, int> Placements(CommandBarSet set) =>
        Flatten(set).Where(i => i.IsCommand && i.Id is not null)
            .GroupBy(i => i.Id!, StringComparer.Ordinal)
            .ToDictionary(g => g.Key, g => g.Count(), StringComparer.Ordinal);

    /// <summary>The commands that are on no menu and no toolbar, in the order given — what the
    /// editor's "not placed anywhere" filter lists.</summary>
    public static IReadOnlyList<string> Unplaced(CommandBarSet set, IEnumerable<string> commandIds)
    {
        var placed = Placements(set);
        return [.. commandIds.Where(id => !placed.ContainsKey(id))];
    }

    // --- What shows right now ---

    /// <summary>
    /// The rows one menu, or one toolbar, shows at this moment.
    /// </summary>
    /// <remarks>
    /// A command is never hidden for being unavailable — it is greyed and says why, as in the
    /// palette — so a menu keeps its shape and people keep their place in it. What does go: a
    /// command nothing answers to, a list with no rows and no empty-state line, a submenu left
    /// with nothing in it, and then whichever separators that leaves doubled or at an edge.
    /// </remarks>
    /// <param name="accessKeys">Menus mark a key on every caption; toolbars do not, or a letter
    /// typed at the file list could press a button.</param>
    /// <param name="showText">For a toolbar, whether its buttons carry captions by default.</param>
    public static IReadOnlyList<BarRow> Present(
        IReadOnlyList<BarItem> items, IBarSource source, bool accessKeys = true, bool showText = false) =>
        Present(items, source, accessKeys, showText, depth: 1);

    private static IReadOnlyList<BarRow> Present(
        IReadOnlyList<BarItem> items, IBarSource source, bool accessKeys, bool showText, int depth)
    {
        var rows = new List<BarRow?>(items.Count);
        foreach (var item in items)
        {
            rows.Add(item.Kind switch
            {
                BarItemKinds.Separator => Row(BarRowKind.Separator, null, ""),
                BarItemKinds.Command => PresentCommand(item, source, accessKeys, showText),
                BarItemKinds.List => PresentList(item, source),
                BarItemKinds.Menu when depth <= MaxDepth => PresentMenu(item, source, accessKeys, showText, depth),
                _ => null,
            });
        }

        var visible = MenuSeparatorRules.Apply(
            [.. rows.Select(r => new MenuSlot(r is { Kind: BarRowKind.Separator }, r is not null))]);
        var shown = rows.Where((_, i) => visible[i]).Select(r => r!).ToList();

        return accessKeys ? WithAccessKeys(shown) : shown;
    }

    private static BarRow? PresentCommand(BarItem item, IBarSource source, bool accessKeys, bool showText)
    {
        if (item.Id is not { } id || source.Command(id) is not { } state) return null;

        // The words somebody chose for this spot win; then what the command calls itself at this
        // moment; then the menu's shorter name for it; then its own.
        var caption = item.Caption
            ?? AccessKeys.Escape(Live(state) ?? CommandBarCaptions.For(id) ?? state.Name);
        if (!accessKeys) caption = AccessKeys.Strip(caption);

        return new BarRow(
            BarRowKind.Command, id, caption, state.Name, state.Icon, state.Gesture,
            IsEnabled: state.Unavailable is null, state.Unavailable, state.IsChecked,
            item.ShowText ?? showText, []);
    }

    /// <summary>A caption for the moment, when there is one. Empty is none: "nothing to undo" has
    /// no description, and a blank menu row would be worse than the command's own name.</summary>
    private static string? Live(BarCommandState state) => state.LiveCaption is { Length: > 0 } live ? live : null;

    private static BarRow? PresentList(BarItem item, IBarSource source)
    {
        if (CommandBarLists.Find(item.Id) is not { } list) return null;
        if (source.ListHasRows(list.Id)) return Row(BarRowKind.List, list.Id, AccessKeys.Escape(list.Name), list.Icon);

        return list.EmptyText is { } empty
            ? Row(BarRowKind.Placeholder, list.Id, AccessKeys.Escape(empty), list.Icon) with { IsEnabled = false }
            : null;
    }

    private static BarRow? PresentMenu(BarItem item, IBarSource source, bool accessKeys, bool showText, int depth)
    {
        // Inside a dropdown every row is a menu row, whatever the toolbar it hangs from wanted.
        var children = Present(item.Items ?? [], source, accessKeys: true, showText: false, depth + 1);
        if (children.Count == 0) return null;

        var caption = accessKeys ? item.Caption ?? "" : AccessKeys.Strip(item.Caption ?? "");
        return Row(BarRowKind.Menu, null, caption) with { Children = children, ShowText = item.ShowText ?? showText };
    }

    private static BarRow Row(BarRowKind kind, string? id, string caption, string? icon = null) =>
        new(kind, id, caption, AccessKeys.Strip(caption), icon, "", IsEnabled: true, null, false, false, []);

    /// <summary>Keys for the rows people choose between. A list's rows are data and a
    /// placeholder cannot be chosen, so neither takes a letter from the rest.</summary>
    private static IReadOnlyList<BarRow> WithAccessKeys(IReadOnlyList<BarRow> rows)
    {
        var keyed = rows.Select((row, index) => (row, index))
            .Where(x => x.row.Kind is BarRowKind.Command or BarRowKind.Menu)
            .ToList();
        var captions = AccessKeys.Assign([.. keyed.Select(x => x.row.Caption)]);

        var result = rows.ToArray();
        for (var i = 0; i < keyed.Count; i++)
            result[keyed[i].index] = keyed[i].row with { Caption = captions[i] };
        return result;
    }

    // --- Toolbar overflow ---

    /// <summary>
    /// How many of a toolbar's items fit, from the left. All of them when there is room;
    /// otherwise as many as leave space for the overflow button that will hold the rest.
    /// </summary>
    public static int Fit(IReadOnlyList<double> widths, double available, double overflowWidth)
    {
        if (widths.Sum() <= available) return widths.Count;

        var used = overflowWidth;
        var count = 0;
        foreach (var width in widths)
        {
            if (used + width > available) break;
            used += width;
            count++;
        }
        return count;
    }
}
