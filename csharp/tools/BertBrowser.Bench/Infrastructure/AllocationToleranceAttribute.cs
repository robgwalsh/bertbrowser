namespace BertBrowser.Bench.Infrastructure;

/// <summary>
/// How much allocation jitter a benchmark is allowed before the compare tool calls it a regression.
/// </summary>
/// <remarks>
/// The default is zero: pure code allocates exactly the same bytes every run, and a single extra
/// object is the regression the CI gate exists to catch. Code over SQLite, the thread pool or the
/// file system does not have that property — connection pooling, work-stealing and OS buffering all
/// move the number a little — so those benchmarks declare a tolerance here, and it travels with the
/// result (<see cref="BertBrowser.Core.Benchmarking.AllocStats.TolerancePct"/>) so the compare tool
/// needs only the two files. Put it on a class to cover every benchmark in it, or on one method.
/// </remarks>
[AttributeUsage(AttributeTargets.Class | AttributeTargets.Method, Inherited = true)]
internal sealed class AllocationToleranceAttribute(double percent) : Attribute
{
    public double Percent { get; } = percent;
}
