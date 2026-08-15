namespace BertBrowser.App.Services;

/// <summary>
/// App data lives in ~/.bertbrowser. It must NOT live in %LOCALAPPDATA%\BertBrowser,
/// because Velopack installs the app there (packId = BertBrowser) and uninstall
/// deletes that directory.
/// </summary>
public static class AppPaths
{
    /// <summary>
    /// Environment variable that moves the whole data directory somewhere else.
    /// </summary>
    /// <remarks>
    /// Read once, before anything opens the database, so a harness run cannot inherit the user's
    /// index, settings or themes — nor write into them. Nothing in the shipped app sets it; it
    /// exists so <c>tools/BertBrowser.Harness</c> can host the real window against a scratch
    /// directory it deletes afterwards.
    /// </remarks>
    public const string OverrideVariable = "BERTBROWSER_DATA_DIR";

    public static string DataDir { get; } =
        Environment.GetEnvironmentVariable(OverrideVariable) is { Length: > 0 } overridden
            ? Path.GetFullPath(overridden)
            : Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                ".bertbrowser");

    public static string DbPath => Path.Combine(DataDir, "bertbrowser.db");
    public static string SettingsPath => Path.Combine(DataDir, "settings.json");

    /// <summary>Where the user's own <c>*.json</c> themes live. Created on demand.</summary>
    public static string ThemesDir => Path.Combine(DataDir, "themes");

    /// <summary>
    /// One-time move of data from the pre-1.0 location (%LOCALAPPDATA%\BertBrowser)
    /// to ~/.bertbrowser. Runs before the DB is opened; no-op once DataDir exists.
    /// </summary>
    public static void MigrateLegacyData()
    {
        if (Directory.Exists(DataDir))
            return;

        var legacyDir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "BertBrowser");

        Directory.CreateDirectory(DataDir);
        if (!Directory.Exists(legacyDir))
            return;

        string[] files = ["bertbrowser.db", "bertbrowser.db-wal", "bertbrowser.db-shm", "settings.json"];
        foreach (var name in files)
        {
            var source = Path.Combine(legacyDir, name);
            var target = Path.Combine(DataDir, name);
            try
            {
                if (File.Exists(source) && !File.Exists(target))
                    File.Move(source, target);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                // Leave the legacy file in place; the app starts fresh rather than failing.
            }
        }
    }
}
