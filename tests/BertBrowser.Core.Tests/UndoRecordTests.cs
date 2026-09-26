using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Compare;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;
using BertBrowser.Core.Services.UndoHistory;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Every kind of record taken undo → redo → undo against real folders. Each test asserts on file
/// <em>contents</em>, following <see cref="TransferExecutorTests"/>: a redo that restores the right
/// names over the wrong bytes is exactly the failure that would otherwise pass.
/// </summary>
public sealed class UndoRecordTests : IDisposable
{
    private readonly string _root;
    private readonly TransferPlanner _planner = new();
    private readonly TransferExecutor _transfers = new();
    private readonly DeleteExecutor _deletes;
    private readonly IUndoHost _host;

    public UndoRecordTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-undo-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
        _deletes = new DeleteExecutor(new FileSystemDeleteProbe(), [], stagingRoot: _root);
        _host = new PlainUndoHost(
            _transfers, _deletes, new RenameExecutor(), new ArchiveEditExecutor(new SharpCompressArchiveReader()));
    }

    public void Dispose()
    {
        try
        {
            Directory.Delete(_root, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    // --- helpers ---

    private string Dir(params string[] parts)
    {
        var path = Path.Combine([_root, .. parts]);
        Directory.CreateDirectory(path);
        return path;
    }

    private string File_(string content, params string[] parts)
    {
        var path = Path.Combine([_root, .. parts]);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content);
        return path;
    }

    private string P(params string[] parts) => Path.Combine([_root, .. parts]);

    private static void AssertContent(string path, string expected)
    {
        Assert.True(File.Exists(path), $"expected a file at {path}");
        Assert.Equal(expected, File.ReadAllText(path));
    }

    private TransferOutcome Transfer(
        TransferVerb verb, string destination, ConflictResolution resolution, params string[] sources)
    {
        var plan = _planner.Plan(sources, destination, verb);
        var resolutions = plan.Transfers.ToDictionary(t => PathKey.Canonicalize(t.SourcePath), _ => resolution);
        return _transfers.Execute(plan, resolutions);
    }

    /// <summary>Takes one step and checks it was clean.</summary>
    private async Task<IUndoableRecord> Step(IUndoableRecord record)
    {
        var result = await record.StepAsync(_host);
        Assert.True(result.Report.Clean, string.Join("; ", result.Report.Failures));
        Assert.NotEqual(record.IsApplied, result.Next.IsApplied);
        return result.Next;
    }

    // --- move ---

    [Fact]
    public async Task AMove_UndoRedoUndo_EndsWhereItStarted()
    {
        File_("a", "src", "a.txt");
        File_("b", "src", "sub", "b.txt");
        Dir("dst");

        var record = TransferRecord.For(Transfer(TransferVerb.Move, P("dst"), ConflictResolution.KeepBoth,
            P("src", "a.txt"), P("src", "sub")))!;

        var undone = await Step(record);
        AssertContent(P("src", "a.txt"), "a");
        AssertContent(P("src", "sub", "b.txt"), "b");

        var redone = await Step(undone);
        Assert.False(File.Exists(P("src", "a.txt")));
        AssertContent(P("dst", "a.txt"), "a");
        AssertContent(P("dst", "sub", "b.txt"), "b");

        await Step(redone);
        AssertContent(P("src", "a.txt"), "a");
        Assert.False(Directory.Exists(P("dst", "sub")));
    }

    /// <summary>
    /// A Replace is redone as a Replace: the entry the undo put back is set aside again, not
    /// numbered past, and comes back once more on the second undo.
    /// </summary>
    [Fact]
    public async Task AReplacingMove_ReplacesAgainOnRedo_AndRestoresAgainOnUndo()
    {
        File_("new", "src", "a.txt");
        File_("old", "dst", "a.txt");

        var record = TransferRecord.For(
            Transfer(TransferVerb.Move, P("dst"), ConflictResolution.Replace, P("src", "a.txt")))!;
        Assert.Single(record.Held);

        var undone = await Step(record);
        AssertContent(P("dst", "a.txt"), "old");
        AssertContent(P("src", "a.txt"), "new");
        Assert.Empty(undone.Held);

        var redone = await Step(undone);
        AssertContent(P("dst", "a.txt"), "new");
        Assert.Single(redone.Held);
        Assert.False(File.Exists(P("dst", "a (2).txt")));

        await Step(redone);
        AssertContent(P("dst", "a.txt"), "old");
        AssertContent(P("src", "a.txt"), "new");
    }

    /// <summary>A Keep-both lands under the same number again, not a fresh one.</summary>
    [Fact]
    public async Task AKeepBothMove_RedoesToTheSameName()
    {
        File_("new", "src", "a.txt");
        File_("old", "dst", "a.txt");

        var record = TransferRecord.For(
            Transfer(TransferVerb.Move, P("dst"), ConflictResolution.KeepBoth, P("src", "a.txt")))!;
        AssertContent(P("dst", "a (2).txt"), "new");

        await Step(await Step(record));

        AssertContent(P("dst", "a (2).txt"), "new");
        AssertContent(P("dst", "a.txt"), "old");
        Assert.False(File.Exists(P("dst", "a (3).txt")));
    }

    /// <summary>
    /// A name taken since the undo is nobody's choice to replace: the redo leaves it, reports it,
    /// and carries on with the rest.
    /// </summary>
    [Fact]
    public async Task ARedo_LeavesANewcomerAlone_AndReportsIt()
    {
        File_("a", "src", "a.txt");
        File_("b", "src", "b.txt");
        Dir("dst");

        var record = TransferRecord.For(Transfer(TransferVerb.Move, P("dst"), ConflictResolution.KeepBoth,
            P("src", "a.txt"), P("src", "b.txt")))!;
        var undone = await Step(record);
        File_("newcomer", "dst", "a.txt");

        var result = await undone.StepAsync(_host);

        Assert.Equal(StepVerdict.Partial, result.Report.Verdict);
        AssertContent(P("dst", "a.txt"), "newcomer");
        AssertContent(P("src", "a.txt"), "a");
        AssertContent(P("dst", "b.txt"), "b");
        Assert.Equal(1, result.Next.ItemCount);
    }

    /// <summary>
    /// After a partial undo, the redo acts on what went back and nothing else — the item that
    /// could not be put back is not moved a second time.
    /// </summary>
    [Fact]
    public async Task APartialUndo_IsRedoneOnlyForWhatWentBack()
    {
        File_("a", "src", "a.txt");
        File_("b", "src", "b.txt");
        Dir("dst");

        var record = TransferRecord.For(Transfer(TransferVerb.Move, P("dst"), ConflictResolution.KeepBoth,
            P("src", "a.txt"), P("src", "b.txt")))!;
        File_("in the way", "src", "a.txt");

        var result = await record.StepAsync(_host);
        Assert.Equal(StepVerdict.Partial, result.Report.Verdict);
        Assert.Equal(1, result.Next.ItemCount);

        await Step(result.Next);
        AssertContent(P("dst", "a.txt"), "a");
        AssertContent(P("dst", "b.txt"), "b");
        AssertContent(P("src", "a.txt"), "in the way");
    }

    [Fact]
    public async Task AMergedMove_RecreatesAndPrunesItsFoldersEachWay()
    {
        File_("a", "src", "Photos", "a.jpg");
        File_("old", "dst", "Photos", "b.jpg");
        File_("b", "src", "Photos", "b.jpg");

        var plan = new TransferMergeExpander().Expand(_planner.Plan([P("src", "Photos")], P("dst"), TransferVerb.Move));
        var resolutions = plan.Transfers.ToDictionary(t => PathKey.Canonicalize(t.SourcePath), _ => ConflictResolution.Replace);
        var record = TransferRecord.For(_transfers.Execute(plan, resolutions))!;
        Assert.False(Directory.Exists(P("src", "Photos")));

        var undone = await Step(record);
        AssertContent(P("src", "Photos", "a.jpg"), "a");
        AssertContent(P("dst", "Photos", "b.jpg"), "old");

        var redone = await Step(undone);
        Assert.False(Directory.Exists(P("src", "Photos")));
        AssertContent(P("dst", "Photos", "b.jpg"), "b");

        await Step(redone);
        AssertContent(P("src", "Photos", "b.jpg"), "b");
        AssertContent(P("dst", "Photos", "b.jpg"), "old");
    }

    // --- paste ---

    private TransferRecord Paste(ConflictResolution resolution, params string[] sources) =>
        TransferRecord.For(Transfer(TransferVerb.Copy, P("dst"), resolution, sources) with { CanUndoCopy = true })!;

    [Fact]
    public async Task AnOverwritingPaste_UndoRedoUndo_KeepsTheOriginal()
    {
        File_("new", "src", "a.txt");
        File_("old", "dst", "a.txt");

        var record = Paste(ConflictResolution.Overwrite, P("src", "a.txt"));
        AssertContent(P("dst", "a.txt"), "new");

        var undone = await Step(record);
        AssertContent(P("dst", "a.txt"), "old");

        var redone = await Step(undone);
        AssertContent(P("dst", "a.txt"), "new");

        await Step(redone);
        AssertContent(P("dst", "a.txt"), "old");
        AssertContent(P("src", "a.txt"), "new");
    }

    /// <summary>
    /// The history can reach a paste long after it happened; a copy edited since is the user's
    /// work now, and is left in place rather than removed with the paste.
    /// </summary>
    [Fact]
    public async Task APasteUndo_LeavesACopyThatWasEditedSince()
    {
        File_("new", "src", "a.txt");
        Dir("dst");
        var record = Paste(ConflictResolution.KeepBoth, P("src", "a.txt"));
        File.WriteAllText(P("dst", "a.txt"), "edited afterwards");
        File.SetLastWriteTimeUtc(P("dst", "a.txt"), DateTime.UtcNow.AddMinutes(5));

        var result = await record.StepAsync(_host);

        Assert.Equal(StepVerdict.Failed, result.Report.Verdict);
        AssertContent(P("dst", "a.txt"), "edited afterwards");
    }

    // --- UndoCopies itself ---

    /// <summary>
    /// The bug the history would otherwise have walked into: undoing the same copy twice deleted
    /// the original the first call had just put back. The second call must now touch nothing.
    /// </summary>
    [Fact]
    public void UndoCopiesTwice_LeavesTheRestoredOriginalAlone()
    {
        File_("new", "src", "a.txt");
        File_("old", "dst", "a.txt");
        var outcome = Transfer(TransferVerb.Copy, P("dst"), ConflictResolution.Overwrite, P("src", "a.txt"));

        var first = _transfers.UndoCopies(outcome);
        var second = _transfers.UndoCopies(outcome);

        Assert.Equal(1, first.Restored);
        Assert.Equal(0, second.Restored);
        Assert.Single(second.Failed);
        AssertContent(P("dst", "a.txt"), "old");
    }

    [Fact]
    public void UndoCopies_DoesNotCountWhatWasAlreadyGone()
    {
        File_("new", "src", "a.txt");
        Dir("dst");
        var outcome = Transfer(TransferVerb.Copy, P("dst"), ConflictResolution.KeepBoth, P("src", "a.txt"));
        File.Delete(P("dst", "a.txt"));

        var result = _transfers.UndoCopies(outcome);

        Assert.Equal(0, result.Restored);
        Assert.Empty(result.Reverted);
    }

    // --- delete ---

    [Fact]
    public async Task AStagedDelete_UndoRedoUndo_PutsTheTreeBack()
    {
        File_("a", "work", "a.txt");
        File_("b", "work", "sub", "b.txt");

        var plan = new DeletePlan(DeleteMode.Staged,
            [new PlannedDelete(P("work", "a.txt"), false), new PlannedDelete(P("work", "sub"), true)], []);
        var record = DeleteRecord.For(_deletes.Execute(plan))!;
        Assert.Equal(2, record.Held.Count);

        var undone = await Step(record);
        AssertContent(P("work", "sub", "b.txt"), "b");

        var redone = await Step(undone);
        Assert.False(File.Exists(P("work", "a.txt")));
        Assert.False(Directory.Exists(P("work", "sub")));

        await Step(redone);
        AssertContent(P("work", "a.txt"), "a");
        AssertContent(P("work", "sub", "b.txt"), "b");
    }

    [Fact]
    public async Task ARecycledDelete_IsRedoneIntoTheBin()
    {
        var bin = new FakeRecycleBin(Dir("bin"));
        var deletes = new DeleteExecutor(new FileSystemDeleteProbe(), [], stagingRoot: _root, recycleBin: bin, recycleProbe: bin);
        var host = new PlainUndoHost(_transfers, deletes, new RenameExecutor(), new ArchiveEditExecutor(new SharpCompressArchiveReader()));
        File_("a", "work", "a.txt");

        var plan = new DeletePlan(DeleteMode.Recycle, [new PlannedDelete(P("work", "a.txt"), false, DeleteDisposition.Recycle)], []);
        var record = DeleteRecord.For(deletes.Execute(plan))!;
        Assert.Empty(record.Held); // the bin holds it, not us

        var undone = (await record.StepAsync(host)).Next;
        AssertContent(P("work", "a.txt"), "a");

        var redone = await redoStep(undone);
        Assert.False(File.Exists(P("work", "a.txt")));
        Assert.Empty(redone.Held);

        await redone.StepAsync(host);
        AssertContent(P("work", "a.txt"), "a");

        async Task<IUndoableRecord> redoStep(IUndoableRecord r) => (await r.StepAsync(host)).Next;
    }

    // --- rename ---

    [Fact]
    public async Task ARename_UndoRedoUndo_TracksTheNames()
    {
        File_("a", "a.txt");
        File_("b", "b.txt");
        var plan = new RenamePlan(
            [new PlannedRename(P("a.txt"), P("b.txt"), false), new PlannedRename(P("b.txt"), P("a.txt"), false)], []);
        var record = RenameRecord.For(new RenameExecutor().Execute(plan))!;
        AssertContent(P("a.txt"), "b");

        var undone = await Step(record);
        AssertContent(P("a.txt"), "a");
        Assert.Equal(P("a.txt"), undone.Details.Single(d => d.To == P("b.txt")).From);

        var redone = await Step(undone);
        AssertContent(P("a.txt"), "b");

        await Step(redone);
        AssertContent(P("a.txt"), "a");
        AssertContent(P("b.txt"), "b");
    }

    // --- sync ---

    [Fact]
    public async Task ASync_UndoRedoUndo_PutsTheRightSideBackEachTime()
    {
        File_("new", "left", "a.txt");
        File_("old", "right", "a.txt");
        File_("extra", "right", "gone.txt");

        var copy = _planner.Plan([P("left", "a.txt")], P("right"), TransferVerb.Copy);
        var plans = new SyncPlans(
            [copy],
            copy.Transfers.ToDictionary(t => PathKey.Canonicalize(t.SourcePath), _ => ConflictResolution.Overwrite),
            new DeletePlan(DeleteMode.Staged, [new PlannedDelete(P("right", "gone.txt"), false)], []),
            []);
        var record = SyncRecord.For(new SyncRunner(_transfers, _deletes).Run(plans))!;
        AssertContent(P("right", "a.txt"), "new");
        Assert.False(File.Exists(P("right", "gone.txt")));

        var undone = await Step(record);
        AssertContent(P("right", "a.txt"), "old");
        AssertContent(P("right", "gone.txt"), "extra");

        var redone = await Step(undone);
        AssertContent(P("right", "a.txt"), "new");
        Assert.False(File.Exists(P("right", "gone.txt")));

        await Step(redone);
        AssertContent(P("right", "a.txt"), "old");
        AssertContent(P("right", "gone.txt"), "extra");
    }

    // --- the history over real records ---

    /// <summary>
    /// Releasing an entry to stay within the budget is what commits its data — the oldest Replace's
    /// displaced file goes, the newer one's is still held.
    /// </summary>
    [Fact]
    public void ReleasingAnEntryCommitsItsStagingAndNoOneElses()
    {
        File_("new1", "src", "one.txt");
        File_("old1", "dst", "one.txt");
        File_("new2", "src", "two.txt");
        File_("old2", "dst", "two.txt");
        var stack = new UndoStack(new UndoBudget(1, 1L << 40));

        var first = TransferRecord.For(Transfer(TransferVerb.Move, P("dst"), ConflictResolution.Replace, P("src", "one.txt")))!;
        stack.Push(first, HeldSize.None, DateTime.UtcNow);
        var second = TransferRecord.For(Transfer(TransferVerb.Move, P("dst"), ConflictResolution.Replace, P("src", "two.txt")))!;
        var (_, released) = stack.Push(second, HeldSize.None, DateTime.UtcNow);

        released.RetireAll();

        Assert.False(File.Exists(first.Held.Single().HeldPath));
        AssertContent(second.Held.Single().HeldPath, "old2");
    }

    [Fact]
    public void HeldSize_CountsFilesExactly_AndWalksAnUnindexedFolder()
    {
        File_("12345", "held", "a.txt");
        File_("123", "held", "dir", "b.txt");

        var estimator = new HeldSizeEstimator();

        Assert.Equal(new HeldSize(5, true), estimator.Measure(new HeldEntry(P("held", "a.txt"), "", false)));
        Assert.Equal(new HeldSize(8, true), estimator.Measure(new HeldEntry(P("held"), P("orig"), true)));
    }

    [Fact]
    public void HeldSize_TakesTheIndexAnswerForAFolderWhenThereIsOne()
    {
        Dir("held");
        var estimator = new HeldSizeEstimator(new FixedSizes(1234));

        Assert.Equal(new HeldSize(1234, true), estimator.Measure(new HeldEntry(P("held"), P("orig"), true)));
    }

    private sealed class FixedSizes(long size) : IHeldFolderSizes
    {
        public long? Of(string originalPath) => size;
    }
}
