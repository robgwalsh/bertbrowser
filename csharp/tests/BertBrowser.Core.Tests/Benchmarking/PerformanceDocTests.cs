using BertBrowser.Core.Benchmarking;
using Xunit;

namespace BertBrowser.Core.Tests.Benchmarking;

public class PerformanceDocTests
{
    [Fact]
    public void Render_StartsWithGeneratedHeader()
    {
        var md = PerformanceDoc.Render([], "```\nrun\n```");
        Assert.StartsWith(PerformanceDoc.Header, md);
        Assert.Contains("# Performance", md);
        Assert.Contains("## How to run", md);
        Assert.Contains("_No baselines recorded yet._", md);
        Assert.Contains("## What the numbers do and do not mean", md);
    }

    [Fact]
    public void Render_OrdersMachinesAndIdsDeterministically()
    {
        var zeta = BenchCompareTests.Set(
            [BenchCompareTests.Entry("core.z", 1, 0), BenchCompareTests.Entry("core.a", 1, 0)], false, "zeta-machine");
        var alpha = BenchCompareTests.Set([BenchCompareTests.Entry("core.m", 1, 0)], false, "alpha-machine");

        var one = PerformanceDoc.Render([zeta, alpha], "cmds");
        var two = PerformanceDoc.Render([alpha, zeta], "cmds");

        Assert.Equal(one, two);
        Assert.True(one.IndexOf("## alpha-machine", StringComparison.Ordinal) < one.IndexOf("## zeta-machine", StringComparison.Ordinal));
        Assert.True(one.IndexOf("`core.a`", StringComparison.Ordinal) < one.IndexOf("`core.z`", StringComparison.Ordinal));
    }

    [Fact]
    public void Render_LegendCarriesTheMachine()
    {
        var set = BenchCompareTests.Set([BenchCompareTests.Entry("core.a", 1, 0)], false, "the-key");
        var md = PerformanceDoc.Render([set], "cmds");

        Assert.Contains("## the-key", md);
        Assert.Contains("| CPU | Test CPU, 8 logical cores |", md);
        Assert.Contains("| RAM | 16 GB |", md);
        Assert.Contains("| Corpus | synthetic, scale 1 (100,000 rows) |", md);
        Assert.Contains("`012345678`", md);
    }

    [Fact]
    public void Render_RealBaselineIsLabelled()
    {
        var set = BenchCompareTests.Set([BenchCompareTests.Entry("core.a", 1, 0)], real: true, "the-key");
        var md = PerformanceDoc.Render([set], "cmds");
        Assert.Contains("## the-key (real data)", md);
        Assert.Contains("| Corpus | the machine's own index and folders |", md);
    }
}
