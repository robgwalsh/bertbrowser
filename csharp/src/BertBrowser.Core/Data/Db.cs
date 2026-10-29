using System.Reflection;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Core.Data;

/// <summary>
/// Connection factory and migration runner. Migrations are embedded resources named
/// Data/Migrations/NNN_*.sql, applied in order; user_version tracks the last applied NNN.
/// </summary>
public sealed class Db
{
    private readonly string _connectionString;

    public Db(string databasePath) : this(databasePath, create: true)
    {
    }

    /// <summary>
    /// Opens a database, optionally refusing to bring one into existence.
    /// </summary>
    /// <param name="create">
    /// False for the elevated index helper, and deliberately so. It runs with an administrator
    /// token, and a file an elevated process creates is owned by Administrators — so a helper that
    /// somehow ran before the app would leave a database and a directory the app then has to live
    /// with. The app creates and migrates first; the helper attaches to what is already there and
    /// fails loudly if it is not.
    /// </param>
    public Db(string databasePath, bool create)
    {
        if (create)
            Directory.CreateDirectory(Path.GetDirectoryName(databasePath)!);

        _connectionString = new SqliteConnectionStringBuilder
        {
            DataSource = databasePath,
            Mode = create ? SqliteOpenMode.ReadWriteCreate : SqliteOpenMode.ReadWrite,
            Pooling = true,
        }.ToString();
    }

    public SqliteConnection Open()
    {
        var conn = new SqliteConnection(_connectionString);
        conn.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;";
        cmd.ExecuteNonQuery();
        return conn;
    }

    /// <summary>
    /// The schema this database is actually at, as the last applied migration's number.
    /// </summary>
    /// <remarks>
    /// Read by the elevated index helper, which never migrates. It now outlives the app that
    /// started it, so an app that updates and migrates underneath it would leave it writing rows
    /// against a schema it does not know — it compares this against
    /// <see cref="ExpectedSchemaVersion"/> and exits rather than carrying on.
    /// </remarks>
    public long SchemaVersion
    {
        get
        {
            using var conn = Open();
            using var cmd = conn.CreateCommand();
            cmd.CommandText = "PRAGMA user_version;";
            return (long)cmd.ExecuteScalar()!;
        }
    }

    /// <summary>The schema version this build's migrations produce.</summary>
    public static long ExpectedSchemaVersion { get; } =
        EmbeddedMigrations().Select(m => m.Version).DefaultIfEmpty(0).Max();

    private static IEnumerable<(string Name, long Version)> EmbeddedMigrations()
    {
        var assembly = Assembly.GetExecutingAssembly();
        return assembly.GetManifestResourceNames()
            .Where(n => n.Contains("Migrations") && n.EndsWith(".sql", StringComparison.OrdinalIgnoreCase))
            .Select(n => (Name: n, Version: ParseVersion(n)));
    }

    public void Migrate()
    {
        using var conn = Open();

        long currentVersion;
        using (var cmd = conn.CreateCommand())
        {
            cmd.CommandText = "PRAGMA user_version;";
            currentVersion = (long)cmd.ExecuteScalar()!;
        }

        var migrations = EmbeddedMigrations()
            .Where(m => m.Version > currentVersion)
            .OrderBy(m => m.Version)
            .ToList();

        var assembly = Assembly.GetExecutingAssembly();
        foreach (var (name, version) in migrations)
        {
            using var stream = assembly.GetManifestResourceStream(name)!;
            using var reader = new StreamReader(stream);
            var sql = reader.ReadToEnd();

            using var tx = conn.BeginTransaction();
            using var cmd = conn.CreateCommand();
            cmd.Transaction = tx;
            cmd.CommandText = sql;
            cmd.ExecuteNonQuery();
            cmd.CommandText = $"PRAGMA user_version = {version};";
            cmd.ExecuteNonQuery();
            tx.Commit();
        }
    }

    private static long ParseVersion(string resourceName)
    {
        // Resource names look like "BertBrowser.Core.Data.Migrations._001_initial.sql"
        // (a leading underscore is added when the filename starts with a digit).
        var parts = resourceName.Split('.');
        var fileBase = parts[^2]; // segment before the "sql" extension
        var digits = new string(fileBase.SkipWhile(c => !char.IsDigit(c)).TakeWhile(char.IsDigit).ToArray());
        if (digits.Length == 0)
            throw new InvalidOperationException($"Cannot parse migration version from '{resourceName}'.");
        return long.Parse(digits);
    }
}
