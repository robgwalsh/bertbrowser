using System.Text.Json.Serialization;

namespace BertBrowser.Core.Services.CommandBars;

/// <summary>What a <see cref="BarItem"/> is. Text rather than an enum because it is written to the
/// settings file: a kind a later build adds must survive a round trip through this one.</summary>
public static class BarItemKinds
{
    /// <summary>A command, by the id the registry runs it under — a catalogue id, or
    /// <c>custom:&lt;guid&gt;</c> for one of the user's own.</summary>
    public const string Command = "command";
    public const string Separator = "separator";

    /// <summary>A submenu: a caption and children. On a toolbar, a button that drops them down.</summary>
    public const string Menu = "menu";

    /// <summary>Rows that come from data — the saved workspaces, the drives — by a
    /// <see cref="CommandBarLists"/> id.</summary>
    public const string List = "list";
}

/// <summary>
/// One entry of a menu or a toolbar. The same shape serves both, so an item can be dragged from one
/// to the other and a saved arrangement is one tree.
/// </summary>
/// <param name="Caption">For a menu, its name. For a command, the words to show instead of its own
/// — "Name" under "Sort by" — or null for those. An <c>&amp;</c> marks the access key.</param>
/// <param name="ShowText">On a toolbar, whether this button carries its caption beside its icon.
/// Null follows the toolbar.</param>
public sealed record BarItem(
    string Kind,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Id = null,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Caption = null,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] IReadOnlyList<BarItem>? Items = null,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] bool? ShowText = null)
{
    public static BarItem Separator { get; } = new(BarItemKinds.Separator);

    public static BarItem Command(string id, string? caption = null) => new(BarItemKinds.Command, id, caption);

    public static BarItem Menu(string caption, params BarItem[] items) => new(BarItemKinds.Menu, Caption: caption, Items: items);

    public static BarItem List(string id) => new(BarItemKinds.List, id);

    [JsonIgnore] public bool IsCommand => Kind == BarItemKinds.Command;
    [JsonIgnore] public bool IsSeparator => Kind == BarItemKinds.Separator;
    [JsonIgnore] public bool IsMenu => Kind == BarItemKinds.Menu;
    [JsonIgnore] public bool IsList => Kind == BarItemKinds.List;
}

/// <summary>One toolbar: the title bar's, or a row under it.</summary>
/// <param name="ShowText">Whether its buttons carry their captions unless one says otherwise.</param>
public sealed record ToolbarSpec(IReadOnlyList<BarItem> Items, bool ShowText = false);

/// <summary>
/// The whole arrangement: the menu bar, the title bar's toolbar and any rows below. This is what a
/// saved arrangement holds and what the settings file keeps as the one in force.
/// </summary>
/// <param name="Revision">Which <see cref="CommandBarDefaults.Revision"/> this was arranged
/// against, so a later build can tell what has been added since.</param>
/// <param name="Menus">The top-level menus, each a <see cref="BarItemKinds.Menu"/>.</param>
public sealed record CommandBarSet(
    int Revision,
    IReadOnlyList<BarItem> Menus,
    ToolbarSpec TitleBar,
    IReadOnlyList<ToolbarSpec> Rows);

/// <summary>An arrangement somebody named, to come back to.</summary>
public sealed record SavedCommandBarSet(string Name, CommandBarSet Set);
