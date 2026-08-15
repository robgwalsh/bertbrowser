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

    private DeleteOutcome Run(string[] sources, DeleteMode mode = DeleteMode.Staged)
    {
        var plan = _planner.Plan(
            sources.Select(s => new DeleteSource(s, Directory.Exists(s))).ToList(), mode);
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
        var outcome = Run([File_("hello", "a.txt")], DeleteMode.Permanent);

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

        var outcome = Run([folder], DeleteMode.Permanent);

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

        var outcome = Run([folder], DeleteMode.Permanent);

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
            [new DeleteSource(gone, false), new DeleteSource(kept, false)], DeleteMode.Staged);
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
        var plan = _planner.Plan([new DeleteSource(folder, true)], DeleteMode.Permanent);

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
            [new DeleteSource(a, false), new DeleteSource(b, false)], DeleteMode.Staged);

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
            DeleteMode.Staged);

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

    // --- what search must not show ---

    /// <summary>
    /// A file the user just deleted turning up in search looks exactly like a delete that did not
    /// work — and a recycled one turns up under a name that is not even the one they deleted, as
    /// <c>$RAB1234.txt</c>. Both are filtered, which is why this is the same predicate for both.
    /// </summary>
    [Theory]
    [InlineData(@"C:\.bertbrowser-trash\delete-abc123\a.txt")]
    [InlineData(@"C:\p\.bertbrowser-deleted-abc123\a.txt")]
    [InlineData(@"C:\$Recycle.Bin\S-1-5-21-1\$RAB1234.txt")]
    [InlineData(@"C:\$RECYCLE.BIN\S-1-5-21-1\$RAB1234.txt")]
    public void DeletedItems_AreHiddenFromSearch(string path)
    {
        Assert.True(DeleteExecutor.IsHeldPath(path));
    }

    [Theory]
    [InlineData(@"C:\p\a.txt")]
    [InlineData(@"C:\p\$Recycled\a.txt")]
    [InlineData(@"C:\p\bertbrowser-trash\a.txt")]
    public void OrdinaryFiles_AreNotHiddenFromSearch(string path)
    {
        Assert.False(DeleteExecutor.IsHeldPath(path));
    }

    // --- the Recycle Bin ---
    //
    // Against a fake bin that really moves the files, so these assert on contents like the rest of
    // the suite. The real one is IFileOperation and cannot be exercised here; what is being pinned
    // down is the executor's own behaviour around it — routing, a mixed batch, and undo.

    private FakeRecycleBin NewBin(params string[] volumelessPaths)
    {
        var bin = new FakeRecycleBin(Path.Combine(_root, "fake-bin"));
        foreach (var path in volumelessPaths) bin.RefuseVolumeFor(path);
        return bin;
    }

    private DeleteOutcome RunWithBin(FakeRecycleBin bin, params string[] sources)
    {
        var planner = new DeletePlanner(new FileSystemDeleteProbe(), [], bin);
        var executor = new DeleteExecutor(
            new FileSystemDeleteProbe(), [], stagingRoot: _root, recycleBin: bin, recycleProbe: bin);
        var plan = planner.Plan(
            sources.Select(s => new DeleteSource(s, Directory.Exists(s))).ToList(), DeleteMode.Recycle);
        return executor.Execute(plan);
    }

    private DeleteUndoResult UndoWithBin(FakeRecycleBin bin, DeleteOutcome outcome) =>
        new DeleteExecutor(
                new FileSystemDeleteProbe(), [], stagingRoot: _root, recycleBin: bin, recycleProbe: bin)
            .Undo(outcome);

    [Fact]
    public void ARecycledFile_LeavesItsFolder_AndIsHeldByTheBin()
    {
        var file = File_("hello", "a.txt");
        var bin = NewBin();

        var outcome = RunWithBin(bin, file);

        Assert.False(Exists(file));
        var item = Assert.Single(outcome.Deleted);
        Assert.NotNull(item.RecycledPath);
        Assert.Null(item.StagedPath);
        AssertContent(item.RecycledPath!, "hello");
        Assert.True(outcome.CanUndo);
    }

    /// <summary>Nothing is staged for a recycled item, so there is no holding folder to commit —
    /// which is the whole structural gain: the data outlives the undo record by as long as the user
    /// leaves the bin alone, rather than by exactly one operation.</summary>
    [Fact]
    public void ARecycledDelete_CreatesNoHoldingFolder()
    {
        var bin = NewBin();

        var outcome = RunWithBin(bin, File_("hello", "a.txt"));

        Assert.Empty(outcome.StagingDirectories);
        DeleteExecutor.CommitStaging(outcome); // must be a no-op rather than reaching into the bin
        AssertContent(outcome.Deleted[0].RecycledPath!, "hello");
    }

    [Fact]
    public void UndoingARecycledDelete_PutsTheFileBackWithItsContents()
    {
        var file = File_("hello", "a.txt");
        var bin = NewBin();
        var outcome = RunWithBin(bin, file);

        var undo = UndoWithBin(bin, outcome);

        Assert.Equal(1, undo.Restored);
        Assert.Empty(undo.Failed);
        AssertContent(file, "hello");
    }

    [Fact]
    public void UndoingARecycledFolder_RestoresTheTreeByteForByte()
    {
        var folder = Dir("tree");
        File_("one", "tree", "a.txt");
        File_("two", "tree", "nested", "b.txt");
        var before = Snapshot(folder);
        var bin = NewBin();

        var outcome = RunWithBin(bin, folder);
        Assert.False(Exists(folder));
        var undo = UndoWithBin(bin, outcome);

        Assert.Equal(1, undo.Restored);
        Assert.Equal(before, Snapshot(folder));
    }

    /// <summary>The case the whole fallback exists for: one selection spanning a volume with a bin
    /// and one without. Both halves must survive, and undo must put both back.</summary>
    [Fact]
    public void AMixedBatch_RecyclesWhatItCan_AndHoldsTheRest()
    {
        var recyclable = File_("first", "a.txt");
        var notRecyclable = File_("second", "b.txt");
        var bin = NewBin(notRecyclable);

        var outcome = RunWithBin(bin, recyclable, notRecyclable);

        Assert.False(Exists(recyclable));
        Assert.False(Exists(notRecyclable));

        var recycled = outcome.Deleted.Single(d => d.SourcePath == recyclable);
        var staged = outcome.Deleted.Single(d => d.SourcePath == notRecyclable);
        Assert.NotNull(recycled.RecycledPath);
        Assert.Null(recycled.StagedPath);
        Assert.NotNull(staged.StagedPath);
        Assert.Null(staged.RecycledPath);
        Assert.NotEmpty(outcome.StagingDirectories);

        var undo = UndoWithBin(bin, outcome);

        Assert.Equal(2, undo.Restored);
        Assert.Empty(undo.Failed);
        AssertContent(recyclable, "first");
        AssertContent(notRecyclable, "second");
    }

    /// <summary>
    /// With no bin wired up at all, a plan that asked to recycle must fall back to the holding
    /// folder. The one outcome that would be unforgivable is erasing instead.
    /// </summary>
    [Fact]
    public void WithNoBinAvailable_ARecyclePlanIsHeld_NeverErased()
    {
        var file = File_("hello", "a.txt");
        var bin = NewBin();
        var planner = new DeletePlanner(new FileSystemDeleteProbe(), [], bin);
        var plan = planner.Plan([new DeleteSource(file, false)], DeleteMode.Recycle);
        Assert.Equal(DeleteDisposition.Recycle, plan.Deletions[0].Disposition);

        // Same plan, executed by an executor that has no bin.
        var outcome = _executor.Execute(plan);

        Assert.False(Exists(file));
        var item = Assert.Single(outcome.Deleted);
        Assert.NotNull(item.StagedPath);
        AssertContent(item.StagedPath!, "hello");

        Assert.Equal(1, _executor.Undo(outcome).Restored);
        AssertContent(file, "hello");
    }

    /// <summary>
    /// The shell erases rather than holds an item too big for the bin. That is not a failure, but
    /// there is nothing to undo — and the outcome has to say so rather than offering a Ctrl+Z that
    /// cannot work.
    /// </summary>
    [Fact]
    public void AnItemTheBinErasedInsteadOfHolding_IsNotOfferedForUndo()
    {
        var file = File_("hello", "a.txt");
        var bin = NewBin();
        bin.EraseInsteadOfHolding(file);

        var outcome = RunWithBin(bin, file);

        Assert.False(Exists(file));
        Assert.Null(Assert.Single(outcome.Deleted).RecycledPath);
        Assert.False(outcome.CanUndo);
    }

    [Fact]
    public void UndoingAnItemTheBinNoLongerHolds_IsReportedRatherThanSilent()
    {
        var file = File_("hello", "a.txt");
        var bin = NewBin();
        var outcome = RunWithBin(bin, file);

        bin.Empty(); // the user emptied the Recycle Bin before pressing Ctrl+Z

        var undo = UndoWithBin(bin, outcome);

        Assert.Equal(0, undo.Restored);
        Assert.Contains("Recycle Bin", Assert.Single(undo.Failed).Message);
    }

    [Fact]
    public void UndoWillNotOverwriteSomethingThatTookTheOriginalPath()
    {
        var file = File_("hello", "a.txt");
        var bin = NewBin();
        var outcome = RunWithBin(bin, file);

        File.WriteAllText(file, "something else entirely");

        var undo = UndoWithBin(bin, outcome);

        Assert.Equal(0, undo.Restored);
        Assert.Single(undo.Failed);
        AssertContent(file, "something else entirely");
    }
}

