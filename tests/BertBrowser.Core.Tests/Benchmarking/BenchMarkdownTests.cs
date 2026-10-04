using BertBrowser.Core.Benchmarking;
using Xunit;

namespace BertBrowser.Core.Tests.Benchmarking;

public class BenchMarkdownTests
{
    [Fact]
    public void RenderCompare_RegressionsFirst_WithCounts()
    {
        var baseline = BenchCompareTests.Set(
            [BenchCompareTests.Entry("z.same", 1000, 0), BenchCompareTests.Entry("a.slower", 1_000_000, 0),
             BenchCompareTests.Entry("m.alloc", 1000, 100)], false);
        var current = BenchCompareTests.Set(
            [BenchCompareTests.Entry("z.same", 1000, 0), BenchCompareTests.Entry("a.slower", 2_000_000, 0),
             BenchCompareTests.Entry("m.alloc", 1000, 200)], false);

        var md = BenchMarkdown.RenderCompare(BenchCompare.Compare(baseline, current, new CompareOptions()), new CompareOptions(), "Test");

        var lines = md.Split('\n').Select(l => l.TrimEnd()).ToArray();
        Assert.Equal("## Test", lines[0]);
        Assert.Contains("3 benchmarks: 1 allocates more, 1 slower, 1 same", md);

        var rows = lines.Where(l => l.StartsWith("| `")).ToArray();
        Assert.StartsWith("| `m.alloc`", rows[0]);
        Assert.StartsWith("| `a.slower`", rows[1]);
        Assert.StartsWith("| `z.same`", rows[2]);
        Assert.Contains("+100.0%", rows[0]);
        Assert.Contains("| 1.00 ms | 2.00 ms | +100.0% |", rows[1]);
    }

    [Fact]
    public void RenderCompare_SaysWhichGatesAreOn()
    {
        var empty = BenchCompareTests.Set([], false);
        var report = BenchCompare.Compare(empty, empty, new CompareOptions());

        Assert.Contains("Time gate on at 15%", BenchMarkdown.RenderCompare(report, new CompareOptions(), "t"));
        Assert.Contains("Time gate off (reported only)", BenchMarkdown.RenderCompare(report, new CompareOptions(GateTime: false), "t"));
        Assert.Contains("allocation gate on at 3% for every benchmark", BenchMarkdown.RenderCompare(report, new CompareOptions(AllocTolerancePctOverride: 3), "t"));
    }

    [Fact]
    public void RenderResults_OneTablePerTier()
    {
        var set = BenchCompareTests.Set(
            [BenchCompareTests.Entry("core.a", 1234, 2048),
             BenchCompareTests.Entry("ui.b", 812, null, "ms", "B"),
             BenchCompareTests.Entry("startup.c", 1240, null, "ms", "C"),
             new BenchEntry("core.broken", "A", new Dictionary<string, string>(), 0, null, null, null, "boom")],
            false);

        var md = BenchMarkdown.RenderResults(set);

        Assert.Contains("### Tier A — Core micro-benchmarks", md);
        Assert.Contains("### Tier B — UI scenarios", md);
        Assert.Contains("### Tier C — Startup", md);
        Assert.Contains("| `core.a` | 1.23 µs | 1.30 µs | 2.00 KB | 1 | 15 |", md);
        Assert.Contains("| `ui.b` | 812 ms |", md);
        Assert.Contains("| `startup.c` | 1.24 s |", md);
        Assert.Contains("| `core.broken` | error: boom |", md);
    }

    [Theory]
    [InlineData(999, "ns", "999 ns")]
    [InlineData(1500, "ns", "1.50 µs")]
    [InlineData(278_000_000, "ns", "278 ms")]
    [InlineData(2.46e9, "ns", "2.46 s")]
    [InlineData(812.4, "ms", "812 ms")]
    [InlineData(0.5, "ms", "500 µs")]
    public void Time_ScalesToReadWell(double value, string unit, string expected) =>
        Assert.Equal(expected, BenchMarkdown.Time(value, unit));

    [Theory]
    [InlineData(512, "512 B")]
    [InlineData(2048, "2.00 KB")]
    [InlineData(1_843_200, "1.76 MB")]
    [InlineData(3_221_225_472, "3.00 GB")]
    public void Bytes_ScalesToReadWell(long value, string expected) =>
        Assert.Equal(expected, BenchMarkdown.Bytes(value));
}
