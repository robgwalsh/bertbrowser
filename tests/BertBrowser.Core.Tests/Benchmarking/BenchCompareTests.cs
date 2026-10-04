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

    [Theory]
    [InlineData(13.0, 13.9, CompareStatus.Same)]       // +0.9 MB: under the 1 MB floor, whatever the percentage
    [InlineData(13.0, 14.2, CompareStatus.Same)]       // +1.2 MB clears the floor but is 9.2%
    [InlineData(13.0, 14.5, CompareStatus.MemoryUp)]   // +1.5 MB and 11.5%
    [InlineData(13.0, 120.0, CompareStatus.MemoryUp)]  // the fifteen-tab leak this gate was written after
    [InlineData(20.0, 17.0, CompareStatus.MemoryDown)]
    public void ManagedHeap_JudgedAtTenPercentOverAMegabyte(double before, double after, CompareStatus expected)
    {
        var report = Compare([Snapshot("m", 100, before)], [Snapshot("m", 100, after)]);
        Assert.Equal(expected, report.Rows[0].Status);
        Assert.Equal(expected == CompareStatus.MemoryUp, report.MemoryRegressed);
        Assert.False(report.TimeRegressed);
        Assert.False(report.AllocRegressed);
    }

    [Theory]
    [InlineData(100, 112, CompareStatus.Same)]      // the spread measured between identical runs
    [InlineData(100, 124, CompareStatus.Same)]      // over the 16 MB floor, under 25%
    [InlineData(100, 130, CompareStatus.MemoryUp)]
    [InlineData(40, 55, CompareStatus.Same)]        // +37%, but 15 MB is under the floor
    [InlineData(100, 70, CompareStatus.MemoryDown)]
    public void PrivateBytes_JudgedLooserThanTheManagedHeap(double before, double after, CompareStatus expected)
    {
        var report = Compare([Snapshot("m", before, 13)], [Snapshot("m", after, 13)]);
        Assert.Equal(expected, report.Rows[0].Status);
        Assert.Equal(expected == CompareStatus.MemoryUp, report.MemoryRegressed);
    }

    [Fact]
    public void WorkingSet_IsReportedAndNeverJudged()
    {
        var report = Compare([Snapshot("m", 100, 13, workingSetMb: 150)], [Snapshot("m", 100, 13, workingSetMb: 600)]);
        Assert.Equal(CompareStatus.Same, report.Rows[0].Status);
        Assert.False(report.MemoryRegressed);
        Assert.Equal(600L * 1024 * 1024, report.Rows[0].CurrentProcess!.WorkingSetBytes);
    }

    [Fact]
    public void MemoryGateOff_StillReportsMemoryUp()
    {
        var report = Compare([Snapshot("m", 100, 13)], [Snapshot("m", 100, 30)], Default with { GateMemory = false });
        Assert.Equal(CompareStatus.MemoryUp, report.Rows[0].Status);
        Assert.False(report.MemoryRegressed);
    }

    [Fact]
    public void MemoryUp_WinsOverAShrinkingOtherNumber()
    {
        // Private bytes fell and the managed heap rose: the rise is the news.
        var report = Compare([Snapshot("m", 200, 13)], [Snapshot("m", 100, 30)]);
        Assert.Equal(CompareStatus.MemoryUp, report.Rows[0].Status);
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

    /// <summary>A process snapshot: the working set is twice the private bytes and the peak 10 MB over that.</summary>
    internal static BenchEntry Snapshot(string id, double privateMb, double managedMb, string tier = "A", double? workingSetMb = null)
    {
        const double mb = 1024 * 1024;
        var workingSet = workingSetMb ?? privateMb * 2;
        return new(id, tier, new Dictionary<string, string>(), 5, null, null,
            new ProcessStats((long)(workingSet * mb), (long)(privateMb * mb), (long)(managedMb * mb), 5, 10, 0, (long)((workingSet + 10) * mb)),
            null);
    }

    private static BenchEntry Failed(string id) =>
        new(id, "A", new Dictionary<string, string>(), 0, null, null, null, "threw NullReferenceException");
}
