using BertBrowser.Bench.Corpus;

namespace BertBrowser.Bench.Infrastructure;

/// <summary>
/// What a benchmark class needs to know about the run, read from environment variables in
/// <c>[GlobalSetup]</c>.
/// </summary>
/// <remarks>
/// Environment variables rather than statics, and this is load-bearing: BenchmarkDotNet's default
/// toolchain runs every benchmark in a child process it builds and starts itself, so a static set
/// by the host is simply not there. The host sets these before handing over, children inherit them,
/// and the in-process toolchain reads the same thing.
/// </remarks>
internal sealed record BenchContext(int Scale, string CorpusRoot, string? RealConfigPath)
{
    public const string ScaleVariable = "BERTBROWSER_BENCH_SCALE";
    public const string CorpusVariable = "BERTBROWSER_BENCH_CORPUS";
    public const string RealVariable = "BERTBROWSER_BENCH_REAL";

    private RealConfig? _real;

    public static BenchContext FromEnvironment()
    {
        var scaleText = Environment.GetEnvironmentVariable(ScaleVariable);
        var scale = int.TryParse(scaleText, out var s) && s > 0 ? s : 1;
        var root = Environment.GetEnvironmentVariable(CorpusVariable) is { Length: > 0 } r ? r : BenchCorpus.DefaultRoot;
        var real = Environment.GetEnvironmentVariable(RealVariable) is { Length: > 0 } p ? p : null;
        return new BenchContext(scale, root, real);
    }

    /// <summary>Publishes a context for child processes (and this one) to read.</summary>
    public void Publish()
    {
        Environment.SetEnvironmentVariable(ScaleVariable, Scale.ToString(System.Globalization.CultureInfo.InvariantCulture));
        Environment.SetEnvironmentVariable(CorpusVariable, CorpusRoot);
        Environment.SetEnvironmentVariable(RealVariable, RealConfigPath ?? "");
    }

    public bool IsReal => RealConfigPath is not null;

    /// <summary>The resolved real-data config, or null for a synthetic run.</summary>
    public RealConfig? Real => RealConfigPath is null ? null : _real ??= RealCorpus.Load(RealConfigPath);

    /// <summary>The index to read: the day's backup of the real one, or the synthetic corpus.</summary>
    public string IndexDbPath => Real?.DbCopyPath ?? BenchCorpus.DbPath(CorpusRoot, Scale);

    /// <summary>The folder a scoped search is rooted at.</summary>
    public string SearchRoot => Real?.SearchRoot ?? SyntheticPaths.SearchRoot;

    public TreePaths Tree => BenchCorpus.TreePaths(CorpusRoot, Scale);

    /// <summary>The model the synthetic corpus was cut from, for benchmarks that want rows in memory.</summary>
    public SyntheticPaths Model => SyntheticPaths.Generate(Scale);

    /// <summary>A fresh, empty, migrated database for a write benchmark, under %TEMP%.</summary>
    public static string ScratchDbPath() =>
        Path.Combine(Path.GetTempPath(), "bertbrowser-bench", $"scratch-{Guid.NewGuid():N}.db");

    /// <summary>Deletes a scratch database and its WAL files, letting the pool go first.</summary>
    public static void DeleteDb(string path)
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        var dir = Path.GetDirectoryName(path);
        if (dir is null || !Directory.Exists(dir)) return;
        foreach (var file in Directory.GetFiles(dir, Path.GetFileName(path) + "*"))
        {
            try { File.Delete(file); }
            catch (IOException) { /* a straggling handle; the next run's cleanup gets it */ }
        }
    }
}
