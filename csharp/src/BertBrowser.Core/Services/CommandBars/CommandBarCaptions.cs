using BertBrowser.Core.Services.Commands;

namespace BertBrowser.Core.Services.CommandBars;

/// <summary>
/// What a command is called on a menu, where that differs from its catalogue name. The catalogue
/// names a command for the palette, which wants a verb to type ("Toggle preview pane"); a menu
/// shows a tick, so it wants the thing ticked ("Preview pane").
/// </summary>
public static class CommandBarCaptions
{
    private static readonly Dictionary<string, string> Short = new(StringComparer.Ordinal)
    {
        ["nav.up"] = "Up",
        ["view.preview"] = "Preview pane",
        ["view.metadata"] = "Metadata pane",
        ["view.flat"] = "Flat view",
        ["view.hidden"] = "Hidden items",
        ["preview.wrap"] = "Word wrap",
        ["preview.fit"] = "Fit image to pane",
        ["preview.auto-advance"] = "Auto-advance",
        ["preview.loop"] = "Loop",
        ["preview.autoplay"] = "Autoplay",
        ["tools.compare-differences"] = "Show only differences",
        ["tools.compare-rescan"] = "Rescan comparison",
        ["tools.compare-sync"] = "Sync…",
        ["app.match-windows-theme"] = "Match Windows light/dark",
        ["app.command-bars"] = "Customise menus and toolbars…",
        ["app.about"] = "About BertBrowser",
    };

    /// <summary>The menu's words for a command, or null when it has none of its own and the
    /// command's name stands.</summary>
    public static string? For(string id) => Short.GetValueOrDefault(id);

    /// <summary>Every id given a caption here, for the test that none has gone stale.</summary>
    public static IEnumerable<string> Ids => Short.Keys;

    /// <summary>What an item reads as with nothing but the model to go on — the editor's row, and
    /// the fallback when a command's live state has no caption of its own.</summary>
    public static string Describe(BarItem item) => item.Kind switch
    {
        BarItemKinds.Separator => "Separator",
        BarItemKinds.Menu => AccessKeys.Strip(item.Caption ?? ""),
        BarItemKinds.List => CommandBarLists.Find(item.Id)?.Name ?? item.Id ?? "",
        _ when item.Caption is { Length: > 0 } caption => AccessKeys.Strip(caption),
        _ when item.Id is { } id => For(id) ?? CommandCatalog.Find(id)?.Name ?? id,
        _ => "",
    };
}
