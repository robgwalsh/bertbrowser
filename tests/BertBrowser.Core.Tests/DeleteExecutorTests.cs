using BertBrowser.Core.Services.Delete;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Deletes real files. Every test asserts on file <em>contents</em> after an undo, not just on
/// existence: a delete that can be reversed into an empty shell of the original tree is not
/// reversible at all.
/// </summary>
/// <remarks>
/// The executor is given the test root as its holding area rather than the root of the machine's
/// C: drive — same volume as the temp files, which is what the staged design needs, without a test
/// run leaving folders at the root of a real disk.
/// </remarks>
public sealed class DeleteExecutorTests : IDisposable
{
    private readonly string _root;
    private readonly DeletePlanner _planner;
    private readonly DeleteExecutor _executor;

    public DeleteExecutorTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-del-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
        _planner = new DeletePlanner(new FileSystemDeleteProbe(), []);
        _executor = new DeleteExecutor(new FileSystemDeleteProbe(), [], stagingRoot: _root);
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

    private DeleteOutcome Run(string[] sources, bool permanent = false)
    {
        var plan = _planner.Plan(
            sources.Select(s => new DeleteSource(s, Directory.Exists(s))).ToList(), permanent);
        return _executor.Execute(plan);
    }

    private static void AssertContent(string path, string expected)
    {
        Assert.True(File.Exists(path), $"expected a file at {path}");
        Assert.Equal(expected, File.ReadAllText(path));
    }

    private static bool Exists(string path) => File.Exists(path) || Directory.Exists(path);

