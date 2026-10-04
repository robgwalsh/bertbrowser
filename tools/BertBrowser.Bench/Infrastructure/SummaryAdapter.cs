using System.Reflection;
using BenchmarkDotNet.Reports;
using BertBrowser.Core.Benchmarking;

namespace BertBrowser.Bench.Infrastructure;

/// <summary>
/// The one place Tier A meets the shared schema: BenchmarkDotNet's <see cref="Summary"/> in,
/// <see cref="BenchEntry"/> rows out.
/// </summary>
internal static class SummaryAdapter
{
    public static IReadOnlyList<BenchEntry> ToEntries(IEnumerable<Summary> summaries, int scale)
    {
        var entries = new List<BenchEntry>();

        foreach (var summary in summaries)
        {
            foreach (var report in summary.Reports)
            {
                var benchmarkCase = report.BenchmarkCase;
                var id = BenchId.For(benchmarkCase);
                var parameters = BenchId.Parameters(benchmarkCase, scale);

                var stats = report.ResultStatistics;
                if (!report.Success || stats is null || stats.N == 0)
                {
                    var errors = report.ExecuteResults
                        .SelectMany(r => r.Errors)
                        .Where(e => !string.IsNullOrWhiteSpace(e))
                        .Distinct()
                        .ToList();
                    var message = errors.Count > 0 ? string.Join(" | ", errors) : "the benchmark produced no measurements";
                    entries.Add(new BenchEntry(id, "A", parameters, 0, null, null, null, message));
                    continue;
                }

                var time = new TimeStats("ns",
                    stats.Mean, stats.Median, stats.Percentiles.P95, stats.StandardDeviation, stats.Min, stats.Max);

                var gc = report.GcStats;
                var allocated = gc.GetBytesAllocatedPerOperation(benchmarkCase) ?? 0;
                var perKOps = gc.TotalOperations > 0 ? 1000.0 / gc.TotalOperations : 0;
                var memory = new AllocStats(
                    allocated,
                    gc.Gen0Collections * perKOps,
                    gc.Gen1Collections * perKOps,
                    gc.Gen2Collections * perKOps,
                    Tolerance(benchmarkCase.Descriptor.WorkloadMethod, benchmarkCase.Descriptor.Type));

                entries.Add(new BenchEntry(id, "A", parameters, stats.N, time, memory, null, null));
            }
        }

        return entries;
    }

    /// <summary>The method's declared tolerance, else its class's, else exact.</summary>
    private static double Tolerance(MethodInfo method, Type type) =>
        method.GetCustomAttribute<AllocationToleranceAttribute>()?.Percent
        ?? type.GetCustomAttribute<AllocationToleranceAttribute>()?.Percent
        ?? 0;
}
