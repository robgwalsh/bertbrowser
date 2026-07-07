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

    public List<CustomCommandDefinition> CustomCommands { get; set; } = new();

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
