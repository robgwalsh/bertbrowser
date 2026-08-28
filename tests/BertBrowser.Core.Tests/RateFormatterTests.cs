using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

public class RateFormatterTests
{
    [Theory]
    [InlineData(null, "")]
    [InlineData(0d, "")]
    [InlineData(-1d, "")]
    [InlineData(512d, "512 B/s")]
    [InlineData(1024d, "1 KB/s")]
    [InlineData(117_440_512d, "112 MB/s")]
    public void Speed_ReadsAsAThroughput(double? rate, string expected) =>
        Assert.Equal(expected, RateFormatter.Speed(rate));

    [Fact]
    public void Speed_IsBlankRatherThanZero_WhenNothingIsKnownYet() =>
        // A "0 B/s" on screen reads as a stalled transfer; a blank reads as "not yet".
        Assert.Equal("", RateFormatter.Speed(null));

    [Fact]
    public void Remaining_IsBlank_WhenThereIsNoEstimate() =>
        Assert.Equal("", RateFormatter.Remaining(null));

    [Theory]
    [InlineData(0, "less than a minute left")]
    [InlineData(59, "less than a minute left")]
    [InlineData(90, "about 2 minutes left")]
    [InlineData(60, "about 1 minute left")]
    [InlineData(360, "about 6 minutes left")]
    [InlineData(3600, "about 1 hour left")]
    [InlineData(4800, "about 1 hour 20 minutes left")]
    [InlineData(7200, "about 2 hours left")]
    public void Remaining_ReadsAsATimeLeft(int seconds, string expected) =>
        Assert.Equal(expected, RateFormatter.Remaining(TimeSpan.FromSeconds(seconds)));

    [Fact]
    public void Remaining_NeverSaysSixtyMinutes()
    {
        // Rounding after splitting hours from minutes turns 1:59:50 into "1 hour 60 minutes".
        var text = RateFormatter.Remaining(TimeSpan.FromSeconds((1 * 3600) + (59 * 60) + 50));
        Assert.DoesNotContain("60 minutes", text);
        Assert.Equal("about 2 hours left", text);
    }
}
