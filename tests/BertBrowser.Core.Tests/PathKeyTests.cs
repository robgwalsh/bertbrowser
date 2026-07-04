using BertBrowser.Core.Paths;
using Xunit;

namespace BertBrowser.Core.Tests;

public class PathKeyTests
{
    [Theory]
    [InlineData(@"C:\Foo\Bar", @"C:\FOO\BAR")]
    [InlineData(@"C:\Foo\Bar\", @"C:\FOO\BAR")]
    [InlineData(@"C:/Foo/Bar", @"C:\FOO\BAR")]
    [InlineData(@"C:\Foo\.\Bar", @"C:\FOO\BAR")]
    [InlineData(@"C:\Foo\Baz\..\Bar", @"C:\FOO\BAR")]
    [InlineData(@"c:\foo", @"C:\FOO")]
    public void Canonicalize_NormalizesForm(string input, string expected) =>
        Assert.Equal(expected, PathKey.Canonicalize(input));

    [Fact]
    public void Canonicalize_DriveRoot_KeepsTrailingSeparator()
    {
        Assert.Equal(@"C:\", PathKey.Canonicalize(@"C:\"));
        Assert.Equal(@"C:\", PathKey.Canonicalize(@"c:\"));
    }

    [Fact]
    public void Canonicalize_PreservesSpecialCharacters()
    {
        Assert.Equal(@"C:\A%B\C_D\E[F]", PathKey.Canonicalize(@"C:\a%b\c_d\e[f]"));
    }

    [Fact]
    public void Canonicalize_UnicodeUppercased()
    {
        Assert.Equal(@"C:\ÜBUNG\ÉTÉ", PathKey.Canonicalize(@"C:\übung\été"));
    }

    [Fact]
    public void Canonicalize_EmptyThrows()
    {
        Assert.Throws<ArgumentException>(() => PathKey.Canonicalize(""));
        Assert.Throws<ArgumentException>(() => PathKey.Canonicalize("   "));
    }

    [Fact]
    public void NormalizeDisplay_KeepsCasing()
    {
        Assert.Equal(@"C:\Foo\Bar", PathKey.NormalizeDisplay(@"C:\Foo\Bar\"));
    }

    [Fact]
    public void PrefixBounds_SimpleDirectory()
    {
        var (lo, hi) = PathKey.PrefixBounds(@"C:\Foo");
        Assert.Equal(@"C:\FOO\", lo);
        Assert.Equal(@"C:\FOO]", hi);
    }

    [Fact]
    public void PrefixBounds_DriveRoot_NoDoubledSeparator()
    {
        var (lo, hi) = PathKey.PrefixBounds(@"C:\");
        Assert.Equal(@"C:\", lo);
        Assert.Equal(@"C:]", hi);
    }

    [Theory]
    [InlineData(@"C:\FOO\FILE.TXT", true)]
    [InlineData(@"C:\FOO\SUB\DEEP\FILE.TXT", true)]
    [InlineData(@"C:\FOO", false)]           // the directory itself is not under itself
    [InlineData(@"C:\FOOBAR\FILE.TXT", false)] // sibling with the same name prefix
    [InlineData(@"C:\OTHER\FILE.TXT", false)]
    public void IsUnder_RespectsSubtreeBoundaries(string key, bool expected) =>
        Assert.Equal(expected, PathKey.IsUnder(key, @"C:\Foo"));

    [Fact]
    public void IsUnder_SpecialCharactersInDirectoryName()
    {
        Assert.True(PathKey.IsUnder(@"C:\A%B\X.TXT", @"C:\a%b"));
        Assert.True(PathKey.IsUnder(@"C:\A_B\X.TXT", @"C:\a_b"));
        Assert.False(PathKey.IsUnder(@"C:\AXB\X.TXT", @"C:\a_b")); // '_' must not act as a wildcard
    }

    [Fact]
    public void IsUnder_DriveRoot_ContainsEverythingOnDrive()
    {
        Assert.True(PathKey.IsUnder(@"C:\ANY\FILE.TXT", @"C:\"));
        Assert.False(PathKey.IsUnder(@"D:\ANY\FILE.TXT", @"C:\"));
    }
}
