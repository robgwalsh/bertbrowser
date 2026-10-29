using BenchmarkDotNet.Columns;
using BenchmarkDotNet.Configs;
using BenchmarkDotNet.Diagnosers;
using BenchmarkDotNet.Jobs;
using BenchmarkDotNet.Toolchains.InProcess.Emit;

namespace BertBrowser.Bench.Infrastructure;

/// <summary>How BenchmarkDotNet is told to run.</summary>
internal static class BenchConfig
{
    /// <param name="job"><c>default</c> or <c>short</c> (3 warmup, 3 measured iterations — CI's choice,
    /// since only allocations are gated there and those do not need many samples).</param>
    /// <param name="toolchain"><c>default</c> builds and starts a child process per benchmark class,
    /// the best time isolation; <c>inprocess</c> runs in this process, which CI uses because it needs
    /// no generated-project build and allocation counts are identical either way.</param>
    /// <param name="artifactsPath">Where BenchmarkDotNet's logs and exporters write — outside the
    /// repository, so a run leaves nothing to tidy. Note that the default toolchain's <em>generated
    /// projects</em> do not go here: BDN places them beside the benchmark assembly, under this
    /// project's <c>bin\</c>, where they inherit <c>Directory.Build.props</c>. Measured: they build clean
    /// under <c>TreatWarningsAsErrors</c>, and <c>bin\</c> is gitignored, so nothing is done about it.</param>
    public static IConfig Create(string job, string toolchain, string artifactsPath)
    {
        var j = job switch
        {
            "short" => Job.ShortRun,
            "default" => Job.Default,
            _ => throw new UsageJobException(job),
        };
        j = j.WithId(job);

        j = toolchain switch
        {
            "inprocess" => j.WithToolchain(InProcessEmitToolchain.Instance),
            "default" => j,
            _ => throw new UsageJobException(toolchain),
        };

        // The default config keeps its validators — including the one that refuses a Debug build —
        // and its exporters, which land in the artifacts path as a debugging trail.
        return ManualConfig.Create(DefaultConfig.Instance)
            .AddJob(j)
            .AddDiagnoser(MemoryDiagnoser.Default)
            .AddColumn(StatisticColumn.Median, StatisticColumn.P95)
            .WithArtifactsPath(artifactsPath)
            .WithOptions(ConfigOptions.DisableLogFile);
    }

    /// <summary>Surfaced by <c>RunCommand</c> as a usage error, since the config is built after parsing.</summary>
    public sealed class UsageJobException(string value) : Exception($"Unknown job or toolchain '{value}'.");
}
