using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Executes real transfers against real files. Every test asserts on file <em>contents</em>, not
/// just existence, so a transfer that loses or truncates data fails loudly.
/// </summary>
public sealed class TransferExecutorTests : IDisposable
{
    private readonly string _root;
    private readonly TransferPlanner _planner = new();
    private readonly TransferExecutor _executor = new();

    public TransferExecutorTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-xfer-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
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

    private TransferOutcome Run(
        string[] sources, string destination, TransferVerb verb = TransferVerb.Move,
        ConflictResolution resolution = ConflictResolution.KeepBoth)
    {
        var plan = _planner.Plan(sources, destination, verb);
        var resolutions = plan.Transfers.ToDictionary(
            t => PathKey.Canonicalize(t.SourcePath), _ => resolution);
        return _executor.Execute(plan, resolutions);
    }

    private static void AssertContent(string path, string expected)
    {
        Assert.True(File.Exists(path), $"expected a file at {path}");
        Assert.Equal(expected, File.ReadAllText(path));
    }

    // --- moves that should just work ---

    [Fact]
    public void MoveFile_LandsAtDestination_AndLeavesTheSource()
    {
        var source = File_("hello", "src", "a.txt");
        var dest = Dir("dest");

        var outcome = Run([source], dest);

        Assert.Empty(outcome.Failed);
        AssertContent(P("dest", "a.txt"), "hello");
        Assert.False(File.Exists(source));
        Assert.Equal(P("dest", "a.txt"), outcome.Completed.Single().FinalPath);
    }

    [Fact]
    public void MoveDirectory_TakesTheWholeTree()
    {
        File_("one", "src", "tree", "a.txt");
        File_("two", "src", "tree", "sub", "b.txt");
        File_("three", "src", "tree", "sub", "deep", "c.txt");
        var dest = Dir("dest");

        var outcome = Run([P("src", "tree")], dest);

        Assert.Empty(outcome.Failed);
        AssertContent(P("dest", "tree", "a.txt"), "one");
        AssertContent(P("dest", "tree", "sub", "b.txt"), "two");
        AssertContent(P("dest", "tree", "sub", "deep", "c.txt"), "three");
        Assert.False(Directory.Exists(P("src", "tree")));
    }

    [Fact]
    public void MoveManyItems_AllArrive()
    {
        var a = File_("a", "src", "a.txt");
        var b = File_("b", "src", "b.txt");
        File_("c", "src", "folder", "c.txt");
        var dest = Dir("dest");

        var outcome = Run([a, b, P("src", "folder")], dest);

        Assert.Equal(3, outcome.Completed.Count);
        AssertContent(P("dest", "a.txt"), "a");
        AssertContent(P("dest", "b.txt"), "b");
        AssertContent(P("dest", "folder", "c.txt"), "c");
    }

    [Fact]
    public void NestedSelection_MovesOnceAndKeepsEverything()
    {
        // Selecting a folder and something inside it must not try to move the inner item twice.
        File_("deep", "src", "tree", "inner", "leaf.txt");
        var dest = Dir("dest");

        var outcome = Run([P("src", "tree"), P("src", "tree", "inner")], dest);

        Assert.Single(outcome.Completed);
        Assert.Empty(outcome.Failed);
        AssertContent(P("dest", "tree", "inner", "leaf.txt"), "deep");
    }

    // --- conflicts ---

    [Fact]
    public void KeepBoth_NumbersTheNewcomer_AndLeavesTheExistingFileAlone()
    {
        var source = File_("new", "src", "a.txt");
        var dest = Dir("dest");
        File_("existing", "dest", "a.txt");

        var outcome = Run([source], dest, resolution: ConflictResolution.KeepBoth);

        AssertContent(P("dest", "a.txt"), "existing");
        AssertContent(P("dest", "a (2).txt"), "new");
        Assert.Equal(P("dest", "a (2).txt"), outcome.Completed.Single().FinalPath);
    }

    [Fact]
    public void KeepBoth_NumbersDirectoriesAfterTheWholeName()
    {
        File_("new", "src", "tree", "x.txt");
        Dir("dest", "tree");

        Run([P("src", "tree")], P("dest"), resolution: ConflictResolution.KeepBoth);

        AssertContent(P("dest", "tree (2)", "x.txt"), "new");
    }

    [Fact]
    public void Skip_LeavesBothSidesUntouched()
    {
        var source = File_("new", "src", "a.txt");
        var dest = Dir("dest");
        File_("existing", "dest", "a.txt");

        var outcome = Run([source], dest, resolution: ConflictResolution.Skip);

        AssertContent(P("dest", "a.txt"), "existing");
        AssertContent(source, "new");
        Assert.Empty(outcome.Completed);
        Assert.Single(outcome.Skipped);
    }

