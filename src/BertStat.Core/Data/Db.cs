using System.Reflection;
using Microsoft.Data.Sqlite;

namespace BertStat.Core.Data;

/// <summary>
/// Connection factory and migration runner. Migrations are embedded resources named
/// Data/Migrations/NNN_*.sql, applied in order; user_version tracks the last applied NNN.
/// </summary>
public sealed class Db
{
    private readonly string _connectionString;

    public Db(string databasePath)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(databasePath)!);
        _connectionString = new SqliteConnectionStringBuilder
        {
            DataSource = databasePath,
            Mode = SqliteOpenMode.ReadWriteCreate,
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

    public void Migrate()
    {
        using var conn = Open();

        long currentVersion;
        using (var cmd = conn.CreateCommand())
        {
            cmd.CommandText = "PRAGMA user_version;";
            currentVersion = (long)cmd.ExecuteScalar()!;
        }

        var assembly = Assembly.GetExecutingAssembly();
        var migrations = assembly.GetManifestResourceNames()
            .Where(n => n.Contains("Migrations") && n.EndsWith(".sql", StringComparison.OrdinalIgnoreCase))
            .Select(n => (Name: n, Version: ParseVersion(n)))
            .Where(m => m.Version > currentVersion)
            .OrderBy(m => m.Version)
            .ToList();

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
        // Resource names look like "BertStat.Core.Data.Migrations._001_initial.sql"
        // (a leading underscore is added when the filename starts with a digit).
        var parts = resourceName.Split('.');
        var fileBase = parts[^2]; // segment before the "sql" extension
        var digits = new string(fileBase.SkipWhile(c => !char.IsDigit(c)).TakeWhile(char.IsDigit).ToArray());
        if (digits.Length == 0)
            throw new InvalidOperationException($"Cannot parse migration version from '{resourceName}'.");
        return long.Parse(digits);
    }
}