/// <summary>
/// A stand-in for the Windows Recycle Bin that really moves files, so tests can assert on contents.
/// It models the three behaviours the executor has to cope with: a volume the bin does not serve,
/// an item the shell erases instead of holding, and a bin that has been emptied.
/// </summary>
internal sealed class FakeRecycleBin(string binRoot) : IRecycleBin, IRecycleProbe
{
    private readonly HashSet<string> _noBin = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<string> _erases = new(StringComparer.OrdinalIgnoreCase);
    private int _next;

    /// <summary>Makes <paramref name="path"/> look like it lives on a share or on media with the
    /// bin turned off.</summary>
    public void RefuseVolumeFor(string path) => _noBin.Add(path);

    /// <summary>Makes the bin accept <paramref name="path"/> but erase it — what the shell does with
    /// an item larger than the bin's quota.</summary>
    public void EraseInsteadOfHolding(string path) => _erases.Add(path);

    public void Empty()
    {
        if (Directory.Exists(binRoot)) Directory.Delete(binRoot, recursive: true);
    }

    public bool CanRecycle(string path) => !_noBin.Contains(path);

    public RecycleResult Recycle(
        IReadOnlyList<PlannedDelete> items,
        CancellationToken ct = default,
        IProgress<DeleteProgress>? progress = null)
    {
        Directory.CreateDirectory(binRoot);
        var recycled = new List<RecycledItem>();
        var failed = new List<FailedDelete>();

        var done = 0;
        foreach (var item in items)
        {
            progress?.Report(new DeleteProgress(done++, items.Count, item.Name));

            if (_erases.Contains(item.SourcePath))
            {
                if (item.IsDirectory) Directory.Delete(item.SourcePath, recursive: true);
                else File.Delete(item.SourcePath);
                recycled.Add(new RecycledItem(item.SourcePath, item.IsDirectory, null));
                continue;
            }

            // The real bin renames to $R<id><ext>; the shape matters more than the exact name.
            var target = Path.Combine(binRoot, $"$R{_next++:D6}{Path.GetExtension(item.SourcePath)}");
            if (item.IsDirectory) Directory.Move(item.SourcePath, target);
            else File.Move(item.SourcePath, target, overwrite: false);
            recycled.Add(new RecycledItem(item.SourcePath, item.IsDirectory, target));
        }

        return new RecycleResult(recycled, failed);
    }

    public bool Restore(DeletedItem item)
    {
        if (item.RecycledPath is not { } held) return false;
        if (item.IsDirectory)
        {
            if (!Directory.Exists(held)) return false;
            Directory.Move(held, item.SourcePath);
        }
        else
        {
            if (!File.Exists(held)) return false;
            File.Move(held, item.SourcePath, overwrite: false);
        }
        return true;
    }
}

/// <summary>Collects progress on the thread that reported it.</summary>
internal sealed class SynchronousProgress<T> : IProgress<T>
{
    public List<T> Reports { get; } = [];

    public void Report(T value) => Reports.Add(value);
}
