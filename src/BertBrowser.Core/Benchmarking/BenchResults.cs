namespace BertBrowser.Core.Benchmarking;

/// <summary>
/// One run's worth of benchmark results — the schema every tier writes and the compare tool reads.
/// </summary>
/// <remarks>
/// <para>
/// Three very different measurers feed this: BenchmarkDotNet over Core (nanoseconds, allocations),
/// the UI harness over whole scenarios (milliseconds, process memory), and timed launches of the
/// real executable (milliseconds since process start). Keeping them in one shape is what lets one
/// baseline file hold a machine's whole picture and one <see cref="BenchCompare"/> judge all of it —
/// but the shape is deliberately not clever: a tier fills the fields it can answer and leaves the
/// rest null, and <see cref="TimeStats.Unit"/> says which clock a number is in.
/// </para>
/// <para>
/// Pure data, in Core rather than in the benchmark tool, because the app and the harness also have
/// to emit pieces of it and neither may reference the tool. Everything here is covered by
/// <c>BertBrowser.Core.Tests</c>.
/// </para>
/// </remarks>
public sealed record BenchResultSet(
    int SchemaVersion,
    DateTime GeneratedUtc,
    string Tool,
    GitInfo Git,
    MachineInfo Machine,
    RunInfo Run,
    IReadOnlyList<BenchEntry> Benchmarks)
{
    /// <summary>The schema this build writes and reads. Bump it when a field changes meaning.</summary>
    public const int CurrentSchemaVersion = 1;
}

/// <summary>Which commit produced a result, and whether the tree had uncommitted changes.</summary>
public sealed record GitInfo(string Sha, string Branch, bool Dirty);

/// <summary>
/// The machine a result came from. <see cref="Key"/> is what baselines are filed under; the rest is
/// the legend a reader needs to interpret the numbers.
/// </summary>
/// <param name="PowerPlan">The active Windows power plan, when it could be read. Noise, not identity.</param>
/// <param name="AppRunning">Whether a real <c>BertBrowser</c> was running during the run.</param>
/// <param name="IndexerRunning">Whether the index helper was running — it tails the USN journal into
/// the user's database the whole time, which is disk and CPU noise worth knowing about.</param>
public sealed record MachineInfo(
    string Key,
    string Cpu,
    int LogicalCores,
    int RamGb,
    int OsBuild,
    string Runtime,
    string? PowerPlan,
    bool AppRunning,
    bool IndexerRunning);

/// <summary>How a run was configured.</summary>
/// <param name="Job">BenchmarkDotNet's job name for Tier A (<c>default</c> or <c>short</c>); a tier's own word otherwise.</param>
/// <param name="Scale">The synthetic corpus multiplier: rows are <c>Scale × 100,000</c>.</param>
/// <param name="Real">True when the run read the user's real index and folders. A real file is only
/// ever compared against a real baseline — the ids coincide and the corpora do not.</param>
/// <param name="Tiers">Which tiers contributed: <c>A</c>, <c>B</c>, <c>C</c>.</param>
/// <param name="RenderMode">Tier B only: the WPF render mode the harness ran under.</param>
/// <param name="Repeat">Tiers B and C: how many times each scenario or launch was run.</param>
/// <param name="Toolchain">Tier A only: <c>default</c> (a child process per benchmark class) or
/// <c>inprocess</c>. Recorded because the two do <em>not</em> report the same allocations for code over
/// SQLite or the hashing APIs — a comparison across toolchains shows "regressions" that are only the
/// toolchain. Null in a file written before this was recorded.</param>
public sealed record RunInfo(
    string Job,
    int Scale,
    bool Real,
    IReadOnlyList<string> Tiers,
    string? RenderMode,
    int? Repeat,
    string? Toolchain = null);

/// <summary>One benchmark's result.</summary>
/// <param name="Id">Stable across runs; the key a comparison joins on. <c>core.*</c>, <c>ui.*</c>, <c>startup.*</c>.</param>
/// <param name="Tier"><c>A</c>, <c>B</c> or <c>C</c>.</param>
/// <param name="Parameters">Whatever sized the work — a query, a file count, a scale — so a number is never read without its denominator.</param>
/// <param name="N">How many measurements the statistics summarise.</param>
/// <param name="Time">Null when the benchmark failed.</param>
/// <param name="Memory">Tier A only.</param>
/// <param name="Process">Tier B only.</param>
/// <param name="Error">Non-null when the benchmark did not produce a result. Any error fails a CI comparison.</param>
public sealed record BenchEntry(
    string Id,
    string Tier,
    IReadOnlyDictionary<string, string> Parameters,
    int N,
    TimeStats? Time,
    AllocStats? Memory,
    ProcessStats? Process,
    string? Error);

/// <summary>Per-operation timing. <see cref="Unit"/> is <c>ns</c> for Tier A and <c>ms</c> otherwise.</summary>
public sealed record TimeStats(
    string Unit,
    double Mean,
    double Median,
    double P95,
    double StdDev,
    double Min,
    double Max)
{
    /// <summary>
    /// Summarises raw samples. The median is the midpoint (mean of the two middle values for an even
    /// count) and p95 is nearest-rank, so with five samples it is simply the largest — honest about
    /// how little five samples can say.
    /// </summary>
    public static TimeStats From(IReadOnlyList<double> samples, string unit)
    {
        if (samples.Count == 0) throw new ArgumentException("At least one sample is needed.", nameof(samples));

        var sorted = samples.Order().ToArray();
        var n = sorted.Length;
        var mean = sorted.Average();
        var median = n % 2 == 1 ? sorted[n / 2] : (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0;
        var p95Rank = (int)Math.Ceiling(0.95 * n);
        var p95 = sorted[Math.Clamp(p95Rank - 1, 0, n - 1)];
        var stdDev = n < 2 ? 0 : Math.Sqrt(sorted.Sum(s => (s - mean) * (s - mean)) / (n - 1));

        return new TimeStats(unit, mean, median, p95, stdDev, sorted[0], sorted[^1]);
    }
}

/// <summary>
/// What one operation allocates. <see cref="TolerancePct"/> travels with the result so the compare
/// tool needs only two files to decide — it is the benchmark author's statement of how much
/// allocation jitter the measured code has (SQLite, the thread pool), and is zero for pure code.
/// </summary>
public sealed record AllocStats(
    long AllocatedBytes,
    double Gen0PerKOps,
    double Gen1PerKOps,
    double Gen2PerKOps,
    double TolerancePct);

/// <summary>A snapshot of the harness process after a scenario, taken after a full collection.</summary>
public sealed record ProcessStats(
    long WorkingSetBytes,
    long PrivateBytes,
    long ManagedHeapBytes,
    int RealizedRows,
    int Items,
    int RetainedThumbnails);
