using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Delete;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The rules that decide what a delete is allowed to reach. Run against a fake filesystem, because
/// the interesting cases — a drive root, the Windows folder, a folder selected together with
/// something inside it — are ones nobody should set up for real to find out.
/// </summary>
public sealed class DeletePlannerTests
{
    private readonly FakeDeleteProbe _probe = new();
    private readonly DeletePlanner _planner;

    public DeletePlannerTests() =>
        _planner = new DeletePlanner(_probe, [@"C:\Windows", @"C:\Users\rob"]);

    private static DeleteSource File_(string path) => new(path, IsDirectory: false);

    private static DeleteSource Dir(string path) => new(path, IsDirectory: true);

    private static DeleteRejection ReasonFor(DeletePlan plan, string source) =>
        plan.Rejected.Single(r => r.SourcePath.Equals(source, StringComparison.OrdinalIgnoreCase)).Reason;

    private DeletePlan Plan(params DeleteSource[] sources) => _planner.Plan(sources, DeleteMode.Staged);

    // --- the ordinary cases ---

    [Fact]
    public void AFile_IsPlanned()
    {
        _probe.AddFile(@"C:\p\a.txt");

        var plan = Plan(File_(@"C:\p\a.txt"));

        Assert.Empty(plan.Rejected);
        var deletion = Assert.Single(plan.Deletions);
        Assert.Equal(@"C:\p\a.txt", deletion.SourcePath);
        Assert.False(deletion.IsDirectory);
    }

    [Fact]
    public void AFolder_IsPlannedAsADirectory_WhateverTheCallerClaimed()
    {
        _probe.AddDirectory(@"C:\p\sub");

        // The selection says "file"; disk says otherwise, and disk wins — the executor needs the
        // right answer to know whether to move a file or a directory.
        var plan = Plan(File_(@"C:\p\sub"));

        Assert.True(Assert.Single(plan.Deletions).IsDirectory);
    }

    [Fact]
    public void TheSameItemTwice_IsPlannedOnce()
    {
        _probe.AddFile(@"C:\p\a.txt");

        var plan = Plan(File_(@"C:\p\a.txt"), File_(@"C:\P\A.TXT"));

        Assert.Single(plan.Deletions);
    }

    [Fact]
    public void NoSources_IsAnEmptyPlan()
    {
        var plan = _planner.Plan([], DeleteMode.Permanent);

        Assert.False(plan.HasWork);
        Assert.True(plan.Permanent);
    }

    // --- the refusals ---

    [Fact]
    public void AnItemThatHasGone_IsRefused()
    {
        var plan = Plan(File_(@"C:\p\vanished.txt"));

        Assert.False(plan.HasWork);
        Assert.Equal(DeleteRejection.SourceMissing, ReasonFor(plan, @"C:\p\vanished.txt"));
    }

