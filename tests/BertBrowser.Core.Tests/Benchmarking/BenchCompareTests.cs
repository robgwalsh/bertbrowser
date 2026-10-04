using BertBrowser.Core.Benchmarking;
using Xunit;

namespace BertBrowser.Core.Tests.Benchmarking;

public class BenchCompareTests
{
    private static readonly CompareOptions Default = new();

    [Fact]
    public void SameNumbers_Same()
    {
        var report = Compare([Entry("a", 1000, 512)], [Entry("a", 1000, 512)]);
        var row = Assert.Single(report.Rows);
        Assert.Equal(CompareStatus.Same, row.Status);
        Assert.False(report.TimeRegressed);
        Assert.False(report.AllocRegressed);
    }

    [Theory]
    [InlineData(1_000_000, 1_150_000, false)] // exactly at the threshold is not over it
    [InlineData(1_000_000, 1_150_001, true)]
    [InlineData(1_000_000, 1_300_000, true)]
    public void Slower_WhenMedianExceedsThreshold(double before, double after, bool slower)
    {
        var report = Compare([Entry("a", before, 0)], [Entry("a", after, 0)]);
        Assert.Equal(slower ? CompareStatus.Slower : CompareStatus.Same, report.Rows[0].Status);
        Assert.Equal(slower, report.TimeRegressed);
    }

    [Fact]
    public void Slower_SuppressedBelowNoiseFloor()
    {
        // 2 ns to 3 ns is +50% and 1 ns — well under the 200 ns floor.
        var report = Compare([Entry("a", 2, 0)], [Entry("a", 3, 0)]);
        Assert.Equal(CompareStatus.Same, report.Rows[0].Status);
        Assert.False(report.TimeRegressed);
    }

    [Fact]
    public void NoiseFloor_IsPerUnit()
    {
        // 10 ms to 14 ms is +40% and 4 ms — under the 5 ms floor for milliseconds.
        var report = Compare([Entry("ui", 10, null, "ms", "B")], [Entry("ui", 14, null, "ms", "B")]);
        Assert.Equal(CompareStatus.Same, report.Rows[0].Status);

        // 100 ms to 140 ms clears both.
        report = Compare([Entry("ui", 100, null, "ms", "B")], [Entry("ui", 140, null, "ms", "B")]);
        Assert.Equal(CompareStatus.Slower, report.Rows[0].Status);
    }

    [Fact]
    public void Faster_IsSymmetric()
    {
        var report = Compare([Entry("a", 1_000_000, 0)], [Entry("a", 700_000, 0)]);
        Assert.Equal(CompareStatus.Faster, report.Rows[0].Status);
        Assert.False(report.TimeRegressed);
    }

    [Fact]
    public void TimeGateOff_StillReportsSlower()
    {
        var report = Compare([Entry("a", 1_000_000, 0)], [Entry("a", 2_000_000, 0)], Default with { GateTime = false });
        Assert.Equal(CompareStatus.Slower, report.Rows[0].Status);
        Assert.False(report.TimeRegressed);
    }

    [Theory]
    [InlineData(1000, 1000, 0, false)]
    [InlineData(1000, 1001, 0, true)]   // exact: one byte more is a regression
    [InlineData(1000, 1020, 2, false)]  // within the entry's 2% tolerance
    [InlineData(1000, 1021, 2, true)]
    public void AllocUp_HonoursEntryTolerance(long before, long after, double tolerance, bool up)
    {
        var report = Compare([Entry("a", 1000, before)], [Entry("a", 1000, after, tolerance: tolerance)]);
        Assert.Equal(up ? CompareStatus.AllocUp : CompareStatus.Same, report.Rows[0].Status);
        Assert.Equal(up, report.AllocRegressed);
    }

    [Fact]
    public void AllocTolerance_OverrideReplacesEntry()
    {
        var baseline = Entry("a", 1000, 1000);
        var current = Entry("a", 1000, 1050, tolerance: 0);

        Assert.Equal(CompareStatus.AllocUp, Compare([baseline], [current]).Rows[0].Status);
        Assert.Equal(CompareStatus.Same,
            Compare([baseline], [current], Default with { AllocTolerancePctOverride = 10 }).Rows[0].Status);
    }