    [Fact]
    public void Replace_StagesTheDisplacedFile_InsteadOfDeletingIt()
    {
        var source = File_("new", "src", "a.txt");
        var dest = Dir("dest");
        File_("existing", "dest", "a.txt");

        var outcome = Run([source], dest, resolution: ConflictResolution.Replace);

        AssertContent(P("dest", "a.txt"), "new");
        Assert.False(File.Exists(source));

        // The displaced content must still be on disk, not gone.
        var staged = TransferExecutor.StagedItems(outcome);
        AssertContent(staged.Single(), "existing");
    }

    [Fact]
    public void Replace_IsDowngradedToKeepBoth_ForACopy()
    {
        // Copy is defined as purely additive; it must never displace anything.
        var source = File_("new", "src", "a.txt");
        var dest = Dir("dest");
        File_("existing", "dest", "a.txt");

        Run([source], dest, TransferVerb.Copy, ConflictResolution.Replace);

        AssertContent(P("dest", "a.txt"), "existing");
        AssertContent(P("dest", "a (2).txt"), "new");
        AssertContent(source, "new");
    }

    [Fact]
    public void Replace_ThatFailsAfterStaging_PutsTheDisplacedFileBack()
    {
        // Clearing the name succeeds, then the move of the locked source fails. The displaced file
        // must return to its own name rather than being stranded in staging with no undo record.
        var source = File_("new", "src", "a.txt");
        var dest = Dir("dest");
        File_("existing", "dest", "a.txt");

        TransferOutcome outcome;
        using (File.Open(source, FileMode.Open, FileAccess.Read, FileShare.None))
            outcome = Run([source], dest, resolution: ConflictResolution.Replace);

        Assert.Single(outcome.Failed);
        Assert.Empty(outcome.Completed);
        AssertContent(P("dest", "a.txt"), "existing"); // back under its own name
        AssertContent(source, "new");                  // and the source never left
        Assert.Empty(TransferExecutor.StagedItems(outcome));
    }

    [Fact]
    public void TwoSourcesWithTheSameName_BothSurvive()
    {
        var one = File_("first", "one", "a.txt");
        var two = File_("second", "two", "a.txt");
        var dest = Dir("dest");

        Run([one, two], dest, resolution: ConflictResolution.KeepBoth);

        AssertContent(P("dest", "a.txt"), "first");
        AssertContent(P("dest", "a (2).txt"), "second");
    }

    // --- undo ---

    [Fact]
    public void Undo_PutsEveryItemBackWhereItCameFrom()
    {
        var a = File_("a", "src", "a.txt");
        var b = File_("b", "src", "b.txt");
        File_("c", "src", "folder", "c.txt");
        var dest = Dir("dest");

        var outcome = Run([a, b, P("src", "folder")], dest);
        Assert.True(outcome.CanUndo);

        var undo = _executor.Undo(outcome);

        Assert.Empty(undo.Failed);
        Assert.Equal(3, undo.Restored);
        AssertContent(a, "a");
        AssertContent(b, "b");
        AssertContent(P("src", "folder", "c.txt"), "c");
        Assert.False(File.Exists(P("dest", "a.txt")));
        Assert.False(Directory.Exists(P("dest", "folder")));
    }

    [Fact]
    public void Undo_RestoresBothSidesOfAReplace()
    {
        var source = File_("new", "src", "a.txt");
        var dest = Dir("dest");
        File_("existing", "dest", "a.txt");

        var outcome = Run([source], dest, resolution: ConflictResolution.Replace);
        var undo = _executor.Undo(outcome);

        Assert.Empty(undo.Failed);
        AssertContent(source, "new");              // the mover went home
        AssertContent(P("dest", "a.txt"), "existing"); // the displaced file came back
        Assert.Empty(TransferExecutor.StagedItems(outcome));
        Assert.False(Directory.Exists(outcome.StagingDirectory!));
    }

    [Fact]
    public void Undo_RestoresARenamedItemToItsOriginalName()
    {
        var source = File_("new", "src", "a.txt");
        var dest = Dir("dest");
        File_("existing", "dest", "a.txt");

        var outcome = Run([source], dest, resolution: ConflictResolution.KeepBoth);
        _executor.Undo(outcome);

        AssertContent(source, "new");
        AssertContent(P("dest", "a.txt"), "existing");
        Assert.False(File.Exists(P("dest", "a (2).txt")));
    }

    [Fact]
    public void Undo_RefusesToOverwriteSomethingThatTookTheOriginalName()
    {
        var source = File_("moved", "src", "a.txt");
        var dest = Dir("dest");

        var outcome = Run([source], dest);
        File.WriteAllText(source, "a different file now lives here");

        var undo = _executor.Undo(outcome);

        Assert.Equal(0, undo.Restored);
        Assert.Single(undo.Failed);
        AssertContent(source, "a different file now lives here"); // not clobbered
        AssertContent(P("dest", "a.txt"), "moved");               // still where it was moved to
    }

