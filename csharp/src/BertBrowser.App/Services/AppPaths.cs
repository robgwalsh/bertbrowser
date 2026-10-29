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

    /// <summary>Where template files for the "New" menu live — both the ones an import writes out
    /// of the registry and any the user points at themselves. Created on demand. Under
    /// <see cref="DataDir"/> so <see cref="OverrideVariable"/> redirects it with everything else,
    /// and a harness run cannot write into the user's.</summary>
    public static string TemplatesDir => Path.Combine(DataDir, "templates");
}
