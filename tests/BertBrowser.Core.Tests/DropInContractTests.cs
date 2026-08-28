using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The truth table for an incoming drop. The dangerous half is what gets reported back: an external
/// source acts on it, so reporting Move for something the user meant as a copy deletes another
/// application's files on our say-so.
/// </summary>
public sealed class DropInContractTests
{
    private const DropEffect Both = DropEffect.Copy | DropEffect.Move;

    // --- In-app: Ctrl copies, everything else moves, and nothing is ever reported ---

    [Theory]
    [InlineData(false, false, TransferVerb.Move)]
    [InlineData(false, true, TransferVerb.Move)]   // Shift is the list's range modifier, not a verb
    [InlineData(true, false, TransferVerb.Copy)]
    [InlineData(true, true, TransferVerb.Copy)]
    public void InApp_CtrlCopies_EverythingElseMoves(bool control, bool shift, TransferVerb expected)
    {
        var decision = DropInContract.Decide(DropOrigin.InApp, control, shift, Both);

        Assert.Equal(expected, decision.Verb);
    }

    /// <summary>
    /// The one that protects our own files. Our drop already happened through TransferExecutor, so
    /// reporting anything but None would have DragOutContract read our own drop as a foreign move
    /// and delete the items we just placed. Make this report the verb and it goes red.
    /// </summary>
    [Theory]
    [InlineData(false, false)]
    [InlineData(true, false)]
    [InlineData(false, true)]
    public void InApp_ReportsNothing_WhateverItDid(bool control, bool shift)
    {
        var decision = DropInContract.Decide(DropOrigin.InApp, control, shift, Both);

        Assert.Equal(DropEffect.None, decision.Report);
    }

    // --- External: copying is the default, and a move must be asked for ---

    /// <summary>
    /// The source is a window we know nothing about, and a move makes <em>it</em> delete its files.
    /// So "put this here" is read as a copy unless the user says otherwise. Flip this default and
    /// dragging a file out of a mail client would remove the attachment.
    /// </summary>
    [Fact]
    public void External_DefaultsToCopy_NotMove()
    {
        var decision = DropInContract.Decide(DropOrigin.External, control: false, shift: false, Both);

        Assert.Equal(TransferVerb.Copy, decision.Verb);
        Assert.Equal(DropEffect.Copy, decision.Report);
    }

    [Fact]
    public void External_ShiftAsksForAMove()
    {
        var decision = DropInContract.Decide(DropOrigin.External, control: false, shift: true, Both);

        Assert.Equal(TransferVerb.Move, decision.Verb);
        Assert.Equal(DropEffect.Move, decision.Report);
    }

    /// <summary>Ctrl means copy on both sides, so holding it can never become a move by accident —
    /// not even with Shift also down.</summary>
    [Fact]
    public void External_CtrlForcesCopyEvenWithShift()
    {
        var decision = DropInContract.Decide(DropOrigin.External, control: true, shift: true, Both);

        Assert.Equal(TransferVerb.Copy, decision.Verb);
    }

    /// <summary>
    /// A source that will not permit a move must never be told one: it may act on the report.
    /// </summary>
    [Fact]
    public void External_NeverReportsAVerbTheSourceDidNotOffer()
    {
        var decision = DropInContract.Decide(
            DropOrigin.External, control: false, shift: true, DropEffect.Copy);

        Assert.Equal(TransferVerb.Copy, decision.Verb);
        Assert.Equal(DropEffect.Copy, decision.Report);
    }

    /// <summary>A move-only source still works, rather than the drop silently doing nothing.</summary>
    [Fact]
    public void External_FallsBackToTheVerbTheSourceDoesOffer()
    {
        var decision = DropInContract.Decide(
            DropOrigin.External, control: false, shift: false, DropEffect.Move);

        Assert.Equal(TransferVerb.Move, decision.Verb);
        Assert.Equal(DropEffect.Move, decision.Report);
    }

    /// <summary>The auto-scroll bit rides along on real drags and is not a verb; a raw comparison
    /// against Copy would miss a value carrying it.</summary>
    [Fact]
    public void TheScrollBitIsNotMistakenForAVerb()
    {
        var decision = DropInContract.Decide(
            DropOrigin.External, control: false, shift: false, Both | DropEffect.Scroll);

        Assert.Equal(TransferVerb.Copy, decision.Verb);
    }

    // --- What is worth accepting at all ---

    [Fact]
    public void ALinkOnlySourceIsRefused_ThisAppMakesNoShortcuts()
    {
        Assert.False(DropInContract.CanAccept(DropEffect.Link));
        Assert.False(DropInContract.CanAccept(DropEffect.None));
        Assert.True(DropInContract.CanAccept(DropEffect.Copy));
        Assert.True(DropInContract.CanAccept(DropEffect.Move));
    }
}
