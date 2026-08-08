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

    /// <summary>When false (default), hidden files/folders are excluded from listings and
    /// search; when true they appear with a dimmed icon, like Explorer.</summary>
    public bool ShowHiddenItems { get; set; }

    /// <summary>Per-directory thumbnail-zoom slider position (0..1), keyed by canonical path.
    /// 0 (or absent) = details list. Only folders explicitly zoomed are stored.</summary>
    public Dictionary<string, double> DirectoryThumbnailScales { get; set; } = new();

    /// <summary>Mouse-wheel vertical scroll speed multiplier (1 = OS default). Defaults to 2×.</summary>
    public double ScrollSpeedMultiplier { get; set; } = 2.0;

    public List<CustomCommandDefinition> CustomCommands { get; set; } = new();

    /// <summary>Active theme: a built-in id ("dark-plus", "light-plus", …) or one of the user's own
    /// themes from <see cref="AppPaths.ThemesDir"/>. Null means the user has never picked one, which
    /// is what lets the first launch honour a Windows high-contrast setting instead of overriding
    /// it — so this stays nullable rather than defaulting to "dark-plus".</summary>
    public string? ThemeId { get; set; }

    /// <summary>Per-token colour tweaks, keyed by theme id and then by token. Kept per theme so
    /// switching away and back doesn't discard the edits made to either one.</summary>
    public Dictionary<string, Dictionary<string, string>> ThemeOverrides { get; set; } = new();

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
