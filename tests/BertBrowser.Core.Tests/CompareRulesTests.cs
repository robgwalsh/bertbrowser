using BertBrowser.Core.Services.Compare;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The judgements a comparison makes about what to show and what a sync may touch. Pure, and held
/// still here because every one of them is read from a colour on a row rather than from a number.
/// </summary>
public sealed class CompareRulesTests
{
    // --- The per-side projection ---

    /// <summary>The mirror is the whole point of having a row state at all: one verdict, two
    /// opposite readings, so neither list ever tells the user the other list's story.</summary>
    [Fact]
    public void NewerOnOneSide_ReadsAsOlderOnTheOther()
    {
        Assert.Equal(CompareRowState.Newer, CompareRules.RowState(CompareVerdict.LeftNewer, CompareSide.Left));
        Assert.Equal(CompareRowState.Older, CompareRules.RowState(CompareVerdict.LeftNewer, CompareSide.Right));
        Assert.Equal(CompareRowState.Newer, CompareRules.RowState(CompareVerdict.RightNewer, CompareSide.Right));
        Assert.Equal(CompareRowState.Older, CompareRules.RowState(CompareVerdict.RightNewer, CompareSide.Left));
    }

    /// <summary>There is no row on the other side to colour, so there is nothing to say there.</summary>
    [Fact]
    public void OnlyOnOneSide_SaysNothingOnTheOther()
    {
        Assert.Equal(CompareRowState.OnlyHere, CompareRules.RowState(CompareVerdict.LeftOnly, CompareSide.Left));
        Assert.Equal(CompareRowState.None, CompareRules.RowState(CompareVerdict.LeftOnly, CompareSide.Right));
        Assert.Equal(CompareRowState.OnlyHere, CompareRules.RowState(CompareVerdict.RightOnly, CompareSide.Right));
        Assert.Equal(CompareRowState.None, CompareRules.RowState(CompareVerdict.RightOnly, CompareSide.Left));
    }

    [Theory]
    [InlineData(CompareVerdict.Same, CompareRowState.Same)]
    [InlineData(CompareVerdict.Differs, CompareRowState.Differs)]
    [InlineData(CompareVerdict.Unknown, CompareRowState.Unknown)]
    public void SymmetricVerdicts_ReadTheSameOnBothSides(CompareVerdict verdict, CompareRowState expected)
    {
        Assert.Equal(expected, CompareRules.RowState(verdict, CompareSide.Left));
        Assert.Equal(expected, CompareRules.RowState(verdict, CompareSide.Right));
    }

    // --- What the filter keeps ---

    /// <summary>
    /// "Show only differences" hiding an uncomparable row would be the one way a row nothing is
    /// known about disappears from the screen the user is about to sync from.
    /// </summary>
    [Fact]
    public void AnUnknownSurvivesTheDifferencesFilter()
    {
        Assert.True(CompareRules.IsDifference(CompareVerdict.Unknown));
        Assert.False(CompareRules.IsDifference(CompareVerdict.Same));
    }

    // --- What a sync would do ---

    /// <summary>You cannot sync what you could not compare, in either direction.</summary>
    [Fact]
    public void AnUnknownIsNeverActedOn()
    {
        Assert.False(CompareRules.WouldCopy(CompareVerdict.Unknown));
        Assert.False(CompareRules.WouldDelete(CompareVerdict.Unknown));
    }

    [Fact]
    public void OnlyTheRightOnlySideIsEverDeleted()
    {
        Assert.True(CompareRules.WouldDelete(CompareVerdict.RightOnly));

        foreach (var verdict in Enum.GetValues<CompareVerdict>())
        {
            if (verdict is CompareVerdict.RightOnly) continue;
            Assert.False(CompareRules.WouldDelete(verdict));
        }
    }

    [Theory]
    [InlineData(CompareVerdict.LeftOnly)]
    [InlineData(CompareVerdict.LeftNewer)]
    [InlineData(CompareVerdict.RightNewer)]
    [InlineData(CompareVerdict.Differs)]
    public void EveryUnsettledPairIsCopied(CompareVerdict verdict) =>
        Assert.True(CompareRules.WouldCopy(verdict));

    [Fact]
    public void AMatchIsNeverCopied()
    {
        Assert.False(CompareRules.WouldCopy(CompareVerdict.Same));
        Assert.False(CompareRules.WouldCopy(CompareVerdict.RightOnly));
    }

    /// <summary>The one copy a user would not expect to have agreed to, flagged so the planner can
    /// leave it unticked.</summary>
    [Fact]
    public void OverwritingTheNewerFileIsSingledOut()
    {
        Assert.True(CompareRules.OverwritesNewer(CompareVerdict.RightNewer));
        Assert.False(CompareRules.OverwritesNewer(CompareVerdict.LeftNewer));
        Assert.False(CompareRules.OverwritesNewer(CompareVerdict.LeftOnly));
    }

    // --- The fold ---

    /// <summary>
    /// A child only ever reports that something beneath is unsettled — never which side it is
    /// missing from. Without that, one left-only file would rename its whole parent folder
    /// "left only" and a sync would try to copy a folder that is already there.
    /// </summary>
    [Fact]
    public void AChildNeverTellsItsFolderWhichSideIsMissing()
    {
        Assert.Equal(
            CompareVerdict.Differs,
            CompareRules.RollUp(CompareVerdict.Same, CompareVerdict.LeftOnly));

        Assert.Equal(
            CompareVerdict.Differs,
            CompareRules.RollUp(CompareVerdict.Same, CompareVerdict.RightNewer));
    }

    [Fact]
    public void AMatchingChildLeavesTheFolderAlone()
    {
        Assert.Equal(CompareVerdict.Same, CompareRules.RollUp(CompareVerdict.Same, CompareVerdict.Same));
        Assert.Equal(CompareVerdict.Differs, CompareRules.RollUp(CompareVerdict.Differs, CompareVerdict.Same));
    }

    /// <summary>
    /// The folder is absent on one side, so everything under it is too; nothing a child says can
    /// downgrade that to a mere difference.
    /// </summary>
    [Fact]
    public void AnAbsentFolderStaysAbsent()
    {
        Assert.Equal(
            CompareVerdict.LeftOnly,
            CompareRules.RollUp(CompareVerdict.LeftOnly, CompareVerdict.LeftOnly));
    }

    /// <summary>Unknown outranks everything, because the only verdict that must never be arrived
    /// at by not knowing is <see cref="CompareVerdict.Same"/>.</summary>
    [Fact]
    public void AnUnknownChildOutranksEverything()
    {
        foreach (var folder in Enum.GetValues<CompareVerdict>())
        {
            Assert.Equal(CompareVerdict.Unknown, CompareRules.RollUp(folder, CompareVerdict.Unknown));
        }
    }

    /// <summary>The fold runs over a dictionary in whatever order it enumerates, so the order two
    /// children arrive in must not change the answer.</summary>
    [Fact]
    public void TheFoldDoesNotDependOnTheOrderChildrenArriveIn()
    {
        var children = Enum.GetValues<CompareVerdict>();

        foreach (var a in children)
        {
            foreach (var b in children)
            {
                Assert.Equal(
                    CompareRules.RollUp(CompareRules.RollUp(CompareVerdict.Same, a), b),
                    CompareRules.RollUp(CompareRules.RollUp(CompareVerdict.Same, b), a));
            }
        }
    }
}
