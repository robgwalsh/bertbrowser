using BertBrowser.Core.Services.Compare;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Turning a comparison into what a sync would do. Pure, and worth pinning here rather than in a
/// dialog: this is where a verdict becomes a write, and the one place that decides whether an entry
/// nobody could compare is quietly acted on anyway.
/// </summary>
public sealed class SyncPlannerTests
{
    private const string Left = @"C:\Work\left";
    private const string Right = @"C:\Work\right";

    private static readonly DateTime Noon = new(2026, 3, 14, 12, 0, 0, DateTimeKind.Utc);

    private static CompareEntry File_(string display, long size = 10, DateTime? modified = null) =>
        new(display.ToUpperInvariant(), Path.GetFileName(display), false, size, modified ?? Noon);

    private static CompareEntry Folder(string display) =>
        new(display.ToUpperInvariant(), Path.GetFileName(display), true, 0, Noon);

    private static FolderCompareOutcome Compared(CompareEntry[] left, CompareEntry[] right) =>
        new(Left, Right,
            FolderComparer.Compare(left, right, CompareTolerance.Strict),
            CompareAvailability.Ready, CompareSourceKind.Index, CompareSourceKind.Index,
            Truncated: false, Cancelled: false, Problem: null);

    private static SyncPreview Preview(
        CompareEntry[] left, CompareEntry[] right, bool removeRightOnly = false) =>
        SyncPlanner.Preview(Compared(left, right), removeRightOnly);

    private static SyncAction Action(SyncPreview preview, string relativeKey) =>
        Assert.Single(preview.Actions, a => a.RelativeKey == relativeKey);

    /// <summary>Knows about the right side's files, so a deletion is not refused for naming
    /// something that (on this machine, in this test) was never there.</summary>
    private static FakeDeleteProbe DeleteProbe(params string[] names)
    {
        var probe = new FakeDeleteProbe();
        probe.AddDirectory(Right);
        foreach (var name in names) probe.AddFile(Path.Combine(Right, name));
        return probe;
    }

    // --- what becomes an action ---

    [Fact]
    public void TwoIdenticalTreesNeedNoSync()
    {
        var tree = new[] { Folder("src"), File_(@"src\a.cs") };

        var preview = Preview(tree, tree);

        Assert.Empty(preview.Actions);
        Assert.False(preview.HasWork);
    }

    [Fact]
    public void AFileOnlyOnTheLeftIsCopied()
    {
        var preview = Preview([File_("new.txt")], []);

        var action = Action(preview, "NEW.TXT");
        Assert.Equal(SyncActionKind.Copy, action.Kind);
        Assert.True(action.Ticked);
        Assert.Equal(@"C:\Work\left\new.txt", action.SourcePath);
        Assert.Equal(@"C:\Work\right\new.txt", action.TargetPath);
    }

    [Fact]
    public void AFileTheLeftHasUpdatedIsOverwritten()
    {
        var preview = Preview(
            [File_("a.txt", modified: Noon.AddHours(1))],
            [File_("a.txt")]);

        var action = Action(preview, "A.TXT");
        Assert.Equal(SyncActionKind.Overwrite, action.Kind);
        Assert.True(action.Ticked);
    }

    /// <summary>
    /// Still shown, because "make the right side match" is not finished while it holds something
    /// else — but not ticked, because overwriting the newer of two files is the one write nobody
    /// would expect to have agreed to by asking for a sync.
    /// </summary>
    [Fact]
    public void OverwritingTheNewerSideIsOfferedButNotTicked()
    {
        var preview = Preview(
            [File_("a.txt")],
            [File_("a.txt", modified: Noon.AddHours(1))]);

        var action = Action(preview, "A.TXT");
        Assert.Equal(SyncActionKind.Overwrite, action.Kind);
        Assert.False(action.Ticked);
        Assert.False(preview.HasWork);
    }

    /// <summary>The destructive half is opt-in, and off is what the dialog opens with.</summary>
    [Fact]
    public void WhatOnlyTheRightHasIsLeftAloneUnlessAskedFor()
    {
        Assert.Empty(Preview([], [File_("extra.txt")]).Actions);

        var action = Action(Preview([], [File_("extra.txt")], removeRightOnly: true), "EXTRA.TXT");
        Assert.Equal(SyncActionKind.Delete, action.Kind);
        Assert.Equal(@"C:\Work\right\extra.txt", action.TargetPath);
        Assert.Equal("", action.SourcePath);
    }

