using BertBrowser.Core.Benchmarking;
using Xunit;

namespace BertBrowser.Core.Tests.Benchmarking;

public class MachineKeyTests
{
    private static readonly MachineFacts Facts = new("AMD Ryzen 9 7950X 16-Core Processor", 32, 64, 26200, "10.0");

    [Fact]
    public void Compute_IsDeterministic() =>
        Assert.Equal(MachineKey.Compute(Facts), MachineKey.Compute(Facts with { }));

    [Fact]
    public void Compute_ReadsAsWhatItIs()
    {
        var key = MachineKey.Compute(Facts);
        Assert.StartsWith("amd-ryzen-9-7950x-16-core-32c-64gb-w26200-net10.0-", key);
        Assert.Matches("-[0-9a-f]{8}$", key);
    }

    [Fact]
    public void Compute_DifferentCoreCount_DifferentKey() =>
        Assert.NotEqual(MachineKey.Compute(Facts), MachineKey.Compute(Facts with { LogicalCores = 16 }));

    [Fact]
    public void Compute_DifferentOsBuild_DifferentKey() =>
        Assert.NotEqual(MachineKey.Compute(Facts), MachineKey.Compute(Facts with { OsBuild = 22631 }));

    [Fact]
    public void Compute_SameSlugDifferentCpu_StillDiffersInHash()
    {
        // Two names that slug identically after the clock suffix is dropped still get their own key.
        var a = MachineKey.Compute(Facts with { Cpu = "Intel Core i7 @ 2.60GHz" });
        var b = MachineKey.Compute(Facts with { Cpu = "Intel Core i7 @ 3.10GHz" });
        Assert.Equal(a[..a.LastIndexOf('-')], b[..b.LastIndexOf('-')]);
        Assert.NotEqual(a, b);
    }

    [Theory]
    [InlineData("Intel(R) Core(TM) i9-13900K", "intel-core-i9-13900k")]
    [InlineData("AMD Ryzen 7 5800X 8-Core Processor", "amd-ryzen-7-5800x-8-core")]
    [InlineData("Intel(R) Xeon(R) CPU E5-2690 v4 @ 2.60GHz", "intel-xeon-e5-2690-v4")]
    [InlineData("AMD Ryzen 7 PRO 6850U with Radeon Graphics", "amd-ryzen-7-pro-6850u")]
    [InlineData("   ", "cpu")]
    public void Slug_StripsVendorNoise(string cpu, string expected) =>
        Assert.Equal(expected, MachineKey.Slug(cpu));

    [Fact]
    public void Slug_IsBounded()
    {
        var slug = MachineKey.Slug("Some Very Long Processor Marketing Name With Many Many Words In It");
        Assert.True(slug.Length <= 32);
        Assert.False(slug.EndsWith('-'));
    }

    [Fact]
    public void Probe_ReturnsPlausibleFacts()
    {
        if (!OperatingSystem.IsWindows()) return;

        var facts = MachineKey.Probe();
        Assert.True(facts.LogicalCores > 0);
        Assert.True(facts.RamGb > 0);
        Assert.True(facts.OsBuild > 0);
        Assert.Matches(@"^\d+\.\d+$", facts.RuntimeMajorMinor);
        Assert.False(string.IsNullOrWhiteSpace(facts.Cpu));
    }
}
