using BertBrowser.Core.Theming;
using Xunit;

namespace BertBrowser.Core.Tests.Theming;

public class ThemeColorTests
{
    [Theory]
    [InlineData("#1E1E1E", 0xFF, 0x1E, 0x1E, 0x1E)]
    [InlineData("1E1E1E", 0xFF, 0x1E, 0x1E, 0x1E)]
    [InlineData("#1e1e1e", 0xFF, 0x1E, 0x1E, 0x1E)]
    [InlineData("  #1E1E1E  ", 0xFF, 0x1E, 0x1E, 0x1E)]
    [InlineData("#79797966", 0x79, 0x79, 0x79, 0x66)]
    [InlineData("#ABC", 0xFF, 0xAA, 0xBB, 0xCC)]
    [InlineData("#8ABC", 0x88, 0xAA, 0xBB, 0xCC)]
    public void Parses_every_supported_form(string text, byte a, byte r, byte g, byte b)
    {
        Assert.True(ThemeColor.TryParse(text, out var color));
        Assert.Equal(new ThemeColor(a, r, g, b), color);
    }

    [Theory]
    [InlineData("")]
    [InlineData(null)]
    [InlineData("#")]
    [InlineData("#12")]
    [InlineData("#12345")]
    [InlineData("#123456789")]
    [InlineData("#GGGGGG")]
    [InlineData("rebeccapurple")]
    public void Rejects_malformed_input_without_throwing(string? text)
    {
        Assert.False(ThemeColor.TryParse(text, out _));
    }

    [Fact]
    public void ToHex_round_trips_and_omits_opaque_alpha()
    {
        Assert.Equal("#1E1E1E", ThemeColor.FromRgb(0x1E, 0x1E, 0x1E).ToHex());
        Assert.Equal("#66797979", new ThemeColor(0x66, 0x79, 0x79, 0x79).ToHex());

        foreach (var literal in new[] { "#1E1E1E", "#66797979", "#000000", "#FFFFFF" })
        {
            Assert.True(ThemeColor.TryParse(literal, out var color));
            Assert.Equal(literal, color.ToHex());
        }
    }

    [Fact]
    public void CompositeOver_blends_toward_the_background()
    {
        var halfWhite = new ThemeColor(0x80, 0xFF, 0xFF, 0xFF);
        var over = halfWhite.CompositeOver(ThemeColor.FromRgb(0, 0, 0));

        Assert.Equal(0xFF, over.A);
        Assert.InRange(over.R, 0x7F, 0x81);

        // Opaque colours are unchanged, whatever is behind them.
        var opaque = ThemeColor.FromRgb(0x12, 0x34, 0x56);
        Assert.Equal(opaque, opaque.CompositeOver(ThemeColor.FromRgb(0xFF, 0xFF, 0xFF)));
    }

    [Fact]
    public void Luminance_orders_black_below_grey_below_white()
    {
        var black = ThemeColor.FromRgb(0, 0, 0).RelativeLuminance();
        var grey = ThemeColor.FromRgb(0x80, 0x80, 0x80).RelativeLuminance();
        var white = ThemeColor.FromRgb(0xFF, 0xFF, 0xFF).RelativeLuminance();

        Assert.Equal(0, black, 6);
        Assert.Equal(1, white, 6);
        Assert.True(black < grey && grey < white);
    }

    [Fact]
    public void Black_on_white_is_the_maximum_contrast_ratio()
    {
        var ratio = ThemeContrast.Ratio(ThemeColor.FromRgb(0, 0, 0), ThemeColor.FromRgb(0xFF, 0xFF, 0xFF));
        Assert.Equal(21, ratio, 2);
    }

    [Theory]
    [InlineData("#FF0000")]
    [InlineData("#00FF00")]
    [InlineData("#0000FF")]
    [InlineData("#1E1E1E")]
    [InlineData("#0F6CBD")]
    [InlineData("#FFFFFF")]
    [InlineData("#000000")]
    public void Hsv_round_trips(string literal)
    {
        var original = ThemeColor.Parse(literal);
        var (h, s, v) = original.ToHsv();
        var round = ThemeColor.FromHsv(h, s, v);

        Assert.Equal(original.R, round.R);
        Assert.Equal(original.G, round.G);
        Assert.Equal(original.B, round.B);
    }
}