    [Fact]
    public void ADriveRoot_IsRefused()
    {
        _probe.AddDirectory(@"D:\");

        var plan = Plan(Dir(@"D:\"));

        Assert.False(plan.HasWork);
        Assert.Equal(DeleteRejection.SourceIsRoot, ReasonFor(plan, @"D:\"));
    }

    [Fact]
    public void AProtectedLocation_IsRefused()
    {
        _probe.AddDirectory(@"C:\Windows");

        var plan = Plan(Dir(@"C:\Windows"));

        Assert.False(plan.HasWork);
        Assert.Equal(DeleteRejection.ProtectedLocation, ReasonFor(plan, @"C:\Windows"));
    }

    [Fact]
    public void SomethingInsideAProtectedLocation_IsStillDeletable()
    {
        // The folder is protected, not everything under it — this is a file browser.
        _probe.AddFile(@"C:\Windows\Temp\junk.log");

        var plan = Plan(File_(@"C:\Windows\Temp\junk.log"));

        Assert.Single(plan.Deletions);
    }

    [Fact]
    public void AnItemInsideAFolderBeingDeleted_TravelsWithItAndIsNotReported()
    {
        _probe.AddDirectory(@"C:\p\sub");
        _probe.AddFile(@"C:\p\sub\a.txt");

        var plan = Plan(Dir(@"C:\p\sub"), File_(@"C:\p\sub\a.txt"));

        Assert.Equal(@"C:\p\sub", Assert.Single(plan.Deletions).SourcePath);
        Assert.Equal(DeleteRejection.InsideADeletedFolder, ReasonFor(plan, @"C:\p\sub\a.txt"));
        Assert.Empty(plan.Problems); // benign: it is being deleted, just not separately
    }

    [Fact]
    public void ADeeplyNestedItem_TravelsWithItsAncestorToo()
    {
        _probe.AddDirectory(@"C:\p\sub");
        _probe.AddDirectory(@"C:\p\sub\deep\deeper");

        var plan = Plan(Dir(@"C:\p\sub"), Dir(@"C:\p\sub\deep\deeper"));

        Assert.Equal(@"C:\p\sub", Assert.Single(plan.Deletions).SourcePath);
    }

    [Fact]
    public void ASiblingWithASharedNamePrefix_IsNotMistakenForAChild()
    {
        // "C:\p\sub2" starts with "C:\p\sub" as a string but is not inside it.
        _probe.AddDirectory(@"C:\p\sub");
        _probe.AddDirectory(@"C:\p\sub2");

        var plan = Plan(Dir(@"C:\p\sub"), Dir(@"C:\p\sub2"));

        Assert.Equal(2, plan.Deletions.Count);
        Assert.Empty(plan.Rejected);
    }

    [Fact]
    public void OneRefusalDoesNotStopTheRest()
    {
        _probe.AddFile(@"C:\p\a.txt");
        _probe.AddDirectory(@"C:\Windows");

        var plan = Plan(Dir(@"C:\Windows"), File_(@"C:\p\a.txt"));

        Assert.Equal(@"C:\p\a.txt", Assert.Single(plan.Deletions).SourcePath);
        Assert.Single(plan.Problems);
    }

    // --- the Recycle Bin ---

    /// <summary>
    /// Where each item is really going is decided here rather than in the executor, so the
    /// confirmation the user answers can say what will actually happen to it.
    /// </summary>
    private DeletePlan PlanWithBin(FakeRecycleProbe bin, DeleteMode mode, params DeleteSource[] sources) =>
        new DeletePlanner(_probe, [@"C:\Windows"], bin).Plan(sources, mode);

    [Fact]
    public void OnAVolumeWithABin_AnOrdinaryDelete_Recycles()
    {
        _probe.AddFile(@"C:\p\a.txt");
        var bin = new FakeRecycleProbe(@"C:\");

        var plan = PlanWithBin(bin, DeleteMode.Recycle, File_(@"C:\p\a.txt"));

        Assert.Equal(DeleteDisposition.Recycle, Assert.Single(plan.Deletions).Disposition);
        Assert.False(plan.HasStagedFallback);
    }

    /// <summary>
    /// The rule that stops a network share turning a delete into an erase: with no bin to hand the
    /// item to, it goes into this app's own holding folder, where Ctrl+Z can still reach it.
    /// </summary>
    [Fact]
    public void OnAVolumeWithNoBin_AnOrdinaryDelete_FallsBackToTheHoldingFolder()
    {
        _probe.AddFile(@"\\server\share\a.txt");
        var bin = new FakeRecycleProbe(@"C:\");

        var plan = PlanWithBin(bin, DeleteMode.Recycle, File_(@"\\server\share\a.txt"));

        Assert.Equal(DeleteDisposition.Stage, Assert.Single(plan.Deletions).Disposition);
    }

    /// <summary>A mixed selection is exactly what the fallback exists for, and the confirmation has
    /// to be able to say that part of it is taking a different route.</summary>
    [Fact]
    public void AMixedSelection_RoutesEachItemOnItsOwnVolume_AndSaysSo()
    {
        _probe.AddFile(@"C:\p\a.txt");
        _probe.AddFile(@"\\server\share\b.txt");
        var bin = new FakeRecycleProbe(@"C:\");

        var plan = PlanWithBin(
            bin, DeleteMode.Recycle, File_(@"C:\p\a.txt"), File_(@"\\server\share\b.txt"));

        Assert.Equal(DeleteDisposition.Recycle, plan.Deletions[0].Disposition);
        Assert.Equal(DeleteDisposition.Stage, plan.Deletions[1].Disposition);
        Assert.True(plan.HasStagedFallback);
    }

    [Fact]
    public void AShiftDelete_ErasesEvenWhereABinExists()
    {
        _probe.AddFile(@"C:\p\a.txt");
        var bin = new FakeRecycleProbe(@"C:\");

        var plan = PlanWithBin(bin, DeleteMode.Permanent, File_(@"C:\p\a.txt"));

        Assert.Equal(DeleteDisposition.Erase, Assert.Single(plan.Deletions).Disposition);
        Assert.True(plan.Permanent);
        Assert.False(plan.HasStagedFallback);
    }

    /// <summary>Asking for the holding folder explicitly never consults the bin at all.</summary>
    [Fact]
    public void AStagedDelete_IgnoresTheBinEntirely()
    {
        _probe.AddFile(@"C:\p\a.txt");
        var bin = new FakeRecycleProbe(@"C:\");

        var plan = PlanWithBin(bin, DeleteMode.Staged, File_(@"C:\p\a.txt"));

        Assert.Equal(DeleteDisposition.Stage, Assert.Single(plan.Deletions).Disposition);
        Assert.False(bin.WasAsked);
    }

    /// <summary>Core on its own has no bin to ask about, so it must not assume one is there.</summary>
    [Fact]
    public void WithNoRecycleProbeInjected_EverythingIsHeldRatherThanRecycled()
    {
        _probe.AddFile(@"C:\p\a.txt");

        var plan = _planner.Plan([File_(@"C:\p\a.txt")], DeleteMode.Recycle);

        Assert.Equal(DeleteDisposition.Stage, Assert.Single(plan.Deletions).Disposition);
    }

    // --- the bin's own contents ---

    /// <summary>
    /// Those <c>$R</c> files are what Ctrl+Z restores from, so deleting one out from under a pending
    /// undo would quietly break it — and this app is elevated, so Windows will not object.
    /// </summary>
    [Theory]
    [InlineData(@"C:\$Recycle.Bin")]
    [InlineData(@"C:\$Recycle.Bin\S-1-5-21-1\$RAB1234.txt")]
    [InlineData(@"C:\$RECYCLE.BIN\S-1-5-21-1\$RAB1234.txt")]
    [InlineData(@"C:\RECYCLER\S-1-5-21-1\Dc1.txt")]
    public void AnythingInTheRecycleBin_IsRefused(string path)
    {
        _probe.AddFile(path);
        _probe.AddDirectory(path);

        var plan = Plan(File_(path));

        Assert.Empty(plan.Deletions);
        Assert.Equal(DeleteRejection.ProtectedLocation, ReasonFor(plan, path));
    }

    [Fact]
    public void AFolderMerelyNamedLikeTheBin_DeeperInATree_IsStillRefused()
    {
        // Conservative on purpose: a name test cannot tell a real bin from a decoy, and refusing to
        // delete something is recoverable in a way that deleting it is not.
        _probe.AddDirectory(@"C:\p\$Recycle.Bin\inner");

        Assert.Empty(Plan(Dir(@"C:\p\$Recycle.Bin\inner")).Deletions);
    }

    [Fact]
    public void AnOrdinaryFolderWithADollarInItsName_IsNotMistakenForTheBin()
    {
        _probe.AddDirectory(@"C:\p\$Recycled");

        Assert.Single(Plan(Dir(@"C:\p\$Recycled")).Deletions);
    }
}

/// <summary>
/// A Recycle Bin that exists on some volumes and not others — the case that matters, since a
/// network share or media with the bin turned off is what makes the staged fallback necessary and
/// is not something a unit test can conjure up for real.
/// </summary>
internal sealed class FakeRecycleProbe(params string[] volumesWithABin) : IRecycleProbe
{
    private readonly HashSet<string> _volumes =
        volumesWithABin.Select(PathKey.Canonicalize).ToHashSet(StringComparer.Ordinal);

    public bool WasAsked { get; private set; }

    public bool CanRecycle(string path)
    {
        WasAsked = true;
        var root = Path.GetPathRoot(path);
        return !string.IsNullOrEmpty(root) && _volumes.Contains(PathKey.Canonicalize(root));
    }
}

internal sealed class FakeDeleteProbe : IDeleteProbe
{
    private readonly HashSet<string> _directories = new(StringComparer.Ordinal);
    private readonly HashSet<string> _files = new(StringComparer.Ordinal);

    public void AddDirectory(string path) => _directories.Add(PathKey.Canonicalize(path));

    public void AddFile(string path) => _files.Add(PathKey.Canonicalize(path));

    public bool DirectoryExists(string path) => _directories.Contains(PathKey.Canonicalize(path));

    public bool FileExists(string path) => _files.Contains(PathKey.Canonicalize(path));
}
