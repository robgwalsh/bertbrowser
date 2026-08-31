using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Elevation;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Which failures are worth a UAC prompt, what the second pass is asked to do, and how the two
/// results become one. Pure: record literals in, record literals out, no disk and no prompt.
/// </summary>
/// <remarks>
/// The merge tests are the ones to keep honest. A merged outcome has to be indistinguishable from
/// one the executor produced alone, because the undo slot, the staging commit and the tab fan-out
/// all read it without knowing elevation happened — so an outcome that loses a staging folder loses
/// the user's displaced data for good, and one that keeps a failure the retry fixed reports a
/// problem that no longer exists.
/// </remarks>
public class ElevatedRetryTests
{
    private const string Dest = @"C:\dest";

    // --- is there anything a token could fix? ---

    [Fact]
    public void AnOutcomeWithOnlyOrdinaryFailuresIsNotRetryable()
    {
        var plan = MovePlan(@"C:\src\a.txt");
        var outcome = MoveOutcome(failed: [new FailedTransfer(@"C:\src\a.txt", "in use")]);

        Assert.Null(ElevatedRetry.RetryFor(plan, outcome));
    }

    [Fact]
    public void AnOutcomeWithNothingWrongIsNotRetryable() =>
        Assert.Null(ElevatedRetry.RetryFor(MovePlan(@"C:\src\a.txt"), MoveOutcome()));

    [Fact]
    public void ACancelledRunIsNotRetryableEvenWhenSomethingWasDenied()
    {
        // A cancel and a denial can coexist — item one refused, cancel pressed at item five. Putting
        // a consent prompt in front of somebody who has just pressed Cancel is the wrong answer
        // whatever else is true.
        var plan = MovePlan(@"C:\src\a.txt");
        var outcome = MoveOutcome(failed: [Denied(@"C:\src\a.txt")], cancelled: true);

        Assert.Null(ElevatedRetry.RetryFor(plan, outcome));
    }

    [Theory]
    [InlineData(Environment.SpecialFolder.Windows)]
    [InlineData(Environment.SpecialFolder.ProgramFiles)]
    [InlineData(Environment.SpecialFolder.UserProfile)]
    public void ElevationIsNeverOfferedForAProtectedLocation(Environment.SpecialFolder folder)
    {
        var path = Environment.GetFolderPath(folder);
        Assert.True(ElevationRules.IsRefusedForElevation(path));

        var plan = MovePlan(path);
        var outcome = MoveOutcome(failed: [Denied(path)]);

        Assert.Null(ElevatedRetry.RetryFor(plan, outcome));
    }

    [Fact]
    public void ElevationIsNeverOfferedForSomethingInsideTheRecycleBin() =>
        Assert.True(ElevationRules.IsRefusedForElevation(@"C:\$Recycle.Bin\S-1-5-21-1\$RAB1234.txt"));

    [Fact]
    public void AnOrdinaryFolderIsNotRefused() =>
        Assert.False(ElevationRules.IsRefusedForElevation(@"C:\Program Files\Vendor\app.exe"));

    // --- what the second pass is asked to do ---

    [Fact]
    public void TheRetryCarriesOnlyTheDeniedItems()
    {
        var plan = MovePlan(@"C:\src\a.txt", @"C:\src\b.txt", @"C:\src\c.txt");
        var outcome = MoveOutcome(failed:
        [
            Denied(@"C:\src\a.txt"),
            new FailedTransfer(@"C:\src\b.txt", "in use"),
        ]);

        var retry = Assert.IsType<TransferRetry>(ElevatedRetry.RetryFor(plan, outcome));

        Assert.Equal([@"C:\src\a.txt"], retry.Plan.Transfers.Select(t => t.SourcePath));
    }

    [Fact]
    public void TheRetryMatchesItemsByKeyRatherThanByString()
    {
        // The plan and the failure disagree about casing, which is what PathKey exists to settle.
        var plan = MovePlan(@"C:\src\A.txt");
        var outcome = MoveOutcome(failed: [Denied(@"c:\SRC\a.TXT")]);

        var retry = Assert.IsType<TransferRetry>(ElevatedRetry.RetryFor(plan, outcome));

        Assert.Single(retry.Plan.Transfers);
    }

