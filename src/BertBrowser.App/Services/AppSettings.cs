using System.Text.Json;

namespace BertBrowser.App.Services;

public sealed class AppSettings
{
    public double? WindowLeft { get; set; }
    public double? WindowTop { get; set; }
    public double? WindowWidth { get; set; }
    public double? WindowHeight { get; set; }
    public bool WindowMaximized { get; set; }
    public string? LastPath { get; set; }

    /// <summary>
    /// The pane and tab arrangement from the last session.
    /// </summary>
    /// <remarks>
    /// Nullable for the reason <see cref="NewFileTypes"/> and <see cref="ThemeId"/> are: null means
    /// never saved, and a first launch then behaves exactly as it always did. <see cref="LastPath"/>
    /// stays as the fallback for that case, and for a session too damaged to restore.
    /// </remarks>
    public BertBrowser.Core.Layout.SessionLayout? Session { get; set; }

    /// <summary>When false (default), hidden files/folders are excluded from listings and
    /// search; when true they appear with a dimmed icon, like Explorer.</summary>
    public bool ShowHiddenItems { get; set; }

    /// <summary>Per-directory thumbnail-zoom slider position (0..1), keyed by canonical path.
    /// 0 (or absent) = details list. Only folders explicitly zoomed are stored.</summary>
    public Dictionary<string, double> DirectoryThumbnailScales { get; set; } = new();

    /// <summary>Mouse-wheel vertical scroll speed multiplier (1 = OS default). Defaults to 2×.</summary>
    public double ScrollSpeedMultiplier { get; set; } = 2.0;

    /// <summary>Whether a newly opened tab shows its preview pane. Not nullable, unlike
    /// <see cref="ThemeId"/> and <see cref="NewFileTypes"/>: "never configured" and "off" mean the
    /// same thing for a pane, so there is nothing for null to say. Visibility itself is per tab —
    /// this is the value a new tab starts from, and toggling in any tab writes it.</summary>
    public bool ShowPreviewPane { get; set; }

    /// <summary>Width of the preview pane, in device-independent pixels. Global rather than per
    /// tab on purpose: panes differ in width, so a remembered per-tab width reads as the splitter
    /// moving on its own rather than as the app remembering anything.</summary>
    public double PreviewPaneWidth { get; set; } = 360;

    /// <summary>How much of a text file the preview reads. The rest is not shown and the pane says
    /// so — a preview that silently stops looks like the whole file.</summary>
    public int PreviewTextMaxBytes { get; set; } = 1024 * 1024;

    /// <summary>The floor the duplicate finder starts from, in bytes.</summary>
    /// <remarks>
    /// Not cosmetic: it bounds both the shortlist's memory and how much has to be read, and
    /// duplicate 400-byte files are not what anyone opens that window for. The picker offers
    /// magnitudes; this is what a session starts from and what changing it writes back.
    /// </remarks>
    public long DuplicateMinSizeBytes { get; set; } = 1024 * 1024;

    /// <summary>Whether the duplicate finder leaves Windows and Program Files out.</summary>
    /// <remarks>
    /// On by default. Those trees are largely one file under several names, and what genuine
    /// duplicates they hold are not the user's to remove.
    /// </remarks>
    public bool DuplicateSkipSystemFolders { get; set; } = true;

    /// <summary>Shape of a thumbnail tile as "width:height" — the zoom slider sets the width and
    /// this decides the height. Anything unparseable falls back to 4:3 (see
    /// <see cref="BertBrowser.Core.Models.AspectRatio"/>), so hand-editing this can't break the
    /// view; the settings picker offers the common shapes but any "W:H" works.</summary>
    public string TileAspectRatio { get; set; } = BertBrowser.Core.Models.AspectRatio.Default.ToString();

    public List<CustomCommandDefinition> CustomCommands { get; set; } = new();

    /// <summary>The file types on the "New" submenu, in menu order. Null means the user has never
    /// configured the list, which is what lets a first launch ship
    /// <see cref="BertBrowser.Core.Services.NewItem.NewFileTemplate.Defaults"/> — an empty list is a
    /// different thing entirely, and means they removed them all on purpose. Same reasoning as
    /// <see cref="ThemeId"/>; "New ▸ Folder" is not in here because it is never configurable.</summary>
    public List<BertBrowser.Core.Services.NewItem.NewFileTemplate>? NewFileTypes { get; set; }

    /// <summary>The menu entries the "New" submenu should show: what is configured, or the shipped
    /// defaults when nothing ever has been. One place, so the menu, the settings page and the
    /// harness cannot disagree about what is on offer.</summary>
    public IReadOnlyList<BertBrowser.Core.Services.NewItem.NewFileTemplate> ResolvedNewFileTypes =>
        (NewFileTypes ?? BertBrowser.Core.Services.NewItem.NewFileTemplate.Defaults())
            .Where(t => t.Enabled && t.Extension.Length > 0)
            .ToList();

    /// <summary>Active theme: a built-in id ("dark-plus", "light-plus", …) or one of the user's own
    /// themes from <see cref="AppPaths.ThemesDir"/>. Null means the user has never picked one, which
    /// is what lets the first launch honour a Windows high-contrast setting instead of overriding
    /// it — so this stays nullable rather than defaulting to "dark-plus".</summary>
    public string? ThemeId { get; set; }

    /// <summary>Per-token colour tweaks, keyed by theme id and then by token. Kept per theme so
    /// switching away and back doesn't discard the edits made to either one.</summary>
    public Dictionary<string, Dictionary<string, string>> ThemeOverrides { get; set; } = new();

    /// <summary>Whether the rename dialog opens with its options panel already showing.</summary>
    public bool AdvancedRenameExpanded { get; set; }

    /// <summary>
    /// The rename options to bring back next time the panel is opened, or null when it never has
    /// been.
    /// </summary>
    /// <remarks>
    /// <b>The knobs persist; the text does not.</b> Counter start and step, the case transform, the
    /// scope and the two toggles are preferences and come back; <c>Template</c>, <c>Find</c> and
    /// <c>Replace</c> are blanked before this is written, because they are the content of one
    /// rename rather than a setting — a regular expression from last week sitting behind F2 is a
    /// trap, not a convenience.
    ///
    /// <para>The plain box never reads this. It always renames through
    /// <c>RenameRule.Simple</c>, or a persisted <c>Case = Upper</c> would quietly upper-case an
    /// ordinary F2 rename.</para>
    /// </remarks>
    public BertBrowser.Core.Services.Rename.RenameRule? AdvancedRename { get; set; }

    private static string FilePath => AppPaths.SettingsPath;

    public static AppSettings Load()
    {
        try
        {
            if (File.Exists(FilePath))
                return JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(FilePath)) ?? new AppSettings();
        }
        catch (Exception ex) when (ex is IOException or JsonException or UnauthorizedAccessException)
        {
        }
        return new AppSettings();
    }

    public void Save()
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
            File.WriteAllText(FilePath, JsonSerializer.Serialize(this, new JsonSerializerOptions { WriteIndented = true }));
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
        }
    }
}
