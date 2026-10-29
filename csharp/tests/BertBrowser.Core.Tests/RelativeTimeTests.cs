using BertBrowser.Core.Services;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class RelativeTimeTests
{
    private static readonly DateTime Now = new(2026, 9, 2, 12, 0, 0, DateTimeKind.Utc);

    [Theory]
    [InlineData(0, "just now")]
    [InlineData(59, "just now")]
    [InlineData(60, "1 min ago")]
    [InlineData(59 * 60 + 59, "59 min ago")]
    [InlineData(3600, "1 h ago")]
    [InlineData(23 * 3600 + 3599, "23 h ago")]
    public void Format_ReadsAsAnAge(int secondsAgo, string expected)
    {
        Assert.Equal(expected, RelativeTime.Format(Now.AddSeconds(-secondsAgo), Now));
    }

    [Fact]
    public void Format_ClampsTheFutureToJustNow()
    {
        // A record stamped by a clock a few seconds ahead of ours must not read as "-3 s ago".
        Assert.Equal("just now", RelativeTime.Format(Now.AddSeconds(5), Now));
    }

    [Fact]
    public void Format_FallsBackToADateAfterADay()
    {
        var utc = Now.AddDays(-2);
        Assert.Equal(utc.ToLocalTime().ToString("g"), RelativeTime.Format(utc, Now));
    }

    private static readonly DateTime LocalNow = new(2026, 9, 25, 16, 0, 0);

    [Fact]
    public void DaySaysNeverForNoTimestamp() =>
        Assert.Equal("Never", RelativeTime.Day(null, LocalNow));

    [Fact]
    public void DayNamesTodayAndYesterday()
    {
        Assert.Equal("Today 09:05", RelativeTime.Day(new DateTime(2026, 9, 25, 9, 5, 0), LocalNow));
        Assert.Equal("Yesterday 23:59", RelativeTime.Day(new DateTime(2026, 9, 24, 23, 59, 0), LocalNow));
    }

    [Fact]
    public void DayDropsTheYearOnlyWithinThisYear()
    {
        Assert.Equal("3 Feb 08:15", RelativeTime.Day(new DateTime(2026, 2, 3, 8, 15, 0), LocalNow));
        Assert.Equal("3 Feb 2025", RelativeTime.Day(new DateTime(2025, 2, 3, 8, 15, 0), LocalNow));
    }
}
