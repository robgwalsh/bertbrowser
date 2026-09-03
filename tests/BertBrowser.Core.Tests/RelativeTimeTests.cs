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
}
