using BertBrowser.Core.Services.Compare;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Mft;
using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// A whole sync, on real files: compare two folders, plan it, run it, put it back. Every assertion
/// is on file <em>contents</em> — a sync that copies the wrong side of a pair passes every test
/// that only asks whether a file is there.
/// </summary>
public sealed class SyncRunnerTests : IDisposable
{
    private readonly string _root;
    private readonly SyncRunner _runner = new(new TransferExecutor(), new DeleteExecutor());

    public SyncRunnerTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-sync-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
        Dir("left");
        Dir("right");
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
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

    /// <summary>The whole pipeline, walking both sides from disk — no index involved, so the test
    /// exercises what a comparison of two unindexed folders actually does.</summary>
    private async Task<SyncPreview> PreviewAsync(bool removeRightOnly = false)
    {
        var db = new Data.Db(P("index.db"));
        db.Migrate();

        var compare = await new FolderCompareService(
                new Data.FsIndexRepository(db), new NullMftIndexService())
            .CompareAsync(P("left"), P("right"), includeHidden: true, CancellationToken.None);

        Assert.Equal(CompareAvailability.Ready, compare.Availability);
        return SyncPlanner.Preview(compare, removeRightOnly);
    }

    private SyncOutcome Run(SyncPreview preview) =>
        _runner.Run(SyncPlanner.ToPlans(
            preview, new TransferPlanner(), new DeletePlanner(), DeleteMode.Staged));

    // --- the run ---

    [Fact]
    public async Task AFileTheRightSideLacksArrivesWithItsContents()
    {
        File_("hello", "left", "new.txt");

        var outcome = Run(await PreviewAsync());

        AssertContent(P("right", "new.txt"), "hello");
        Assert.Equal(1, outcome.CopiedCount);
        Assert.Equal(0, outcome.ReplacedCount);
        Assert.Empty(outcome.FailedCopies);
    }

    [Fact]
    public async Task AStaleFileOnTheRightIsReplacedAndTheOldOneKept()
    {
        var newer = File_("v2", "left", "a.txt");
        File_("v1", "right", "a.txt");
        File.SetLastWriteTimeUtc(newer, DateTime.UtcNow.AddHours(1));

        var outcome = Run(await PreviewAsync());

        AssertContent(P("right", "a.txt"), "v2");
        Assert.Equal(1, outcome.ReplacedCount);
        AssertContent(Assert.Single(TransferExecutor.StagedItems(outcome.Copies[0])), "v1");
    }

    [Fact]
    public async Task AWholeFolderArrivesInOneAction()
    {
        File_("x", "left", "bin", "deep", "app.txt");

        var outcome = Run(await PreviewAsync());

        AssertContent(P("right", "bin", "deep", "app.txt"), "x");
        Assert.Equal(1, outcome.CopiedCount);
    }

    [Fact]
    public async Task NothingIsRemovedUnlessRemovalWasAskedFor()
    {
        File_("mine", "right", "extra.txt");

        Run(await PreviewAsync());

        AssertContent(P("right", "extra.txt"), "mine");
    }

    [Fact]
    public async Task WhatOnlyTheRightHasIsRemovedWhenAskedFor()
    {
        File_("mine", "right", "extra.txt");

        var outcome = Run(await PreviewAsync(removeRightOnly: true));

        Assert.False(File.Exists(P("right", "extra.txt")));
        Assert.Equal(1, outcome.RemovedCount);
    }

    /// <summary>
    /// Both halves in one run, which is the case the ordering exists for: the copies land before
    /// anything is taken away, so a run stopped in the middle leaves the right side holding more
    /// than it started with rather than less.
    /// </summary>
    [Fact]
    public async Task AMixedRunCopiesReplacesAndRemoves()
    {
        var newer = File_("v2", "left", "a.txt");
        File_("v1", "right", "a.txt");
        File.SetLastWriteTimeUtc(newer, DateTime.UtcNow.AddHours(1));
        File_("fresh", "left", "b.txt");
        File_("stale", "right", "c.txt");

        var outcome = Run(await PreviewAsync(removeRightOnly: true));

        AssertContent(P("right", "a.txt"), "v2");
        AssertContent(P("right", "b.txt"), "fresh");
        Assert.False(File.Exists(P("right", "c.txt")));
        Assert.True(outcome.CanUndo);
    }

    // --- putting it back ---

    /// <summary>
    /// The undo the user was offered, in full: what arrived goes, what was replaced comes back,
    /// what was removed returns. A half-undo would leave the right side in a state neither side
    /// ever had.
    /// </summary>
    [Fact]
    public async Task UndoPutsTheRightSideBackExactlyAsItWas()
    {
        var newer = File_("v2", "left", "a.txt");
        File_("v1", "right", "a.txt");
        File.SetLastWriteTimeUtc(newer, DateTime.UtcNow.AddHours(1));
        File_("fresh", "left", "b.txt");
        File_("stale", "right", "c.txt");

        var outcome = Run(await PreviewAsync(removeRightOnly: true));
        var undo = _runner.Undo(outcome);

        Assert.Empty(undo.Failed);
        AssertContent(P("right", "a.txt"), "v1");
        Assert.False(File.Exists(P("right", "b.txt")));
        AssertContent(P("right", "c.txt"), "stale");

        // And the left side was never a party to any of it.
        AssertContent(P("left", "a.txt"), "v2");
        AssertContent(P("left", "b.txt"), "fresh");
    }

    [Fact]
    public async Task UndoRemovesAWholeFolderTheSyncBrought()
    {
        File_("x", "left", "bin", "deep", "app.txt");

        _runner.Undo(Run(await PreviewAsync()));

        Assert.False(Directory.Exists(P("right", "bin")));
        AssertContent(P("left", "bin", "deep", "app.txt"), "x");
    }

    /// <summary>Once retired, the replaced file is finally gone and the one that replaced it stays
    /// — which is the whole point of holding it until then.</summary>
    [Fact]
    public async Task RetiringTheRunCommitsWhatItSetAside()
    {
        var newer = File_("v2", "left", "a.txt");
        File_("v1", "right", "a.txt");
        File.SetLastWriteTimeUtc(newer, DateTime.UtcNow.AddHours(1));

        var outcome = Run(await PreviewAsync());
        SyncRunner.Retire(outcome);

        AssertContent(P("right", "a.txt"), "v2");
        Assert.Empty(TransferExecutor.StagedItems(outcome.Copies[0]));
    }

    /// <summary>A second sync of a folder already in step must do nothing at all — not re-copy,
    /// and above all not leave a trail of "a (2).txt" behind it.</summary>
    [Fact]
    public async Task SyncingTwiceIsSyncingOnce()
    {
        File_("hello", "left", "new.txt");
        File_("x", "left", "bin", "app.txt");

        Run(await PreviewAsync());
        var second = await PreviewAsync();

        Assert.Empty(second.Actions);
        Assert.Equal(2, Directory.GetFileSystemEntries(P("right")).Length);
    }
}