    [Fact]
    public void Undo_ReportsAnItemThatMovedOnAfterwards()
    {
        var source = File_("moved", "src", "a.txt");
        var dest = Dir("dest");

        var outcome = Run([source], dest);
        File.Delete(P("dest", "a.txt"));

        var undo = _executor.Undo(outcome);

        Assert.Equal(0, undo.Restored);
        Assert.Single(undo.Failed);
    }

    [Fact]
    public void Undo_IsRefusedForACopy()
    {
        var source = File_("x", "src", "a.txt");
        var dest = Dir("dest");

        var outcome = Run([source], dest, TransferVerb.Copy);

        Assert.False(outcome.CanUndo);
        Assert.NotEmpty(_executor.Undo(outcome).Failed);
        AssertContent(P("dest", "a.txt"), "x"); // the copy is untouched
    }

    // --- staging hygiene ---

    [Fact]
    public void PurgeStaging_LeavesAStagingFolderThatStillHoldsData()
    {
        var source = File_("new", "src", "a.txt");
        var dest = Dir("dest");
        File_("existing", "dest", "a.txt");

        var outcome = Run([source], dest, resolution: ConflictResolution.Replace);
        _executor.PurgeStaging(outcome);

        // Purging must never be the thing that destroys the displaced copy.
        AssertContent(TransferExecutor.StagedItems(outcome).Single(), "existing");
    }

    [Fact]
    public void PurgeStaging_RemovesTheFolderOnceEmpty()
    {
        var source = File_("new", "src", "a.txt");
        var dest = Dir("dest");
        File_("existing", "dest", "a.txt");

        var outcome = Run([source], dest, resolution: ConflictResolution.Replace);
        File.Delete(TransferExecutor.StagedItems(outcome).Single());
        _executor.PurgeStaging(outcome);

        Assert.False(Directory.Exists(outcome.StagingDirectory!));
    }

    [Fact]
    public void PurgeStaging_IgnoresAPathItDidNotCreate()
    {
        var precious = Dir("not-staging");
        File_("keep me", "not-staging", "important.txt");
        var outcome = new TransferOutcome(
            TransferVerb.Move, _root, [], [], [], precious);

        _executor.PurgeStaging(outcome);

        AssertContent(P("not-staging", "important.txt"), "keep me");
    }

    // --- live-disk revalidation between plan and execution ---

    [Fact]
    public void SourceDeletedAfterPlanning_FailsThatItemOnly()
    {
        var doomed = File_("gone soon", "src", "doomed.txt");
        var survivor = File_("fine", "src", "survivor.txt");
        var dest = Dir("dest");

        var plan = _planner.Plan([doomed, survivor], dest, TransferVerb.Move);
        File.Delete(doomed);
        var outcome = _executor.Execute(plan);

        Assert.Single(outcome.Failed);
        Assert.Equal(doomed, outcome.Failed.Single().SourcePath);
        AssertContent(P("dest", "survivor.txt"), "fine");
    }

    [Fact]
    public void DestinationTurnedIntoASubfolderAfterPlanning_IsRefused()
    {
        // Plan against a sibling folder, then make that folder live inside the source.
        File_("payload", "tree", "data.txt");
        var dest = Dir("dest");

        var plan = _planner.Plan([P("tree")], dest, TransferVerb.Move);

        // Re-point the plan's destination at a folder inside the source.
        var inner = Dir("tree", "inner");
        var sneaky = new TransferPlan(plan.Verb, inner, plan.Transfers, plan.Rejected);
        var outcome = _executor.Execute(sneaky);

        Assert.Single(outcome.Failed);
        AssertContent(P("tree", "data.txt"), "payload"); // the tree survived intact
        Assert.True(Directory.Exists(P("tree")));
        Assert.Empty(Directory.GetFileSystemEntries(dest));
    }

    [Fact]
    public void DestinationDeletedAfterPlanning_FailsWithoutTouchingTheSource()
    {
        var source = File_("payload", "src", "a.txt");
        var dest = Dir("dest");

        var plan = _planner.Plan([source], dest, TransferVerb.Move);
        Directory.Delete(dest);
        var outcome = _executor.Execute(plan);

        Assert.Single(outcome.Failed);
        AssertContent(source, "payload");
    }

    [Fact]
    public void ConflictAppearingAfterPlanning_FallsBackToKeepBoth()
    {
        var source = File_("new", "src", "a.txt");
        var dest = Dir("dest");

        var plan = _planner.Plan([source], dest, TransferVerb.Move);
        Assert.False(plan.Transfers.Single().Conflicts);

        File_("appeared", "dest", "a.txt"); // someone else got there first
        _executor.Execute(plan);

        AssertContent(P("dest", "a.txt"), "appeared");
        AssertContent(P("dest", "a (2).txt"), "new");
    }

