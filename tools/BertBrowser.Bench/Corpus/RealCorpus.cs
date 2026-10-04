using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Bench.Corpus;

/// <summary>What <c>bench/real.local.json</c> names, plus where the day's backup of the index went.</summary>
internal sealed record RealConfig(
    string DbPath,
    string SearchRoot,
    string ListingFolder,
    string TreeFolder,
    string TextFolder,
    string MediaFolder)
{
    /// <summary>Set by the host once the backup exists; what the benchmarks actually open.</summary>
    public string? DbCopyPath { get; init; }
}

/// <summary>
/// The opt-in run against the user's own index and folders.
/// </summary>
/// <remarks>
/// <b>The live database is never opened for a benchmark.</b> The index helper tails the USN journal
/// into it the whole time, which is both noise and contention, and a read-only open of a WAL database
/// still needs its <c>-shm</c>. So the host takes one SQLite backup a day into the bench cache and
/// the benchmarks read that. The folders are read-only by nature of what runs against them.
/// </remarks>
internal static class RealCorpus
{
    private static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    public static RealConfig Load(string path)
    {
        var config = JsonSerializer.Deserialize<RealConfig>(File.ReadAllText(path), Json)
            ?? throw new InvalidDataException($"{path} is empty.");

        foreach (var (name, value) in new[]
        {
            ("dbPath", config.DbPath), ("searchRoot", config.SearchRoot), ("listingFolder", config.ListingFolder),
            ("treeFolder", config.TreeFolder), ("textFolder", config.TextFolder), ("mediaFolder", config.MediaFolder),
        })
        {
            if (string.IsNullOrWhiteSpace(value))
                throw new InvalidDataException($"{path}: '{name}' is missing. See bench/README.md for the shape.");
        }

        if (!File.Exists(config.DbPath)) throw new FileNotFoundException($"{path}: no database at {config.DbPath}.");
        foreach (var folder in new[] { config.SearchRoot, config.ListingFolder, config.TreeFolder, config.TextFolder, config.MediaFolder })
        {
            if (!Directory.Exists(folder)) throw new DirectoryNotFoundException($"{path}: no folder at {folder}.");
        }

        return config;
    }

    /// <summary>Today's backup of the live index, made if there is not one yet.</summary>
    public static string EnsureBackup(RealConfig config, string cacheRoot, Action<string> log, bool refresh)
    {
        var dir = Path.Combine(cacheRoot, "real");
        Directory.CreateDirectory(dir);
        var copy = Path.Combine(dir, $"bertbrowser-{DateTime.Now.ToString("yyyyMMdd", CultureInfo.InvariantCulture)}.db");

        if (File.Exists(copy) && !refresh)
        {
            log($"# real: using today's backup {copy}");
            return copy;
        }

        log($"# real: backing up {config.DbPath} → {copy}");
        var clock = System.Diagnostics.Stopwatch.StartNew();

        foreach (var suffix in new[] { "", "-wal", "-shm" })
            if (File.Exists(copy + suffix)) File.Delete(copy + suffix);

        using (var source = new SqliteConnection(new SqliteConnectionStringBuilder
               {
                   DataSource = config.DbPath, Mode = SqliteOpenMode.ReadOnly, Pooling = false,
               }.ToString()))
        using (var destination = new SqliteConnection(new SqliteConnectionStringBuilder
               {
                   DataSource = copy, Mode = SqliteOpenMode.ReadWriteCreate, Pooling = false,
               }.ToString()))
        {
            source.Open();
            destination.Open();
            source.BackupDatabase(destination);
        }

        log($"# real: {new FileInfo(copy).Length / (1024 * 1024)} MB in {clock.Elapsed.TotalSeconds:0.0} s");
        return copy;
    }

    /// <summary>Writes the config with the copy path filled in, for the benchmark processes to read.</summary>
    public static string WriteResolved(RealConfig config, string cacheRoot)
    {
        var path = Path.Combine(cacheRoot, "real", "resolved.json");
        File.WriteAllText(path, JsonSerializer.Serialize(config, Json));
        return path;
    }
}
