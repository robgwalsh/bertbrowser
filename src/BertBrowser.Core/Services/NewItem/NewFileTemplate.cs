namespace BertBrowser.Core.Services.NewItem;

/// <summary>
/// One entry on the "New" submenu, persisted in settings.json.
/// </summary>
/// <remarks>
/// Plain get/set so the file stays hand-editable, and deliberately only two shapes: a template is
/// either empty or a file on disk. Windows' ShellNew has a third — bytes held in the registry — but
/// those are written out to a real file once, at import, so nothing downstream has to know.
/// </remarks>
public sealed class NewFileTemplate
{
    /// <summary>What the menu says, e.g. "Text Document".</summary>
    public string Label { get; set; } = "";

    /// <summary>Including the dot, e.g. ".txt".</summary>
    public string Extension { get; set; } = "";

    /// <summary>A file to copy the new file's contents from, or null for an empty one.</summary>
    public string? TemplatePath { get; set; }

    /// <summary>Whether it appears on the menu. Defaults to true, which is also what an entry
    /// written before this property existed deserializes to.</summary>
    public bool Enabled { get; set; } = true;

    /// <summary>The name the dialog opens with, before its extension: "New Text Document".</summary>
    public string DefaultBaseName => $"New {Label}";

    /// <summary>What ships when the user has never configured the list.</summary>
    public static List<NewFileTemplate> Defaults() =>
    [
        new() { Label = "Text Document", Extension = ".txt" },
        new() { Label = "Markdown Document", Extension = ".md" },
        new() { Label = "JSON File", Extension = ".json" },
    ];
}
