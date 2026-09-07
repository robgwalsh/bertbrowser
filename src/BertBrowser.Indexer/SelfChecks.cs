using BertBrowser.Core.Data;

namespace BertBrowser.Indexer;

/// <summary>
/// The reasons an unsupervised helper stops on its own.
/// </summary>
/// <remarks>
/// <para>
/// This process used to end when the app that launched it did, which needed no policy at all. Now
/// it outlives every app, so it has to notice for itself when it has been left behind — and it has
/// to notice <em>while idle</em>, because the dangerous case is precisely a helper with no app
/// connected. That is why this is a timer and not a check between sessions.
/// </para>
/// <para>
/// All three conditions are the same shape: something about the installation this helper belongs to
/// has changed underneath it, and carrying on would mean an elevated process of an old build
/// writing to a database of a new one.
/// </para>
/// </remarks>
internal static class SelfChecks
{
    public static IDisposable Start(Db db, CancellationTokenSource lifetime)
    {
        var schemaAtStart = TryReadSchema(db);
        var ownPath = Environment.ProcessPath;
        var ownStamp = LastWriteOf(ownPath);
        var appPath = Path.Combine(AppContext.BaseDirectory, "BertBrowser.exe");

        return new Timer(_ =>
        {
            try
            {
                if (ShouldStop(db, schemaAtStart, ownPath, ownStamp, appPath))
                    lifetime.Cancel();
            }
            catch (ObjectDisposedException)
            {
                // Already shutting down.
            }
        }, null, Interval, Interval);
    }

    private static readonly TimeSpan Interval = TimeSpan.FromSeconds(30);

    private static bool ShouldStop(Db db, long? schemaAtStart, string? ownPath, DateTime? ownStamp, string appPath)
    {
        // The app updated and migrated while this helper was idle. App.OnStartup migrates before it
        // ever looks for an index helper, so there is a real window — and with nothing connected,
        // an unbounded one. Only a reading that actually came back counts: a busy or locked
        // database is not a reason to stop indexing.
        if (schemaAtStart is { } before && TryReadSchema(db) is { } now && now != before) return true;

        // Velopack replaces the install directory wholesale; an uninstall removes it.
        if (ownPath is not null && (!File.Exists(ownPath) || LastWriteOf(ownPath) != ownStamp)) return true;

        return !File.Exists(appPath);
    }

    /// <summary>The schema, or null when the database could not be read just now.</summary>
    private static long? TryReadSchema(Db db)
    {
        try
        {
            return db.SchemaVersion;
        }
        catch (Exception ex) when (ex is Microsoft.Data.Sqlite.SqliteException or IOException)
        {
            return null;
        }
    }

    private static DateTime? LastWriteOf(string? path)
    {
        if (path is null) return null;

        try
        {
            return File.Exists(path) ? File.GetLastWriteTimeUtc(path) : null;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return null;
        }
    }
}