    /// <summary>
    /// The rule the whole safety story rests on. An entry no verdict could be reached for is
    /// counted and left alone — never copied, and above all never deleted.
    /// </summary>
    [Fact]
    public void AnEntryNobodyCouldCompareProducesNoActionAtAll()
    {
        var preview = Preview(
            [File_("odd.txt", modified: DateTime.MinValue)],
            [File_("odd.txt", modified: DateTime.MinValue)],
            removeRightOnly: true);

        Assert.Empty(preview.Actions);
        Assert.Equal(1, preview.UnknownCount);
    }

    // --- folders are whole ---

    [Fact]
    public void AFolderTheRightSideLacksIsOneActionCoveringEverythingInIt()
    {
        var left = new[]
        {
            Folder("bin"), Folder(@"bin\deep"), File_(@"bin\deep\app.exe"), File_(@"bin\readme.md"),
        };

        var preview = Preview(left, []);

        var action = Assert.Single(preview.Actions);
        Assert.Equal("BIN", action.RelativeKey);
        Assert.True(action.IsDirectory);
    }

    [Fact]
    public void AFolderOnlyTheRightSideHasIsOneDeletion()
    {
        var right = new[] { Folder("old"), File_(@"old\a.txt"), File_(@"old\b.txt") };

        var action = Assert.Single(Preview([], right, removeRightOnly: true).Actions);

        Assert.Equal("OLD", action.RelativeKey);
        Assert.True(action.IsDirectory);
    }

    /// <summary>A folder both sides have is opened rather than replaced, or a sync of one changed
    /// file would rewrite everything beside it.</summary>
    [Fact]
    public void AFolderBothSidesHaveIsDescendedIntoInstead()
    {
        var left = new[] { Folder("src"), File_(@"src\a.cs", size: 10), File_(@"src\b.cs") };
        var right = new[] { Folder("src"), File_(@"src\a.cs", size: 99), File_(@"src\b.cs") };

        var action = Assert.Single(Preview(left, right).Actions);

        Assert.Equal(@"SRC\A.CS", action.RelativeKey);
        Assert.False(action.IsDirectory);
    }

    /// <summary>The awkward pair: a file where the other side has a folder. One action takes over
    /// the name, and nothing inside the folder gets an action of its own.</summary>
    [Fact]
    public void AFileReplacingAFolderIsStillOneAction()
    {
        var preview = Preview(
            [File_("thing")],
            [Folder("thing"), File_(@"thing\inner.txt")],
            removeRightOnly: true);

        var action = Assert.Single(preview.Actions);
        Assert.Equal("THING", action.RelativeKey);
        Assert.Equal(SyncActionKind.Overwrite, action.Kind);
    }

    // --- weights ---

    [Fact]
    public void AFolderWeighsWhatIsInsideIt()
    {
        var left = new[] { Folder("bin"), File_(@"bin\a", size: 100), File_(@"bin\deep\b", size: 25) };

        Assert.Equal(125, Action(Preview(left, []), "BIN").Bytes);
    }

    [Fact]
    public void TheTotalCountsOnlyWhatWouldBeWritten()
    {
        var preview = Preview(
            [File_("keep.txt", size: 40)],
            [File_("gone.txt", size: 9_000)],
            removeRightOnly: true);

        Assert.Equal(40, preview.TotalBytes);
        Assert.Equal(1, preview.CopyCount);
        Assert.Equal(1, preview.DeleteCount);
    }

    [Fact]
    public void AnUntickedActionIsNotInTheTotal()
    {
        var preview = Preview([File_("a.txt", size: 40)], []);
        var none = SyncPlanner.WithTicks(preview, new HashSet<string>(StringComparer.Ordinal));

        Assert.Equal(0, none.TotalBytes);
        Assert.False(none.HasWork);
    }

    /// <summary>Unticking a folder unticks everything inside it, because everything inside it is
    /// that one action.</summary>
    [Fact]
    public void UntickingAFolderRemovesEverythingUnderIt()
    {
        var preview = Preview([Folder("bin"), File_(@"bin\app.exe"), File_("loose.txt")], []);
        var kept = SyncPlanner.WithTicks(preview, new HashSet<string>(["LOOSE.TXT"], StringComparer.Ordinal));

        Assert.Equal("LOOSE.TXT", Assert.Single(kept.Ticked).RelativeKey);
    }