    [Fact]
    public void TheRetryCarriesNoRejections()
    {
        var plan = new TransferPlan(
            TransferVerb.Move, Dest, [Planned(@"C:\src\a.txt")],
            [new RejectedTransfer(@"C:\src\z.txt", TransferRejection.MovesWithAncestor, "travels with its folder")]);
        var outcome = MoveOutcome(failed: [Denied(@"C:\src\a.txt")]);

        var retry = Assert.IsType<TransferRetry>(ElevatedRetry.RetryFor(plan, outcome));

        Assert.Empty(retry.Plan.Rejected);
    }

    [Fact]
    public void TheRetryKeepsTheConflictResolutionTheUserChose()
    {
        var plan = MovePlan(@"C:\src\a.txt");
        var outcome = MoveOutcome(failed: [Denied(@"C:\src\a.txt")]);
        var chosen = new Dictionary<string, ConflictResolution>(StringComparer.Ordinal)
        {
            [BertBrowser.Core.Paths.PathKey.Canonicalize(@"C:\src\a.txt")] = ConflictResolution.Replace,
        };

        var retry = Assert.IsType<TransferRetry>(ElevatedRetry.RetryFor(plan, outcome, chosen));

        Assert.Equal(ConflictResolution.Replace, Assert.Single(retry.Resolutions).Value);
    }

    [Fact]
    public void ADeleteRetryKeepsTheModeAndDropsTheItemsThatSucceeded()
    {
        var plan = new DeletePlan(
            DeleteMode.Recycle,
            [new PlannedDelete(@"C:\src\a.txt", false), new PlannedDelete(@"C:\src\b.txt", false)],
            []);
        var outcome = new DeleteOutcome(
            false, [], [new FailedDelete(@"C:\src\b.txt", "denied", AccessDenied: true)], []);

        var retry = Assert.IsType<DeleteRetry>(ElevatedRetry.RetryFor(plan, outcome));

        Assert.Equal(DeleteMode.Recycle, retry.Plan.Mode);
        Assert.Equal([@"C:\src\b.txt"], retry.Plan.Deletions.Select(d => d.SourcePath));
    }

    [Fact]
    public void AStrandedRenameIsRetriedFromWhereTheItemActuallyIs()
    {
        // The forward rename staged the item out from under itself, then failed, then could not put
        // it back. Retrying from the path the plan names would rename from somewhere that no longer
        // holds anything — and report success.
        var stranded = @"C:\src\.bertbrowser-rename-abc";
        var plan = new RenamePlan([new PlannedRename(@"C:\src\a.txt", @"C:\src\b.txt", false)], []);
        var outcome = new RenameOutcome([],
            [new FailedRename(@"C:\src\a.txt", "denied", AccessDenied: true, StrandedPath: stranded)]);

        var retry = Assert.IsType<RenameRetry>(ElevatedRetry.RetryFor(plan, outcome));

        Assert.Equal(stranded, Assert.Single(retry.Plan.Renames).SourcePath);
        Assert.Equal(@"C:\src\b.txt", retry.Plan.Renames[0].TargetPath);
    }

    [Fact]
    public void CreatingRetriesTheWholePlanBecauseThereIsOnlyEverOneItem()
    {
        var plan = new NewItemPlan(@"C:\Program Files\Vendor", "notes.txt", NewItemKind.File, null, null);
        var outcome = new NewItemOutcome(null, new FailedNewItem("denied", AccessDenied: true));

        var retry = Assert.IsType<NewItemRetry>(ElevatedRetry.RetryFor(plan, outcome));

        Assert.Equal(plan, retry.Plan);
    }

    [Fact]
    public void CreatingIsNotRetriedWhenTheNameWasSimplyTaken()
    {
        var plan = new NewItemPlan(@"C:\Program Files\Vendor", "notes.txt", NewItemKind.File, null, null);
        var outcome = new NewItemOutcome(null, new FailedNewItem("'notes.txt' already exists in this folder."));

        Assert.Null(ElevatedRetry.RetryFor(plan, outcome));
    }

    // --- folding the two results into one ---

    [Fact]
    public void MergingDropsTheFailureTheRetryFixed()
    {
        var plan = MovePlan(@"C:\src\a.txt");
        var first = MoveOutcome(failed: [Denied(@"C:\src\a.txt")]);
        var retry = ElevatedRetry.RetryFor(plan, first)!;
        var second = MoveOutcome(completed: [Completed(@"C:\src\a.txt")]);

        var merged = ElevatedRetry.Merge(first, retry, second);

        Assert.Empty(merged.Failed);
        Assert.Single(merged.Completed);
    }

