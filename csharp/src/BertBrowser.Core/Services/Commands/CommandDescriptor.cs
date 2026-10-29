namespace BertBrowser.Core.Services.Commands;

/// <summary>
/// Where a command's <em>shortcut</em> is live. It says nothing about the palette, which can run
/// any available command from anywhere; it is the rule that used to be spread across three key
/// handlers as "which control has focus".
/// </summary>
public enum CommandContext
{
    /// <summary>Always, the settings page included.</summary>
    App,

    /// <summary>Whenever the folders are showing — anywhere in the window, but not while the
    /// settings page is standing in for them.</summary>
    Browser,

    /// <summary>Only while a file list has the keyboard. Enter, Del and Ctrl+C mean something else
    /// in the folder tree and in a text box, so they are the list's alone.</summary>
    FileList,
}

/// <summary>How readily the palette offers a command nobody asked for by name.</summary>
public enum CommandProminence
{
    /// <summary>Eligible for the suggestions an empty palette shows.</summary>
    Essential,
    Standard,

    /// <summary>Found by typing, ranked behind an equal match.</summary>
    Rare,
}

/// <summary>
/// One thing the app can be told to do: a stable id, the words the palette and the Keyboard page
/// show, and the shortcut it ships with. What it <em>does</em> lives in the App, keyed by the same id.
/// </summary>
/// <param name="Id">Dotted and lowercase (<c>tab.new</c>). Persisted in the keymap and the pins,
/// so it never changes meaning.</param>
/// <param name="Aliases">Other words people look for it under — a competitor's name for the same
/// thing, or the noun the name left out.</param>
public sealed record CommandDescriptor(
    string Id,
    string Name,
    string Category,
    CommandContext Context,
    string? Icon,
    IReadOnlyList<KeyChord> DefaultChords,
    IReadOnlyList<string> Aliases,
    CommandProminence Prominence);

/// <summary>The palette's groups, in the order "Browse all commands" lists them.</summary>
public static class CommandCategories
{
    public const string Navigation = "Go";
    public const string Tabs = "Tabs";
    public const string Panes = "Panes";
    public const string Selection = "Selection";
    public const string File = "File";
    public const string Edit = "Edit";
    public const string View = "View";
    public const string Search = "Search";
    public const string Tools = "Tools";
    public const string Preview = "Preview";
    public const string App = "App";

    public static IReadOnlyList<string> All { get; } =
        [Navigation, Tabs, Panes, Selection, File, Edit, View, Search, Tools, Preview, App];
}
