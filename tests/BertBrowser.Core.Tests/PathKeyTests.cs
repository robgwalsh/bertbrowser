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

    /// <summary>The key-only form has to give <see cref="PathKey.IsUnder"/>'s answer for every pair
    /// of canonical keys, since it replaces it wherever a loop asks.</summary>
    [Theory]
    [InlineData(@"C:\FOO\FILE.TXT", @"C:\FOO")]
    [InlineData(@"C:\FOO\SUB\DEEP\FILE.TXT", @"C:\FOO")]
    [InlineData(@"C:\FOO", @"C:\FOO")]
    [InlineData(@"C:\FOOBAR\FILE.TXT", @"C:\FOO")]
    [InlineData(@"C:\FO", @"C:\FOO")]
    [InlineData(@"C:\OTHER\FILE.TXT", @"C:\FOO")]
    [InlineData(@"C:\ANY\FILE.TXT", @"C:\")]
    [InlineData(@"C:\ANY", @"C:\")]
    [InlineData(@"C:\", @"C:\")]
    [InlineData(@"D:\ANY\FILE.TXT", @"C:\")]
    [InlineData(@"\\SERVER\SHARE\A\B.TXT", @"\\SERVER\SHARE\")]
    [InlineData(@"\\SERVER\SHARE\A\B.TXT", @"\\SERVER\SHARE\A")]
    [InlineData(@"\\SERVER\SHARED\A", @"\\SERVER\SHARE\")]
    [InlineData(@"C:\A]B\X", @"C:\A")]
    public void IsUnderKey_AgreesWithIsUnder(string key, string dirKey) =>
        Assert.Equal(PathKey.IsUnder(key, dirKey), PathKey.IsUnderKey(key, dirKey));

    [Theory]
    [InlineData(@"C:\FOO\FILE.TXT", true)]
    [InlineData(@"C:\FOO\SUB\DEEP\FILE.TXT", true)]
    [InlineData(@"C:\FOO", false)]            // itself is in the set, and is not its own ancestor
    [InlineData(@"C:\FOOBAR\FILE.TXT", false)]
    [InlineData(@"D:\DATA\X", true)]          // under the drive root, whose key keeps its separator
    [InlineData(@"D:\", false)]
    [InlineData(@"E:\DATA\X", false)]
    public void HasAncestorKeyIn_FindsOnlyRealAncestors(string key, bool expected)
    {
        var dirs = new HashSet<string>(StringComparer.Ordinal) { @"C:\FOO", @"D:\" };
        Assert.Equal(expected, PathKey.HasAncestorKeyIn(key, dirs));
        Assert.Equal(expected, dirs.Any(other => other != key && PathKey.IsUnder(key, other)));
    }
}
