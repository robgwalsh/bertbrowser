using BertBrowser.Core.Models;
using BertBrowser.Core.Services.FlatView;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// When a flat view asks before it lists, and what it says. The wording is under test as much as
/// the threshold: the number in it is the only thing that makes declining an informed choice.
/// </summary>
public sealed class FlatViewRulesTests
{
    private const int Cap = 50_000;

    private static DirSizeResult Size(int files, int dirs, bool incomplete = false) =>
        new("D:\\MEDIA", SizeBytes: 0, files, dirs, incomplete, DateTime.UtcNow);

    [Fact]
    public void NoCachedSizeMeansGoAhead()
    {
        // A missing dir_size_cache row means *unknown*, never zero — the same rule that makes a
        // folder's size render blank. Nothing is known, so there is nothing to ask about: this is
        // what every network share and non-NTFS volume takes.
        Assert.False(FlatViewRules.Decide("D:\\Media", null, Cap).Confirm);
    }

    [Theory]
    [InlineData(0)]
    [InlineData(1)]
    [InlineData(Cap - 1)]
    [InlineData(Cap)]      // exactly the cap fits, so nothing is lost and nothing is asked
    public void AtOrUnderTheCapItJustRuns(int files) =>
        Assert.False(FlatViewRules.Decide("D:\\Media", Size(files, 0), Cap).Confirm);

    [Fact]
    public void OverTheCapItAsksFirst()
    {
        var decision = FlatViewRules.Decide("D:\\Media", Size(Cap + 1, 0), Cap);

        Assert.True(decision.Confirm);
        Assert.Contains("50,001 files", decision.Message);
        Assert.Contains("D:\\Media", decision.Message);
        Assert.Equal("Show 50,000", decision.ConfirmLabel);
    }

    [Fact]
    public void FoldersDoNotCountTowardsTheCeiling() =>
        // A flat view lists no folder rows, so 40,000 files fit however many folders hold them.
        Assert.False(FlatViewRules.Decide("D:\\Media", Size(files: 40_000, dirs: 20_000), Cap).Confirm);

    [Fact]
    public void AnIncompleteRowIsAFloorAndSaysSo()
    {
        // The size pass could not reach part of the tree, so the real number is this or larger.
        var decision = FlatViewRules.Decide(
            "D:\\Media", Size(Cap + 1, 0, incomplete: true), Cap);

        Assert.Contains("at least 50,001 files", decision.Message);
    }
}