    [Fact]
    public void MergingKeepsAFailureTheRetryCouldNotFix()
    {
        var plan = MovePlan(@"C:\src\a.txt");
        var first = MoveOutcome(failed: [Denied(@"C:\src\a.txt")]);
        var retry = ElevatedRetry.RetryFor(plan, first)!;
        var second = MoveOutcome(failed: [Denied(@"C:\src\a.txt")]);

        var merged = ElevatedRetry.Merge(first, retry, second);

        Assert.Single(merged.Failed);
    }

    [Fact]
    public void MergingLeavesAFailureNobodyRetriedAlone()
    {
        var plan = MovePlan(@"C:\src\a.txt", @"C:\src\b.txt");
        var first = MoveOutcome(failed:
        [
            Denied(@"C:\src\a.txt"),
            new FailedTransfer(@"C:\src\b.txt", "in use"),
        ]);
        var retry = ElevatedRetry.RetryFor(plan, first)!;
        var second = MoveOutcome(completed: [Completed(@"C:\src\a.txt")]);

        var merged = ElevatedRetry.Merge(first, retry, second);

        Assert.Equal([@"C:\src\b.txt"], merged.Failed.Select(f => f.SourcePath));
    }

    [Fact]
    public void MergingKeepsBothStagingFolders()
    {
        // The one whose real-world failure is data loss: a staging folder dropped here is never
        // committed and never purged, so what a Replace displaced stays hidden on disk for good.
        var plan = MovePlan(@"C:\src\a.txt");
        var first = MoveOutcome(failed: [Denied(@"C:\src\a.txt")], staging: [@"C:\dest\.bertbrowser-replaced-1"]);
        var retry = ElevatedRetry.RetryFor(plan, first)!;
        var second = MoveOutcome(
            completed: [Completed(@"C:\src\a.txt")], staging: [@"C:\dest\.bertbrowser-replaced-2"]);

        var merged = ElevatedRetry.Merge(first, retry, second);

        Assert.Equal(2, merged.StagingDirectories.Count);
    }

    [Fact]
    public void AMergedOutcomeIsStillUndoable()
    {
        var plan = MovePlan(@"C:\src\a.txt");
        var first = MoveOutcome(failed: [Denied(@"C:\src\a.txt")]);
        var retry = ElevatedRetry.RetryFor(plan, first)!;
        var second = MoveOutcome(completed: [Completed(@"C:\src\a.txt")]);

        Assert.True(ElevatedRetry.Merge(first, retry, second).CanUndo);
    }

    [Fact]
    public void MergingRefusesTwoPassesThatDisagreeAboutTheOperation()
    {
        var plan = MovePlan(@"C:\src\a.txt");
        var first = MoveOutcome(failed: [Denied(@"C:\src\a.txt")]);
        var retry = ElevatedRetry.RetryFor(plan, first)!;
        var second = new TransferOutcome(TransferVerb.Copy, Dest, [], [], [], []);

        Assert.Throws<ArgumentException>(() => ElevatedRetry.Merge(first, retry, second));
    }

    [Fact]
    public void AMergedRenameReportsTheOriginalSourceOfAStrandedItem()
    {
        // Undo is this same execution with every path swapped, so a completed record naming the
        // staging path would put the item back under the staging name rather than where it came from.
        var stranded = @"C:\src\.bertbrowser-rename-abc";
        var plan = new RenamePlan([new PlannedRename(@"C:\src\a.txt", @"C:\src\b.txt", false)], []);
        var first = new RenameOutcome([],
            [new FailedRename(@"C:\src\a.txt", "denied", AccessDenied: true, StrandedPath: stranded)]);
        var retry = ElevatedRetry.RetryFor(plan, first)!;
        var second = new RenameOutcome([new CompletedRename(stranded, @"C:\src\b.txt", false)], []);

        var merged = ElevatedRetry.Merge(first, retry, second);

        Assert.Equal(@"C:\src\a.txt", Assert.Single(merged.Completed).SourcePath);
        Assert.Empty(merged.Failed);
    }

    [Fact]
    public void MergingDeletesKeepsBothHoldingFolders()
    {
        var plan = new DeletePlan(DeleteMode.Staged, [new PlannedDelete(@"C:\src\a.txt", false)], []);
        var first = new DeleteOutcome(
            false, [], [new FailedDelete(@"C:\src\a.txt", "denied", AccessDenied: true)], [@"C:\.bertbrowser-trash\delete-1"]);
        var retry = ElevatedRetry.RetryFor(plan, first)!;
        var second = new DeleteOutcome(
            false, [new DeletedItem(@"C:\src\a.txt", false, @"C:\.bertbrowser-trash\delete-2\a.txt")], [],
            [@"C:\.bertbrowser-trash\delete-2"]);

        var merged = ElevatedRetry.Merge(first, retry, second);

        Assert.Equal(2, merged.StagingDirectories.Count);
        Assert.Empty(merged.Failed);
        Assert.True(merged.CanUndo);
    }

