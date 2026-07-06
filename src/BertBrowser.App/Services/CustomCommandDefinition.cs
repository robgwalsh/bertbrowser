namespace BertBrowser.App.Services;

/// <summary>A user-defined context menu entry, persisted in settings.json.</summary>
public sealed class CustomCommandDefinition
{
    public string Name { get; set; } = "";

    /// <summary>Program to run (full path or anything resolvable by the shell).</summary>
    public string Command { get; set; } = "";

    /// <summary>Argument template; see <see cref="BertBrowser.Core.Services.CommandTemplate"/>.</summary>
    public string Arguments { get; set; } = "";

    public bool AppliesToFiles { get; set; } = true;
    public bool AppliesToDirectories { get; set; }
}
