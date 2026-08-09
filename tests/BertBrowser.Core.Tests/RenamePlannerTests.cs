using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Rename;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The rules that decide whether a rename may go ahead. Run against a fake filesystem, because what
/// matters is which names are taken and by whom — a name held by another item in the same batch is
/// only free if that item is really leaving it.
/// </summary>
public sealed class RenamePlannerTests
{
    private readonly FakeRenameProbe _probe = new();
    private readonly RenamePlanner _planner;

    public RenamePlannerTests() => _planner = new RenamePlanner(_probe);

    private static RenameSource File_(string path) => new(path, IsDirectory: false);

    private static RenameSource Dir(string path) => new(path, IsDirectory: true);

    private static RenameRejection ReasonFor(RenamePlan plan, string source) =>
        plan.Rejected.Single(r => r.SourcePath.Equals(source, StringComparison.OrdinalIgnoreCase)).Reason;

    private static string TargetOf(RenamePlan plan, string source) =>
        plan.Renames.Single(r => r.SourcePath.Equals(source, StringComparison.OrdinalIgnoreCase)).TargetName;

    // --- the ordinary cases ---

    [Fact]
    public void OneFile_IsPlannedIntoItsOwnFolder()
    {
        _probe.AddFile(@"C:\p\old.txt");

        var plan = _planner.Plan([File_(@"C:\p\old.txt")], "new.txt");

        Assert.Empty(plan.Rejected);
        Assert.Equal(@"C:\p\new.txt", plan.Renames.Single().TargetPath);
    }

    [Fact]
    public void SeveralFiles_AreNumbered()
    {
        _probe.AddFile(@"C:\p\a.jpg");
        _probe.AddFile(@"C:\p\b.jpg");

        var plan = _planner.Plan([File_(@"C:\p\a.jpg"), File_(@"C:\p\b.jpg")], "Trip");

        Assert.Empty(plan.Rejected);
        Assert.Equal("Trip 1.jpg", TargetOf(plan, @"C:\p\a.jpg"));
        Assert.Equal("Trip 2.jpg", TargetOf(plan, @"C:\p\b.jpg"));
    }

    [Fact]
    public void RenamingToTheSameNameIsPlannedButIsNoWork()
    {
        _probe.AddFile(@"C:\p\a.txt");

        var plan = _planner.Plan([File_(@"C:\p\a.txt")], "a.txt");

        Assert.True(plan.Renames.Single().IsNoOp);
        Assert.False(plan.HasWork);
        Assert.Empty(plan.Work);
    }

    [Fact]
    public void ChangingOnlyTheCasing_IsRealWork()
    {
        _probe.AddFile(@"C:\p\readme.md");

        var plan = _planner.Plan([File_(@"C:\p\readme.md")], "README.md");

        Assert.True(plan.HasWork);
        Assert.Empty(plan.Rejected);
    }

    [Fact]
    public void TheSameItemListedTwice_IsRenamedOnce()
    {
        _probe.AddFile(@"C:\p\a.txt");

        var plan = _planner.Plan([File_(@"C:\p\a.txt"), File_(@"C:\P\A.TXT")], "b.txt");

        Assert.Equal("b.txt", plan.Renames.Single().TargetName); // not numbered — it is one item
    }

    // --- nothing is ever renamed over something else ---

    [Fact]
    public void ANameHeldBySomethingElse_IsRefused()
    {
        _probe.AddFile(@"C:\p\a.txt");
        _probe.AddFile(@"C:\p\taken.txt");

        var plan = _planner.Plan([File_(@"C:\p\a.txt")], "taken.txt");

        Assert.False(plan.HasWork);
        Assert.Equal(RenameRejection.NameTaken, ReasonFor(plan, @"C:\p\a.txt"));
    }

    [Fact]
    public void ANameHeldByAFolder_IsRefusedForAFileToo()
    {
        _probe.AddFile(@"C:\p\a.txt");
        _probe.AddDirectory(@"C:\p\taken.txt");

        var plan = _planner.Plan([File_(@"C:\p\a.txt")], "taken.txt");

        Assert.Equal(RenameRejection.NameTaken, ReasonFor(plan, @"C:\p\a.txt"));
    }

    [Fact]
    public void OneTakenNameInABatch_OnlyRefusesTheItemThatWantedIt()
    {
        _probe.AddDirectory(@"C:\p\one");
        _probe.AddDirectory(@"C:\p\two");
        _probe.AddDirectory(@"C:\p\three");
        _probe.AddDirectory(@"C:\p\Work 2"); // in the way of the second one

        var plan = _planner.Plan(
            [Dir(@"C:\p\one"), Dir(@"C:\p\two"), Dir(@"C:\p\three")], "Work");

        Assert.Equal(RenameRejection.NameTaken, ReasonFor(plan, @"C:\p\two"));
        Assert.Equal(2, plan.Work.Count);
    }

    // --- a name held by another selected item ---

    [Fact]
    public void RenamingASetOntoItsOwnNames_IsAllowed()
    {
        // Re-running the same numbered rename: every target is a name the batch already holds.
        _probe.AddFile(@"C:\p\Trip 1.jpg");
        _probe.AddFile(@"C:\p\Trip 2.jpg");

        var plan = _planner.Plan(
            [File_(@"C:\p\Trip 2.jpg"), File_(@"C:\p\Trip 1.jpg")], "Trip");

        Assert.Empty(plan.Rejected);
        Assert.Equal("Trip 1.jpg", TargetOf(plan, @"C:\p\Trip 2.jpg"));
        Assert.Equal("Trip 2.jpg", TargetOf(plan, @"C:\p\Trip 1.jpg"));
    }