    // --- putting things back ---

    [Fact]
    public void AnUndoRetryCarriesOnlyTheItemsThatCouldNotBePutBack()
    {
        var outcome = MoveOutcome(completed: [Completed(@"C:\src\a.txt"), Completed(@"C:\src\b.txt")]);
        var result = new TransferUndoResult(1, [Denied(@"C:\src\b.txt")]);

        var retry = Assert.IsType<TransferUndoRetry>(ElevatedRetry.UndoRetryFor(outcome, result));

        Assert.Equal([@"C:\src\b.txt"], retry.Outcome.Completed.Select(c => c.SourcePath));
    }

    [Fact]
    public void AnUndoRetryCarriesNoStagingFolders()
    {
        // The unelevated half still holds items in its own, and the elevated pass's cleanup has no
        // business running over them.
        var outcome = MoveOutcome(
            completed: [Completed(@"C:\src\a.txt")], staging: [@"C:\dest\.bertbrowser-replaced-1"]);
        var result = new TransferUndoResult(0, [Denied(@"C:\src\a.txt")]);

        var retry = Assert.IsType<TransferUndoRetry>(ElevatedRetry.UndoRetryFor(outcome, result));

        Assert.Empty(retry.Outcome.StagingDirectories);
    }

    [Fact]
    public void AnUndoThatFailedForSomeOtherReasonIsNotRetried()
    {
        var outcome = MoveOutcome(completed: [Completed(@"C:\src\a.txt")]);
        var result = new TransferUndoResult(0, [new FailedTransfer(@"C:\src\a.txt", "it is no longer there")]);

        Assert.Null(ElevatedRetry.UndoRetryFor(outcome, result));
    }

    [Fact]
    public void MergingAnUndoAddsTheCountsAndDropsWhatTheRetryFixed()
    {
        var outcome = MoveOutcome(completed: [Completed(@"C:\src\a.txt"), Completed(@"C:\src\b.txt")]);
        var first = new TransferUndoResult(1, [Denied(@"C:\src\b.txt")]);
        var retry = ElevatedRetry.UndoRetryFor(outcome, first)!;
        var second = new TransferUndoResult(1, []);

        var merged = ElevatedRetry.Merge(first, retry, second);

        Assert.Equal(2, merged.Restored);
        Assert.Empty(merged.Failed);
    }

    [Fact]
    public void MergingADeleteUndoWorksTheSameWay()
    {
        var outcome = new DeleteOutcome(
            false,
            [new DeletedItem(@"C:\src\a.txt", false, null, @"C:\$Recycle.Bin\S-1-5-21-1\$RAA.txt")],
            [],
            []);
        var first = new DeleteUndoResult(0, [new FailedDelete(@"C:\src\a.txt", "denied", AccessDenied: true)]);
        var retry = ElevatedRetry.UndoRetryFor(outcome, first)!;
        var second = new DeleteUndoResult(1, []);

        var merged = ElevatedRetry.Merge(first, retry, second);

        Assert.Equal(1, merged.Restored);
        Assert.Empty(merged.Failed);
    }

    // --- helpers ---

    private static PlannedTransfer Planned(string source) =>
        new(source, false, Path.Combine(Dest, Path.GetFileName(source)), false);

    private static CompletedTransfer Completed(string source) =>
        new(source, Path.Combine(Dest, Path.GetFileName(source)), false, null);

    private static FailedTransfer Denied(string source) =>
        new(source, "Access to the path is denied.", AccessDenied: true);

    private static TransferPlan MovePlan(params string[] sources) =>
        new(TransferVerb.Move, Dest, [.. sources.Select(Planned)], []);

    private static TransferOutcome MoveOutcome(
        IReadOnlyList<CompletedTransfer>? completed = null,
        IReadOnlyList<FailedTransfer>? failed = null,
        IReadOnlyList<string>? staging = null,
        bool cancelled = false) =>
        new(TransferVerb.Move, Dest, completed ?? [], [], failed ?? [], staging ?? [], cancelled);
}
