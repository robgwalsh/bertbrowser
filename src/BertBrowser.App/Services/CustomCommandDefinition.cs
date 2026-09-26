namespace BertBrowser.App.Services;

/// <summary>A user-defined context menu entry, persisted in settings.json.</summary>
public sealed class CustomCommandDefinition
{
    /// <summary>What <see cref="AppSettings.ContextMenuLayout"/> places this command by
    /// (<c>cmd:&lt;id&gt;</c>). A command saved before ids existed gets a fresh one on every load
    /// until the Settings page next saves it, and meanwhile sits where unplaced commands go — which
    /// is where every command sat before the menu could be arranged.</summary>
    public string Id { get; set; } = Guid.NewGuid().ToString("N");

    /// <summary>Whether the command is on the menu. Off means the user took it off the menu without
    /// deleting it.</summary>
    public bool ShowInMenu { get; set; } = true;

    public string Name { get; set; } = "";

    /// <summary>Program to run (full path or anything resolvable by the shell).</summary>
    public string Command { get; set; } = "";

    /// <summary>Argument template; see <see cref="BertBrowser.Core.Services.CommandTemplate"/>.</summary>
    public string Arguments { get; set; } = "";

    public bool AppliesToFiles { get; set; } = true;
    public bool AppliesToDirectories { get; set; }

    /// <summary>
    /// Run this command as administrator, with a UAC prompt each time. Off by default, which is
    /// also what an older <c>settings.json</c> without this property deserializes to — so commands
    /// configured before this existed keep running as the ordinary user, which is the safe way
    /// round.
    /// </summary>
    public bool RunElevated { get; set; }
}