    [Fact]
    public void AimingAtTheNameOfASelectedItemThatIsNotMoving_IsRefused()
    {
        // "b.txt" is selected but its own rename is refused (the name is taken), so it stays put —
        // and the item aiming at "b.txt" must be refused as well rather than overwrite it.
        _probe.AddDirectory(@"C:\p\keep");
        _probe.AddDirectory(@"C:\p\Work 2");
        _probe.AddDirectory(@"C:\p\Work 1");

        var plan = _planner.Plan([Dir(@"C:\p\keep"), Dir(@"C:\p\Work 1")], "Work");

        // "keep" -> "Work 1" would land on "Work 1", which is only leaving if its own rename to
        // "Work 2" succeeds — it does not, because an unrelated "Work 2" is already there.
        Assert.Equal(RenameRejection.NameTaken, ReasonFor(plan, @"C:\p\Work 1"));
        Assert.Equal(RenameRejection.NameTaken, ReasonFor(plan, @"C:\p\keep"));
        Assert.False(plan.HasWork);
    }

    [Fact]
    public void AnItemThePatternLeavesAlone_StillCountsAsPlanned()
    {
        _probe.AddDirectory(@"C:\p\Work 1");
        _probe.AddDirectory(@"C:\p\other");

        var plan = _planner.Plan([Dir(@"C:\p\Work 1"), Dir(@"C:\p\other")], "Work");

        Assert.Empty(plan.Rejected);
        Assert.True(plan.Renames.Single(r => r.SourcePath == @"C:\p\Work 1").IsNoOp);
        Assert.Single(plan.Work); // only "other" is written
    }

    // --- refusals ---

    [Fact]
    public void AMissingItem_IsRefused()
    {
        var plan = _planner.Plan([File_(@"C:\p\gone.txt")], "new.txt");

        Assert.Equal(RenameRejection.SourceMissing, ReasonFor(plan, @"C:\p\gone.txt"));
    }

    [Fact]
    public void ADriveRoot_IsRefused()
    {
        _probe.AddDirectory(@"C:\");

        var plan = _planner.Plan([Dir(@"C:\")], "data");

        Assert.Equal(RenameRejection.SourceIsRoot, ReasonFor(plan, @"C:\"));
    }

    [Theory]
    [InlineData("")]
    [InlineData("a/b")]
    [InlineData("CON")]
    public void AnIllegalName_IsRefused(string pattern)
    {
        _probe.AddFile(@"C:\p\a.txt");

        var plan = _planner.Plan([File_(@"C:\p\a.txt")], pattern);

        Assert.Equal(RenameRejection.InvalidName, ReasonFor(plan, @"C:\p\a.txt"));
        Assert.False(plan.HasWork);
    }

    [Fact]
    public void AnIllegalPatternInABatch_RefusesEveryItem()
    {
        _probe.AddFile(@"C:\p\a.txt");
        _probe.AddFile(@"C:\p\b.txt");

        var plan = _planner.Plan([File_(@"C:\p\a.txt"), File_(@"C:\p\b.txt")], "a/b");

        Assert.Equal(2, plan.Rejected.Count);
        Assert.All(plan.Rejected, r => Assert.Equal(RenameRejection.InvalidName, r.Reason));
        Assert.False(plan.HasWork);
    }

    [Fact]
    public void ABatchSpanningTwoFolders_NumbersAcrossBothAndRenamesInPlace()
    {
        // Search results are flat, so a selection can span folders; each item stays where it is.
        _probe.AddFile(@"C:\p\a.txt");
        _probe.AddFile(@"C:\q\b.txt");

        var plan = _planner.Plan([File_(@"C:\p\a.txt"), File_(@"C:\q\b.txt")], "Note");

        Assert.Empty(plan.Rejected);
        Assert.Equal(@"C:\p\Note 1.txt", plan.Renames[0].TargetPath);
        Assert.Equal(@"C:\q\Note 2.txt", plan.Renames[1].TargetPath);
    }

    [Fact]
    public void AnItemInsideAFolderBeingRenamed_IsRefused()
    {
        // A flattened search result can hold both, and renaming the folder first would move the
        // other item's path out from under it.
        _probe.AddDirectory(@"C:\p\box");
        _probe.AddFile(@"C:\p\box\inner.txt");

        var plan = _planner.Plan([Dir(@"C:\p\box"), File_(@"C:\p\box\inner.txt")], "Thing");

        Assert.Equal(RenameRejection.InsideARenamedFolder, ReasonFor(plan, @"C:\p\box\inner.txt"));
        Assert.Single(plan.Work);
    }

    [Fact]
    public void NoSources_PlansNothing() =>
        Assert.False(_planner.Plan([], "anything").HasWork);
}

/// <summary>An in-memory filesystem: which paths exist, and whether each is a file or a folder.</summary>
internal sealed class FakeRenameProbe : IRenameProbe
{
    private readonly HashSet<string> _directories = new(StringComparer.Ordinal);
    private readonly HashSet<string> _files = new(StringComparer.Ordinal);

    public void AddDirectory(string path) => _directories.Add(PathKey.Canonicalize(path));

    public void AddFile(string path) => _files.Add(PathKey.Canonicalize(path));

    public bool DirectoryExists(string path) => _directories.Contains(PathKey.Canonicalize(path));

    public bool FileExists(string path) => _files.Contains(PathKey.Canonicalize(path));
}
