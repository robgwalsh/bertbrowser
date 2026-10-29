using BertBrowser.Core.Models;
using Xunit;

namespace BertBrowser.Core.Tests;

public class AspectRatioTests
{
    [Theory]
    [InlineData("4:3", 4, 3)]
    [InlineData("16:9", 16, 9)]
    [InlineData(" 3 : 2 ", 3, 2)]
    [InlineData("1:1", 1, 1)]
    public void TryParse_AcceptsWidthColonHeight(string text, int width, int height)
    {
        Assert.True(AspectRatio.TryParse(text, out var value));
        Assert.Equal(width, value.Width);
        Assert.Equal(height, value.Height);
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("4")]
    [InlineData("4:3:2")]
    [InlineData("4/3")]
    [InlineData("four:three")]
    [InlineData("0:3")]
    [InlineData("4:0")]
    [InlineData("-4:3")]
    [InlineData("4:-3")]
    [InlineData("4.5:3")]
    [InlineData("101:3")]
    [InlineData("4:101")]
    public void TryParse_RejectsAnythingElse(string? text)
    {
        Assert.False(AspectRatio.TryParse(text, out _));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("nonsense")]
    [InlineData("0:0")]
    public void Parse_FallsBackToDefaultRatherThanThrowing(string? text)
    {
        Assert.Equal(AspectRatio.Default, AspectRatio.Parse(text));
    }

    [Fact]
    public void Default_IsFourByThree() => Assert.Equal("4:3", AspectRatio.Default.ToString());

    [Fact]
    public void Presets_LeadWithTheDefaultShapesAndAllRoundTrip()
    {
        Assert.Contains(AspectRatio.Default, AspectRatio.Presets);
        foreach (var preset in AspectRatio.Presets)
            Assert.Equal(preset, AspectRatio.Parse(preset.ToString()));
    }

    [Theory]
    [InlineData("4:3", 200, 150)]
    [InlineData("1:1", 200, 200)]
    [InlineData("16:9", 160, 90)]
    [InlineData("3:4", 120, 160)]
    public void HeightFor_ScalesTheWidthByTheRatio(string text, double width, double expected)
    {
        Assert.Equal(expected, AspectRatio.Parse(text).HeightFor(width), 6);
    }

    /// <summary>A struct's default value is reachable however careful the factories are; it must
    /// not hand WPF a NaN height.</summary>
    [Fact]
    public void HeightFor_TreatsADefaultConstructedRatioAsSquare()
    {
        Assert.Equal(180, default(AspectRatio).HeightFor(180));
    }
}
