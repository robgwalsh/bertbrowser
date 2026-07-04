namespace BertBrowser.App.Services;

/// <summary>
/// App data lives in ~/.bertbrowser. It must NOT live in %LOCALAPPDATA%\BertBrowser,
/// because Velopack installs the app there (packId = BertBrowser) and uninstall
/// deletes that directory.
/// </summary>
public static class AppPaths
{
    public static string DataDir { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        ".bertbrowser");

    public static string DbPath => Path.Combine(DataDir, "bertbrowser.db");
    public static string SettingsPath => Path.Combine(DataDir, "settings.json");

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
