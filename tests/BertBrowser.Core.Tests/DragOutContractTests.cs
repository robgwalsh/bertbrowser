using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The drag-out rule decides whether a finished drag means "delete the user's originals", on the
/// word of an external application. That makes it the most dangerous branch in the feature and the
/// one worth pinning down completely — so it is a pure function, and this is its truth table.
/// Mutate any row of <see cref="DragOutContract.Decide"/> and one of these goes red.
/// </summary>
public sealed class DragOutContractTests
{
    private static DragOutAction Decide(
        DropEffect returned,
        DropEffect? logical = null,
        DropEffect? performed = null,
        bool handledInApp = false) =>
        DragOutContract.Decide(handledInApp, returned, logical, performed);

    // --- Our own drops ---

    /// <summary>
    /// The trap this guard exists for: DropPipeline handles an in-app drop and TransferExecutor has
    /// already relocated the items, but DoDragDrop still reports Move. Acting on that would delete
    /// what we just placed at the destination's source.
    /// </summary>
    [Theory]
    [InlineData(DropEffect.Move)]
    [InlineData(DropEffect.Copy)]
    [InlineData(DropEffect.None)]
    public void OurOwnPipelineHandlingTheDrop_MeansThereIsNothingLeftToDo(DropEffect returned)
    {
        Assert.Equal(
            DragOutAction.Nothing,
            Decide(returned, logical: DropEffect.Move, performed: DropEffect.Move, handledInApp: true));
    }

    // --- Nothing happened ---

    [Fact]
    public void ACancelledOrRefusedDrag_DoesNothing()
    {
        Assert.Equal(DragOutAction.Nothing, Decide(DropEffect.None));
    }

    [Fact]
    public void ACancelledDrag_StaysNothing_EvenIfATargetLeftAPerformedEffectBehind()
    {
        Assert.Equal(DragOutAction.Nothing, Decide(DropEffect.None, performed: DropEffect.Move));
    }

    // --- Copy and link never remove anything ---

    [Fact]
    public void ACopy_LeavesTheOriginalsAndOnlyRefreshes()
    {
        Assert.Equal(DragOutAction.RefreshOnly, Decide(DropEffect.Copy));
    }

    [Fact]
    public void ALink_IsNotAMove()
    {
        Assert.Equal(DragOutAction.RefreshOnly, Decide(DropEffect.Link));
    }

    // --- The optimized move: the target already did it ---

    /// <summary>Explorer moving within a volume relocates the files itself and reports None, which
    /// means "already done" — deleting here would destroy files that are now at the destination.</summary>
    [Fact]
    public void AnOptimizedMove_ReportedAsNone_RemovesNothing()
    {
        Assert.Equal(DragOutAction.RefreshOnly, Decide(DropEffect.Move, logical: DropEffect.None));
    }

    [Fact]
    public void AnOptimizedMove_ReportedOnlyInTheLegacyFormat_RemovesNothing()
    {
        Assert.Equal(DragOutAction.RefreshOnly, Decide(DropEffect.Move, performed: DropEffect.None));
    }

    // --- The non-optimized move: ours to finish ---

    [Fact]
    public void ANonOptimizedMove_RemovesTheSources()
    {
        Assert.Equal(DragOutAction.RemoveSources, Decide(DropEffect.Move, logical: DropEffect.Move));
    }

    [Fact]
    public void ANonOptimizedMove_ReportedOnlyInTheLegacyFormat_RemovesTheSources()
    {
        Assert.Equal(DragOutAction.RemoveSources, Decide(DropEffect.Move, performed: DropEffect.Move));
    }

    /// <summary>
    /// Plenty of targets ignore the performed-effect protocol entirely. Falling back to the return
    /// value is the documented reading and is what makes a real move out of the app work at all;
    /// the safety net is that the removal goes through the reversible delete, not an erase.
    /// </summary>
    [Fact]
    public void AMoveWithNoReportAtAll_FallsBackToTheReturnedEffect()
    {
        Assert.Equal(DragOutAction.RemoveSources, Decide(DropEffect.Move));
    }

    // --- Disagreement between the two formats ---

    /// <summary>LOGICALPERFORMEDDROPEFFECT exists precisely because targets report Move in the older
    /// format even for an optimized move. When they disagree, the newer one is the truthful one.</summary>
    [Fact]
    public void WhenTheTwoReportsDisagree_TheLogicalOneWins_AndCanSpareTheSources()
    {
        Assert.Equal(
            DragOutAction.RefreshOnly,
            Decide(DropEffect.Move, logical: DropEffect.None, performed: DropEffect.Move));
    }

    [Fact]
    public void WhenTheTwoReportsDisagree_TheLogicalOneWins_AndCanCallForRemoval()
    {
        Assert.Equal(
            DragOutAction.RemoveSources,
            Decide(DropEffect.Move, logical: DropEffect.Move, performed: DropEffect.None));
    }

    /// <summary>A target that answers "I copied" to a Move drag is contradicting itself. Nothing
    /// but an explicit Move earns a deletion.</summary>
    [Fact]
    public void ATargetThatContradictsItself_DoesNotEarnADeletion()
    {
        Assert.Equal(DragOutAction.RefreshOnly, Decide(DropEffect.Move, logical: DropEffect.Copy));
    }

    // --- Bit hygiene ---

    /// <summary>The shell ORs in DROPEFFECT_SCROLL during auto-scroll. A raw equality check against
    /// Move would miss a value carrying it, and a raw check against None would misread it.</summary>
    [Fact]
    public void TheScrollBit_IsNotMistakenForAVerb()
    {
        Assert.Equal(DragOutAction.Nothing, Decide(DropEffect.Scroll));
    }

    [Fact]
    public void TheScrollBit_DoesNotHideAMove()
    {
        Assert.Equal(
            DragOutAction.RemoveSources,
            Decide(DropEffect.Move | DropEffect.Scroll, logical: DropEffect.Move));
    }

    [Fact]
    public void TheScrollBit_DoesNotHideACopy()
    {
        Assert.Equal(DragOutAction.RefreshOnly, Decide(DropEffect.Copy | DropEffect.Scroll));
    }

    /// <summary>A target reporting Move alongside the scroll bit is still reporting Move.</summary>
    [Fact]
    public void TheScrollBit_IsIgnoredInTheReportedEffectToo()
    {
        Assert.Equal(
            DragOutAction.RemoveSources,
            Decide(DropEffect.Move, logical: DropEffect.Move | DropEffect.Scroll));
    }

    /// <summary>Copy+Move together is what the source offers, not what a target answers; if one
    /// comes back that way the Move bit is present and the sources are ours to remove.</summary>
    [Fact]
    public void ACopyMoveCombination_IsTreatedAsAMove()
    {
        Assert.Equal(DragOutAction.RemoveSources, Decide(DropEffect.Copy | DropEffect.Move));
    }
}