    // --- a compare that cannot be trusted ---

    /// <summary>
    /// A truncated scan describes a prefix of a folder, not the folder. Read off it, "only on the
    /// right" names files the left side has and would have offered to delete them.
    /// </summary>
    [Fact]
    public void ATruncatedComparisonProducesNothing()
    {
        var compare = Compared([], [File_("a.txt")]) with { Truncated = true };

        Assert.Empty(SyncPlanner.Preview(compare, removeRightOnly: true).Actions);
    }

    [Fact]
    public void ARefusedComparisonProducesNothing()
    {
        var compare = FolderCompareOutcome.Refused(Left, Right, "no");

        Assert.Empty(SyncPlanner.Preview(compare, removeRightOnly: true).Actions);
    }

    // --- into the executors' own plans ---

    [Fact]
    public void EachDestinationFolderGetsItsOwnTransferPlan()
    {
        var probe = new FakeProbe();
        probe.AddDirectory(Left);
        probe.AddDirectory(Right);
        probe.AddDirectory(@"C:\Work\left\src");
        probe.AddDirectory(@"C:\Work\right\src");
        probe.AddFile(@"C:\Work\left\top.txt");
        probe.AddFile(@"C:\Work\left\src\deep.txt");

        var left = new[] { Folder("src"), File_("top.txt"), File_(@"src\deep.txt") };
        var right = new[] { Folder("src") };

        var plans = SyncPlanner.ToPlans(
            Preview(left, right), new TransferPlanner(probe), new DeletePlanner(DeleteProbe("gone.txt")), DeleteMode.Recycle);

        Assert.Equal(2, plans.Copies.Count);
        Assert.Equal(Right, plans.Copies[0].DestinationDirectory);
        Assert.Equal(@"C:\Work\right\src", plans.Copies[1].DestinationDirectory);
        Assert.All(plans.Copies, p => Assert.Equal(TransferVerb.Copy, p.Verb));
    }

    /// <summary>
    /// Every sync copy overwrites, including the ones the comparison found a free name for. A file
    /// that appeared on the right in between is a name to take over, not one to sidestep with a
    /// silent "x (2)" that leaves the sync looking done.
    /// </summary>
    [Fact]
    public void EverySyncCopyIsGivenTheOverwriteResolution()
    {
        var probe = new FakeProbe();
        probe.AddDirectory(Left);
        probe.AddDirectory(Right);
        probe.AddFile(@"C:\Work\left\fresh.txt");

        var plans = SyncPlanner.ToPlans(
            Preview([File_("fresh.txt")], []), new TransferPlanner(probe),
            new DeletePlanner(DeleteProbe("gone.txt")), DeleteMode.Recycle);

        Assert.Equal(ConflictResolution.Overwrite, Assert.Single(plans.Resolutions).Value);
    }

    [Fact]
    public void DeletionsBecomeADeletePlanInTheModeAsked()
    {
        var probe = new FakeProbe();
        probe.AddDirectory(Left);
        probe.AddDirectory(Right);

        var plans = SyncPlanner.ToPlans(
            Preview([], [File_("gone.txt")], removeRightOnly: true),
            new TransferPlanner(probe), new DeletePlanner(DeleteProbe("gone.txt")), DeleteMode.Recycle);

        Assert.Empty(plans.Copies);
        Assert.Equal(DeleteMode.Recycle, plans.Removals.Mode);
        Assert.Equal(1, plans.ItemCount);
    }

    private sealed class FakeProbe : ITransferProbe
    {
        private readonly HashSet<string> _dirs = new(StringComparer.OrdinalIgnoreCase);
        private readonly HashSet<string> _files = new(StringComparer.OrdinalIgnoreCase);

        public void AddDirectory(string path) => _dirs.Add(path);
        public void AddFile(string path) => _files.Add(path);

        public bool DirectoryExists(string path) => _dirs.Contains(path);
        public bool FileExists(string path) => _files.Contains(path);
        public string ResolveFinalPath(string path) => path;
    }
}
