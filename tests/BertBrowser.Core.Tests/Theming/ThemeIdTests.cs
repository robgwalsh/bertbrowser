using BertBrowser.Core.Theming;
using Xunit;

namespace BertBrowser.Core.Tests.Theming;

/// <summary>
/// A theme id becomes a filename in an elevated process, so these are the rules standing between an
/// imported <c>*.json</c> and a write outside the themes folder. Mutate <see cref="ThemeId.IsSafe"/>
/// to return true and the traversal theories below go red.
/// </summary>
public class ThemeIdTests
{
    [Theory]
    [InlineData("dark-plus")]
    [InlineData("my-theme-2")]
    [InlineData("theme")]
    [InlineData("123")]
    [InlineData("Mixed-Case")]
    public void OrdinaryIdsAreAccepted(string id) => Assert.True(ThemeId.IsSafe(id));

    [Theory]
    // Rooted: Path.Combine discards the themes folder entirely for these.
    [InlineData(@"C:\Windows\Temp\evil")]
    [InlineData(@"\\server\share\evil")]
    [InlineData("/etc/passwd")]
    // Traversal: resolves out of the folder even though it stays relative.
    [InlineData(@"..\..\..\Windows\Temp\evil")]
    [InlineData("../../evil")]
    [InlineData("..")]
    [InlineData(".")]
    // A separator anywhere makes it more than one path segment.
    [InlineData(@"sub\theme")]
    [InlineData("sub/theme")]
    public void APathIsNotAnId(string id) => Assert.False(ThemeId.IsSafe(id));

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData(null)]
    public void NothingIsNotAnId(string? id) => Assert.False(ThemeId.IsSafe(id));

    [Theory]
    [InlineData("con")]
    [InlineData("CON")]
    [InlineData("nul")]
    [InlineData("LPT1")]
    public void ReservedDeviceNamesAreRefused(string id) => Assert.False(ThemeId.IsSafe(id));

    [Theory]
    // Windows strips these, so "evil " and "evil" would be one file under two ids.
    [InlineData("evil ")]
    [InlineData(" evil")]
    [InlineData("evil.")]
    public void NamesWindowsSilentlyRewritesAreRefused(string id) => Assert.False(ThemeId.IsSafe(id));

    [Theory]
    [InlineData("theme:stream")]
    [InlineData("theme*")]
    [InlineData("theme?")]
    [InlineData("theme|pipe")]
    [InlineData("theme\"quote")]
    public void CharactersAFilenameCannotHoldAreRefused(string id) => Assert.False(ThemeId.IsSafe(id));

    [Fact]
    public void AnIdLongEnoughToThreatenMaxPathIsRefused() =>
        Assert.False(ThemeId.IsSafe(new string('a', ThemeId.MaxLength + 1)));

    [Fact]
    public void AnIdExactlyAtTheLimitIsAccepted() =>
        Assert.True(ThemeId.IsSafe(new string('a', ThemeId.MaxLength)));

    [Fact]
    public void UniqueSlugsADisplayNameDownToLettersDigitsAndHyphens() =>
        Assert.Equal("my-theme", ThemeId.Unique("My Theme!", []));

    [Fact]
    public void UniqueSuffixesUntilItIsFree() =>
        Assert.Equal("my-theme-3", ThemeId.Unique("My Theme", ["my-theme", "my-theme-2"]));

    [Fact]
    public void UniqueTreatsTakenIdsCaseInsensitively() =>
        Assert.Equal("my-theme-2", ThemeId.Unique("My Theme", ["MY-THEME"]));

    [Theory]
    [InlineData(@"C:\Windows\Temp\evil")]
    [InlineData(@"..\..\evil")]
    [InlineData("!!!")]
    [InlineData("")]
    [InlineData(null)]
    [InlineData("con")]
    public void UniqueAlwaysProducesSomethingSafe(string? name) =>
        Assert.True(ThemeId.IsSafe(ThemeId.Unique(name, [])));

    [Fact]
    public void UniqueFallsBackWhenANameSlugsToNothing() =>
        Assert.Equal("theme", ThemeId.Unique("!!!", []));

    [Fact]
    public void UniqueKeepsAVeryLongNameWithinTheLimit() =>
        Assert.True(ThemeId.Unique(new string('a', 500), []).Length <= ThemeId.MaxLength);
}