    /// <summary>Every file under a tree as "relative path → contents", so a restored tree can be
    /// compared with what it was byte for byte rather than by count.</summary>
    private static Dictionary<string, string> Snapshot(string root) =>
        Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories)
            .ToDictionary(
                f => Path.GetRelativePath(root, f),
                File.ReadAllText,
                StringComparer.OrdinalIgnoreCase);

    // --- the ordinary delete ---

    [Fact]
    public void AFile_LeavesItsFolder_ButIsStillHeld()
    {
        var file = File_("hello", "docs", "a.txt");

        var outcome = Run([file]);

        Assert.Empty(outcome.Failed);
        Assert.False(File.Exists(file));

        var deleted = Assert.Single(outcome.Deleted);
        AssertContent(deleted.StagedPath!, "hello"); // intact until the delete is committed
        Assert.True(outcome.CanUndo);
    }

    [Fact]
    public void AFolder_GoesWholeAndComesBackWhole()
    {
        var folder = Dir("tree");
        File_("one", "tree", "a.txt");
        File_("two", "tree", "deep", "b.txt");
        File_("three", "tree", "deep", "deeper", "c.txt");
        var before = Snapshot(folder);

        var outcome = Run([folder]);
        Assert.False(Directory.Exists(folder));

        var undo = _executor.Undo(outcome);

        Assert.Empty(undo.Failed);
        Assert.Equal(1, undo.Restored);
        Assert.Equal(before, Snapshot(folder));
    }

    [Fact]
    public void SeveralItems_AreAllDeletedAndAllRestored()
    {
        var a = File_("a", "a.txt");
        var b = File_("b", "b.txt");
        var c = Dir("c");
        File_("inside", "c", "inner.txt");

        var outcome = Run([a, b, c]);

        Assert.Equal(3, outcome.Deleted.Count);
        Assert.False(Exists(a));
        Assert.False(Exists(b));
        Assert.False(Exists(c));

        _executor.Undo(outcome);

        AssertContent(a, "a");
        AssertContent(b, "b");
        AssertContent(P("c", "inner.txt"), "inside");
    }

    [Fact]
    public void TwoItemsWithTheSameName_AreBothHeldWithoutOneEatingTheOther()
    {
        // A flattened search result can hand over two files that share a name.
        var one = File_("first", "x", "same.txt");
        var two = File_("second", "y", "same.txt");

        var outcome = Run([one, two]);

        Assert.Empty(outcome.Failed);
        Assert.Equal(2, outcome.Deleted.Count);
        Assert.NotEqual(outcome.Deleted[0].StagedPath, outcome.Deleted[1].StagedPath);

        _executor.Undo(outcome);

        AssertContent(one, "first");
        AssertContent(two, "second");
    }

    [Fact]
    public void TheHoldingFolderIsHidden_SoADeleteDoesNotLeaveSomethingOnScreen()
    {
        var file = File_("hello", "a.txt");

        var outcome = Run([file]);

        var batch = Path.GetDirectoryName(outcome.Deleted[0].StagedPath!)!;
        Assert.True(File.GetAttributes(batch).HasFlag(FileAttributes.Hidden));
        Assert.True(File.GetAttributes(Path.GetDirectoryName(batch)!).HasFlag(FileAttributes.Hidden));
    }

    // --- committing ---

    [Fact]
    public void CommittingErasesWhatWasHeld()
    {
        var folder = Dir("tree");
        File_("one", "tree", "a.txt");

        var outcome = Run([folder]);
        var staged = outcome.Deleted[0].StagedPath!;
        Assert.True(Directory.Exists(staged));

        DeleteExecutor.CommitStaging(outcome);

        Assert.False(Directory.Exists(staged));
        foreach (var directory in outcome.StagingDirectories)
            Assert.False(Directory.Exists(directory));
    }

    [Fact]
    public void CommittingTakesTheTrashFolderWithItWhenItsLastBatchGoes()
    {
        var outcome = Run([File_("hello", "a.txt")]);
        var trash = Path.GetDirectoryName(outcome.StagingDirectories[0])!;
        Assert.True(Directory.Exists(trash));

        DeleteExecutor.CommitStaging(outcome);

        Assert.False(Directory.Exists(trash));
    }

    [Fact]
    public void CommittingRefusesAnythingThatIsNotAHoldingFolder()
    {
        // The guard that stops a mangled outcome turning CommitStaging into "delete this folder".
        var precious = Dir("precious");
        File_("keep me", "precious", "a.txt");

        DeleteExecutor.CommitStaging(new DeleteOutcome(false, [], [], [precious]));

        AssertContent(P("precious", "a.txt"), "keep me");
    }

    // --- undo ---

    [Fact]
    public void UndoRefusesToOverwriteWhateverTookTheOriginalName()
    {
        var file = File_("original", "a.txt");
        var outcome = Run([file]);

        File.WriteAllText(file, "something else entirely");

        var undo = _executor.Undo(outcome);

        Assert.Equal(0, undo.Restored);
        Assert.Single(undo.Failed);
        AssertContent(file, "something else entirely"); // untouched
        AssertContent(outcome.Deleted[0].StagedPath!, "original"); // and still held, not lost
    }

    [Fact]
    public void UndoLeavesTheHoldingFolderBehindWhenItStillHoldsSomething()
    {
        var kept = File_("original", "kept.txt");
        var moved = File_("moved", "moved.txt");
        var outcome = Run([kept, moved]);

        File.WriteAllText(kept, "in the way");

        var undo = _executor.Undo(outcome);

        Assert.Equal(1, undo.Restored);
        AssertContent(moved, "moved");
        // The one that could not go back must not be swept up with the empty-folder cleanup.
        AssertContent(outcome.Deleted.First(d => d.SourcePath == kept).StagedPath!, "original");
    }

    [Fact]
    public void UndoRemovesTheHoldingFolderOnceEverythingIsBack()
    {
        var outcome = Run([File_("hello", "a.txt")]);

        _executor.Undo(outcome);

        foreach (var directory in outcome.StagingDirectories)
            Assert.False(Directory.Exists(directory));
    }

    [Fact]
    public void UndoIsRefusedForAPermanentDelete()
    {
        var outcome = Run([File_("hello", "a.txt")], permanent: true);

        var undo = _executor.Undo(outcome);

        Assert.Equal(0, undo.Restored);
        Assert.Single(undo.Failed);
    }

    // --- permanent delete ---

    [Fact]
    public void APermanentDelete_ErasesInPlaceAndHoldsNothing()
    {
        var folder = Dir("tree");
        File_("one", "tree", "a.txt");

        var outcome = Run([folder], permanent: true);

        Assert.Empty(outcome.Failed);
        Assert.False(Directory.Exists(folder));
        Assert.Empty(outcome.StagingDirectories);
        Assert.Null(outcome.Deleted[0].StagedPath);
        Assert.False(outcome.CanUndo);
    }

    [Fact]
    public void APermanentDelete_GetsPastAReadOnlyFileInsideTheTree()
    {
        var folder = Dir("tree");
        var locked = File_("locked", "tree", "readonly.txt");
        File.SetAttributes(locked, FileAttributes.ReadOnly);

        var outcome = Run([folder], permanent: true);

        Assert.Empty(outcome.Failed);
        Assert.False(Directory.Exists(folder));
    }

    // --- failures are per item ---

    [Fact]
    public void AnItemThatVanishedBetweenPlanningAndDeleting_FailsAloneAndIsReported()
    {
        var gone = File_("gone", "gone.txt");
        var kept = File_("kept", "kept.txt");

        var plan = _planner.Plan(
            [new DeleteSource(gone, false), new DeleteSource(kept, false)], permanent: false);
        File.Delete(gone); // disk changes while the confirmation is open

        var outcome = _executor.Execute(plan);

        Assert.Single(outcome.Failed);
        Assert.Equal(kept, Assert.Single(outcome.Deleted).SourcePath);
        Assert.False(File.Exists(kept));
    }

    [Fact]
    public void AProtectedLocationIsRefusedAtTheLastMomentToo()
    {
        // Planned by an executor that does not protect it, run by one that does: the executor must
        // hold the line itself rather than trusting a plan built before the confirmation.
        var folder = Dir("system-ish");
        var plan = _planner.Plan([new DeleteSource(folder, true)], permanent: true);

        var guarded = new DeleteExecutor(new FileSystemDeleteProbe(), [folder], stagingRoot: _root);
        var outcome = guarded.Execute(plan);

        Assert.Single(outcome.Failed);
        Assert.True(Directory.Exists(folder));
    }

    [Fact]
    public void CancellationStopsTheRunWithoutDeletingAnything()
    {
        var a = File_("a", "a.txt");
        var b = File_("b", "b.txt");
        var plan = _planner.Plan(
            [new DeleteSource(a, false), new DeleteSource(b, false)], permanent: false);

        using var cts = new CancellationTokenSource();
        cts.Cancel();
        var outcome = _executor.Execute(plan, cts.Token);

        Assert.Empty(outcome.Deleted);
        Assert.Empty(outcome.Failed);
        AssertContent(a, "a");
        AssertContent(b, "b");
    }

    [Fact]
    public void ProgressReportsEveryItemAndThenFinishes()
    {
        var plan = _planner.Plan(
            [new DeleteSource(File_("a", "a.txt"), false), new DeleteSource(File_("b", "b.txt"), false)],
            permanent: false);

        // Not Progress<T>: it posts to the thread pool when there is no sync context, so the list
        // would still be empty when the assertions run.
        var reports = new SynchronousProgress<DeleteProgress>();
        _executor.Execute(plan, CancellationToken.None, reports);

        Assert.Equal(2, reports.Reports[^1].Total);
        Assert.Equal(2, reports.Reports[^1].Done);
    }

    // --- sweeping up after a crash ---

    [Fact]
    public void AbandonedHoldingFoldersAreSweptUp_ButRecentOnesAreLeftAlone()
    {
        var outcome = Run([File_("hello", "a.txt")]);
        var batch = outcome.StagingDirectories[0];

        // A batch from this session belongs to a possibly-still-running instance: leave it.
        DeleteExecutor.PurgeAbandonedStaging(TimeSpan.FromDays(1), [_root]);
        Assert.True(Directory.Exists(batch));

        Directory.SetCreationTimeUtc(batch, DateTime.UtcNow.AddDays(-2));
        DeleteExecutor.PurgeAbandonedStaging(TimeSpan.FromDays(1), [_root]);

        Assert.False(Directory.Exists(batch));
        Assert.False(Directory.Exists(Path.Combine(_root, ".bertbrowser-trash")));
    }

    // --- what search has to hide ---

    [Fact]
    public void AStagedItemIsRecognisedAsHeld_SoSearchCanHideIt()
    {
        var outcome = Run([File_("hello", "a.txt")]);

        Assert.True(DeleteExecutor.IsHeldPath(outcome.Deleted[0].StagedPath!));
    }

    [Theory]
    [InlineData(@"C:\.bertbrowser-trash\delete-abc123\a.txt", true)]
    [InlineData(@"C:\p\.bertbrowser-deleted-abc123\a.txt", true)]
    [InlineData(@"C:\.BERTBROWSER-TRASH\delete-abc123\a.txt", true)]
    [InlineData(@"C:\p\a.txt", false)]
    // A folder that merely starts the same way is somebody's own folder, not ours.
    [InlineData(@"C:\.bertbrowser-trash-notes\a.txt", false)]
    public void HeldPathsAreTheOnesInAHoldingFolder(string path, bool held) =>
        Assert.Equal(held, DeleteExecutor.IsHeldPath(path));

    // --- meta: the checks above can actually fail ---

    [Fact]
    public void MetaTheSnapshotComparisonNoticesAMissingFile()
    {
        var folder = Dir("tree");
        File_("one", "tree", "a.txt");
        File_("two", "tree", "b.txt");
        var before = Snapshot(folder);

        File.Delete(P("tree", "b.txt"));

        Assert.NotEqual(before, Snapshot(folder));
    }

    [Fact]
    public void MetaTheSnapshotComparisonNoticesChangedContents()
    {
        var folder = Dir("tree");
        File_("one", "tree", "a.txt");
        var before = Snapshot(folder);

        File.WriteAllText(P("tree", "a.txt"), "tampered");

        Assert.NotEqual(before, Snapshot(folder));
    }
}

/// <summary>Collects progress on the thread that reported it.</summary>
internal sealed class SynchronousProgress<T> : IProgress<T>
{
    public List<T> Reports { get; } = [];

    public void Report(T value) => Reports.Add(value);
}