    [Fact]
    public void AllocFromZero_AnyAllocationIsUp()
    {
        var report = Compare([Entry("a", 1000, 0)], [Entry("a", 1000, 24, tolerance: 50)]);
        Assert.Equal(CompareStatus.AllocUp, report.Rows[0].Status);
    }

    [Fact]
    public void AllocUp_OutranksSlower()
    {
        var report = Compare([Entry("a", 1_000_000, 100)], [Entry("a", 2_000_000, 200)]);
        Assert.Equal(CompareStatus.AllocUp, report.Rows[0].Status);
        Assert.True(report.TimeRegressed);
        Assert.True(report.AllocRegressed);
    }

    [Fact]
    public void AllocDown_ReportedButNotARegression()
    {
        var report = Compare([Entry("a", 1000, 1000)], [Entry("a", 1000, 500)]);
        Assert.Equal(CompareStatus.AllocDown, report.Rows[0].Status);
        Assert.False(report.AllocRegressed);
    }

    [Fact]
    public void New_Removed_Errored()
    {
        var report = Compare(
            [Entry("kept", 1000, 0), Entry("gone", 1000, 0)],
            [Entry("kept", 1000, 0), Entry("fresh", 1000, 0), Failed("broken")]);

        Assert.Equal(CompareStatus.New, report.Rows.Single(r => r.Id == "fresh").Status);
        Assert.Equal(CompareStatus.Removed, report.Rows.Single(r => r.Id == "gone").Status);
        Assert.Equal(CompareStatus.Errored, report.Rows.Single(r => r.Id == "broken").Status);
        Assert.True(report.HasErrors);
        Assert.True(report.HasRemoved);
        Assert.False(report.TimeRegressed);
        Assert.False(report.AllocRegressed);
    }

    [Fact]
    public void RealAgainstSynthetic_Refused()
    {
        var synthetic = Set([Entry("a", 1, 0)], real: false);
        var real = Set([Entry("a", 1, 0)], real: true);
        Assert.Throws<InvalidOperationException>(() => BenchCompare.Compare(synthetic, real, Default));
        Assert.Throws<InvalidOperationException>(() => BenchCompare.Compare(real, synthetic, Default));
    }

    [Fact]
    public void TimeStats_From_MedianAndNearestRankP95()
    {
        var stats = TimeStats.From([5, 1, 4, 2, 3], "ms");
        Assert.Equal(3, stats.Median);
        Assert.Equal(5, stats.P95);
        Assert.Equal(1, stats.Min);
        Assert.Equal(5, stats.Max);
        Assert.Equal(3, stats.Mean);

        var even = TimeStats.From([1, 2, 3, 4], "ms");
        Assert.Equal(2.5, even.Median);

        var many = TimeStats.From(Enumerable.Range(1, 100).Select(i => (double)i).ToArray(), "ns");
        Assert.Equal(95, many.P95);
    }

    [Fact]
    public void TimeStats_From_Empty_Throws() =>
        Assert.Throws<ArgumentException>(() => TimeStats.From([], "ms"));

    // ---- helpers ----

    private static CompareReport Compare(BenchEntry[] baseline, BenchEntry[] current, CompareOptions? options = null) =>
        BenchCompare.Compare(Set(baseline, false), Set(current, false), options ?? Default);

    internal static BenchResultSet Set(IReadOnlyList<BenchEntry> entries, bool real, string machineKey = "test-machine") =>
        new(BenchResultSet.CurrentSchemaVersion, new DateTime(2026, 10, 2, 12, 0, 0, DateTimeKind.Utc), "test",
            new GitInfo("0123456789abcdef", "main", false),
            new MachineInfo(machineKey, "Test CPU", 8, 16, 26200, "10.0.2", "Balanced", false, false),
            new RunInfo("default", 1, real, ["A"], null, null),
            entries);

    internal static BenchEntry Entry(string id, double median, long? alloc, string unit = "ns", string tier = "A", double tolerance = 0) =>
        new(id, tier, new Dictionary<string, string>(), 15,
            new TimeStats(unit, median, median, median * 1.05, median * 0.02, median * 0.98, median * 1.1),
            alloc is { } a ? new AllocStats(a, 1.0, 0, 0, tolerance) : null,
            null, null);

    private static BenchEntry Failed(string id) =>
        new(id, "A", new Dictionary<string, string>(), 0, null, null, null, "threw NullReferenceException");
}
