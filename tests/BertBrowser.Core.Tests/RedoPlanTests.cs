using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The plans a redo runs, built from what an undo put back. Pure: the rules here are what keep a
/// redo from displacing anything nobody chose to replace.
/// </summary>
public sealed class RedoPlanTests
{
    private static CompletedTransfer Item(string name, bool displaced = false) =>
        new($@"C:\src\{name}", $@"C:\dst\{name}", false, displaced ? $@"C:\dst\.bertbrowser-replaced-1\{name}" : null);

    [Fact]
    public void EachItemLandsWhereItLandedBefore()
    {
        var (plan, _) = TransferRedo.PlanFor(TransferVerb.Move, @"C:\dst", [Item("a (2).txt")], []);

        Assert.Equal(@"C:\dst\a (2).txt", plan.Transfers.Single().DestinationPath);
    }

    [Fact]
    public void OnlyWhatWasDisplacedIsDisplacedAgain_AsAReplaceForAMove()
    {
        var (_, resolutions) = TransferRedo.PlanFor(
            TransferVerb.Move, @"C:\dst", [Item("a.txt", displaced: true), Item("b.txt")], []);

        Assert.Equal(ConflictResolution.Replace, resolutions[PathKey.Canonicalize(@"C:\src\a.txt")]);
        Assert.Equal(ConflictResolution.Skip, resolutions[PathKey.Canonicalize(@"C:\src\b.txt")]);
    }

    [Fact]
    public void ACopyDisplacesWithOverwrite_SinceACopyNeverReplaces()
    {
        var (_, resolutions) = TransferRedo.PlanFor(TransferVerb.Copy, @"C:\dst", [Item("a.txt", displaced: true)], []);

        Assert.Equal(ConflictResolution.Overwrite, resolutions.Single().Value);
    }

    [Fact]
    public void TheOriginalOrderComesBack_SoAFolderIsWrittenBeforeItsContents()
    {
        // An undo walks newest first, so that is the order its list arrives in.
        var (plan, _) = TransferRedo.PlanFor(TransferVerb.Copy, @"C:\dst", [Item(@"dir\a.txt"), Item("dir")], []);

        Assert.Equal([@"C:\src\dir", @"C:\src\dir\a.txt"], plan.Transfers.Select(t => t.SourcePath));
    }

    [Fact]
    public void OnlyAMovePrunes()
    {
        string[] pruned = [@"C:\src\Photos"];

        Assert.Equal(pruned, TransferRedo.PlanFor(TransferVerb.Move, @"C:\dst", [Item("a.txt")], pruned).Plan.PruneDirectories);
        Assert.Empty(TransferRedo.PlanFor(TransferVerb.Copy, @"C:\dst", [Item("a.txt")], pruned).Plan.PruneDirectories);
    }

    [Fact]
    public void ADeleteGoesBackTheWayItWent()
    {
        var plan = DeleteRedo.PlanFor(
        [
            new DeletedItem(@"C:\w\held.txt", false, @"C:\.bertbrowser-trash\delete-1\held.txt"),
            new DeletedItem(@"C:\w\binned.txt", false, null, @"C:\$Recycle.Bin\S-1\$R1.txt"),
        ]);

        Assert.Equal(DeleteMode.Recycle, plan.Mode);
        Assert.False(plan.Permanent);
        Assert.Equal(DeleteDisposition.Recycle, plan.Deletions.Single(d => d.Name == "binned.txt").Disposition);
        Assert.Equal(DeleteDisposition.Stage, plan.Deletions.Single(d => d.Name == "held.txt").Disposition);
    }

    [Fact]
    public void ADeleteThatWasOnlyHeldIsRedoneAsHeld()
    {
        var plan = DeleteRedo.PlanFor([new DeletedItem(@"C:\w\a.txt", false, @"C:\t\a.txt")]);

        Assert.Equal(DeleteMode.Staged, plan.Mode);
    }
}