    // --- failure isolation, progress, cancellation ---

    [Fact]
    public void ALockedFile_FailsAloneAndTheRestStillMove()
    {
        var locked = File_("locked", "src", "locked.txt");
        var other = File_("other", "src", "other.txt");
        var dest = Dir("dest");

        using (File.Open(locked, FileMode.Open, FileAccess.Read, FileShare.None))
        {
            var outcome = Run([locked, other], dest);

            Assert.Single(outcome.Failed);
            Assert.Equal(locked, outcome.Failed.Single().SourcePath);
            AssertContent(P("dest", "other.txt"), "other");
        }
        AssertContent(locked, "locked"); // never lost
    }

    [Fact]
    public void Progress_ReportsEveryItem()
    {
        var a = File_("a", "src", "a.txt");
        var b = File_("b", "src", "b.txt");
        var dest = Dir("dest");

        var seen = new List<TransferProgress>();
        var plan = _planner.Plan([a, b], dest, TransferVerb.Move);
        _executor.Execute(plan, null, default, new SyncProgress(seen.Add));

        Assert.Equal(2, seen.Max(p => p.Total));
        Assert.Contains(seen, p => p.CurrentName == "a.txt");
        Assert.Contains(seen, p => p.CurrentName == "b.txt");
    }

    [Fact]
    public void Cancellation_StopsCleanlyWithoutLosingAnything()
    {
        var a = File_("a", "src", "a.txt");
        var b = File_("b", "src", "b.txt");
        var dest = Dir("dest");

        using var cts = new CancellationTokenSource();
        cts.Cancel();
        var plan = _planner.Plan([a, b], dest, TransferVerb.Move);
        var outcome = _executor.Execute(plan, null, cts.Token);

        Assert.Empty(outcome.Completed);
        Assert.Empty(outcome.Failed);
        AssertContent(a, "a");
        AssertContent(b, "b");
    }

    // --- copies ---

    [Fact]
    public void CopyDirectory_LeavesTheSourceInPlace()
    {
        File_("payload", "src", "tree", "sub", "x.txt");
        var dest = Dir("dest");

        Run([P("src", "tree")], dest, TransferVerb.Copy);

        AssertContent(P("dest", "tree", "sub", "x.txt"), "payload");
        AssertContent(P("src", "tree", "sub", "x.txt"), "payload");
    }

    // --- reparse points ---

    [Fact]
    public void CopyingATreeContainingAJunction_IsRefusedRatherThanSilentlyDroppingIt()
    {
        File_("real", "target", "inner.txt");
        var tree = Dir("src", "tree");
        CreateDirectoryLink(Path.Combine(tree, "link"), P("target"));

        var dest = Dir("dest");
        var outcome = Run([tree], dest, TransferVerb.Copy);

        Assert.Single(outcome.Failed);
        Assert.Contains("junction", outcome.Failed.Single().Message, StringComparison.OrdinalIgnoreCase);
        AssertContent(P("target", "inner.txt"), "real"); // link target untouched
    }

    [Fact]
    public void MovingATreeContainingAJunction_WithinAVolume_KeepsTheJunction()
    {
        // A same-volume move is a rename, so the junction rides along and must be preserved.
        File_("real", "target", "inner.txt");
        var tree = Dir("src", "tree");
        CreateDirectoryLink(Path.Combine(tree, "link"), P("target"));

        var dest = Dir("dest");
        var outcome = Run([tree], dest);

        Assert.Empty(outcome.Failed);
        var moved = P("dest", "tree", "link");
        Assert.True(Directory.Exists(moved));
        Assert.NotEqual(0, (int)(new DirectoryInfo(moved).Attributes & FileAttributes.ReparsePoint));
        AssertContent(Path.Combine(moved, "inner.txt"), "real");
    }

    /// <summary>Fails loudly rather than skipping: these two tests guard a path where a junction
    /// could be silently destroyed, so quietly not running them is worse than a red build.</summary>
    private static void CreateDirectoryLink(string link, string target)
    {
        try
        {
            Directory.CreateSymbolicLink(link, target);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            throw new InvalidOperationException(
                "This test needs permission to create directory symbolic links — run it elevated, or turn on " +
                "Windows Developer Mode. It covers a data-loss path and must not be skipped silently.", ex);
        }
    }

    /// <summary>Synchronous <see cref="IProgress{T}"/>: the built-in one posts to a sync context,
    /// which would race the assertions.</summary>
    private sealed class SyncProgress(Action<TransferProgress> report) : IProgress<TransferProgress>
    {
        public void Report(TransferProgress value) => report(value);
    }
}
