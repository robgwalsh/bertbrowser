using BertBrowser.Core.Services.Search;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The clock is injected throughout, which is the point: "today" is otherwise a test that
/// fails once a day at midnight. Same reason <c>TransferRate</c> takes its timestamps.
/// </summary>
public sealed class DateShorthandTests
{
    /// <summary>A Wednesday, so the week boundaries are not accidentally right.</summary>
    private static readonly DateTime Now = new(2026, 8, 26, 15, 30, 0, DateTimeKind.Local);

    private static (DateTime Lo, DateTime Hi) Resolve(string text)
    {
        Assert.True(DateShorthand.TryResolve(text, Now, out var lo, out var hi));
        return (lo, hi);
    }

    /// <summary>The local wall-clock instant a bound is expected to fall on, in UTC.</summary>
    private static DateTime Local(int y, int m, int d) =>
        DateTime.SpecifyKind(new DateTime(y, m, d), DateTimeKind.Local).ToUniversalTime();

    [Fact]
    public void Today()
    {
        var (lo, hi) = Resolve("today");
        Assert.Equal(Local(2026, 8, 26), lo);
        Assert.Equal(Local(2026, 8, 27), hi);
    }

    [Fact]
    public void Yesterday()
    {
        var (lo, hi) = Resolve("yesterday");
        Assert.Equal(Local(2026, 8, 25), lo);
        Assert.Equal(Local(2026, 8, 26), hi);
    }

    [Fact]
    public void ThisWeekStartsMonday()
    {
        // 26 Aug 2026 is a Wednesday; the ISO week began on Monday the 24th.
        var (lo, hi) = Resolve("thisweek");
        Assert.Equal(Local(2026, 8, 24), lo);
        Assert.Equal(Local(2026, 8, 31), hi);
    }

    [Fact]
    public void LastMonth()
    {
        var (lo, hi) = Resolve("lastmonth");
        Assert.Equal(Local(2026, 7, 1), lo);
        Assert.Equal(Local(2026, 8, 1), hi);
    }

    [Fact]
    public void LastNDaysEndsAtTheEndOfToday()
    {
        // Something modified an hour ago is inside "last7days" — a window that stopped at the
        // start of today would exclude everything the user did this morning.
        var (lo, hi) = Resolve("last7days");
        Assert.Equal(Local(2026, 8, 27), hi);
        Assert.Equal(Local(2026, 8, 20), lo);
    }

    [Theory]
    [InlineData("2026", 2026, 1, 1, 2027, 1, 1)]
    [InlineData("2026-08", 2026, 8, 1, 2026, 9, 1)]
    [InlineData("2026-08-26", 2026, 8, 26, 2026, 8, 27)]
    public void CalendarSpans(string text, int ly, int lm, int ld, int hy, int hm, int hd)
    {
        var (lo, hi) = Resolve(text);
        Assert.Equal(Local(ly, lm, ld), lo);
        Assert.Equal(Local(hy, hm, hd), hi);
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("banana")]
    [InlineData("2026-13-01")]   // no thirteenth month
    [InlineData("last")]         // no count
    [InlineData("lastfortnights")]
    [InlineData("0000")]
    public void RefusesWhatIsNotADate(string? text) =>
        Assert.False(DateShorthand.TryResolve(text, Now, out _, out _));

    /// <summary>
    /// Shorthands mean the user's day, not UTC's. Resolving in UTC would shift every result by
    /// the machine's offset — invisible in London and eight hours wrong in California.
    /// </summary>
    [Fact]
    public void ResolvesAgainstLocalMidnightNotUtcMidnight()
    {
        var (lo, _) = Resolve("today");
        Assert.Equal(DateTimeKind.Utc, lo.Kind);
        Assert.Equal(new DateTime(2026, 8, 26), lo.ToLocalTime().Date);
        Assert.Equal(TimeSpan.Zero, lo.ToLocalTime().TimeOfDay);
    }
}
