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
        Assert.False(FlatViewRules.Decide("D:\\Media", null, FlatViewMode.Files, Cap).Confirm);
    }

    [Theory]
    [InlineData(0)]
    [InlineData(1)]
    [InlineData(Cap - 1)]
    [InlineData(Cap)]      // exactly the cap fits, so nothing is lost and nothing is asked
    public void AtOrUnderTheCapItJustRuns(int files) =>
        Assert.False(FlatViewRules.Decide("D:\\Media", Size(files, 0), FlatViewMode.Files, Cap).Confirm);

    [Fact]
    public void OverTheCapItAsksFirst()
    {
        var decision = FlatViewRules.Decide("D:\\Media", Size(Cap + 1, 0), FlatViewMode.Files, Cap);

        Assert.True(decision.Confirm);
        Assert.Contains("50,001 files", decision.Message);
        Assert.Contains("D:\\Media", decision.Message);
        Assert.Equal("Show 50,000", decision.ConfirmLabel);
    }

    [Fact]
    public void FoldersCountTowardsTheCeilingOnlyWhenTheyAreShown()
    {
        var estimate = Size(files: 40_000, dirs: 20_000);

        // 40,000 files fit; 60,000 files and folders do not.
        Assert.False(FlatViewRules.Decide("D:\\Media", estimate, FlatViewMode.Files, Cap).Confirm);

        var both = FlatViewRules.Decide("D:\\Media", estimate, FlatViewMode.All, Cap);
        Assert.True(both.Confirm);
        Assert.Contains("60,000 files and folders", both.Message);
    }

    [Fact]
    public void AnIncompleteRowIsAFloorAndSaysSo()
    {
        // The size pass could not reach part of the tree, so the real number is this or larger.
        var decision = FlatViewRules.Decide(
            "D:\\Media", Size(Cap + 1, 0, incomplete: true), FlatViewMode.Files, Cap);

        Assert.Contains("at least 50,001 files", decision.Message);
    }

    [Fact]
    public void OffNeverAsks() =>
        Assert.False(FlatViewRules.Decide("D:\\Media", Size(9_000_000, 0), FlatViewMode.Off, Cap).Confirm);
}
