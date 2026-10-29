using BertBrowser.Core.Updates;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The channel predicate decides which update feed an installed copy looks at, from nothing but its
/// own version string. Mutate <see cref="ReleaseChannel.IsUnstable"/> to answer unconditionally —
/// either way — and one half of this goes red.
/// </summary>
public sealed class ReleaseChannelTests
{
    [Theory]
    [InlineData("1.1.3-unstable.42")]
    [InlineData("1.1.3-unstable.7+abc1234")]      // source-linked build metadata
    [InlineData("1.1.3-unstable")]                // no run number
    [InlineData("1.1.3-UNSTABLE.1")]              // the tag is not case-sensitive to us
    [InlineData("  1.1.3-unstable.42  ")]
    public void AnUnstableTag_IsTheUnstableChannel(string version)
    {
        Assert.True(ReleaseChannel.IsUnstable(version));
    }

    [Theory]
    [InlineData("1.1.2")]
    [InlineData("1.1.2+abc1234")]
    [InlineData("1.2.0-beta.1")]
    [InlineData("1.2.0-rc.1")]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData(null)]
    public void AnythingElse_IsStable(string? version)
    {
        Assert.False(ReleaseChannel.IsUnstable(version));
    }

    /// <summary>
    /// The reason this matches an identifier rather than a substring: build metadata is not a
    /// pre-release tag, and a version whose metadata merely mentions the word is a stable build.
    /// </summary>
    [Theory]
    [InlineData("1.2.3+unstable")]
    [InlineData("1.2.3+unstable-notes")]
    [InlineData("1.2.3-beta.unstable")]
    public void TheWordElsewhereInTheVersion_DoesNotSwitchChannel(string version)
    {
        Assert.False(ReleaseChannel.IsUnstable(version));
    }
}
