using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Search;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class SizeTextTests
{
    [Theory]
    [InlineData("0", 0L)]
    [InlineData("512", 512L)]
    [InlineData("512b", 512L)]
    [InlineData("1k", 1024L)]
    [InlineData("1kb", 1024L)]
    [InlineData("1KB", 1024L)]
    [InlineData("  2 mb ", 2L * 1024 * 1024)]
    [InlineData("1.5mb", (long)(1.5 * 1024 * 1024))]
    [InlineData("3gb", 3L * 1024 * 1024 * 1024)]
    [InlineData("1tb", 1L << 40)]
    public void ParsesSizesWithUnits(string text, long expected)
    {
        Assert.True(SizeText.TryParse(text, out var bytes));
        Assert.Equal(expected, bytes);
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("mb")]        // a bare unit names no quantity
    [InlineData("kb")]
    [InlineData("-5mb")]      // no negative files
    [InlineData("abc")]
    [InlineData("10 apples")]
    [InlineData("1e400")]     // overflows a double before it reaches long
    public void RefusesWhatIsNotASize(string? text) =>
        Assert.False(SizeText.TryParse(text, out _));

    /// <summary>
    /// The units have to agree with the Size column, or a filter and the number beside it
    /// disagree: at 1000-based multiples a 104,857,600-byte file is "over 100mb" to the filter
    /// and "100 MB" to the list. This is the test that pins 1024.
    /// </summary>
    [Fact]
    public void UnitsAreTheSameOnesTheSizeColumnPrints()
    {
        Assert.True(SizeText.TryParse("100mb", out var hundredMb));
        Assert.Equal("100 MB", ByteSizeFormatter.Format(hundredMb));

        Assert.True(SizeText.TryParse("1gb", out var oneGb));
        Assert.StartsWith("1", ByteSizeFormatter.Format(oneGb));
        Assert.EndsWith("GB", ByteSizeFormatter.Format(oneGb));
    }
}
