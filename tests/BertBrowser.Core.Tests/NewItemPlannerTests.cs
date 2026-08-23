using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Creating must be refused before anything is written, because the dialog shows the refusal while
/// the name can still be changed. Every rule here is one the user reads rather than one they hit.
/// </summary>
public sealed class NewItemPlannerTests
{
    private readonly FakeNewItemProbe _probe = new();
    private readonly NewItemPlanner _planner;

    public NewItemPlannerTests()
    {
        _planner = new NewItemPlanner(_probe);
        _probe.AddDirectory(@"C:\p");
    }

    private NewItemPlan PlanFolder(string name) =>
        _planner.Plan(@"C:\p", name, NewItemKind.Folder);

    private NewItemPlan PlanFile(string name, string? template = null) =>
        _planner.Plan(@"C:\p", name, NewItemKind.File, template);

    private static NewItemRejection? ReasonOf(NewItemPlan plan) => plan.Rejected?.Reason;

    // --- the ordinary cases ---

    [Fact]
    public void AFreeNameIsPlanned()
    {
        var plan = PlanFolder("Reports");
        Assert.True(plan.HasWork);
        Assert.Equal(@"C:\p\Reports", plan.TargetPath);
    }

    [Fact]
    public void TrailingDotsAndSpacesAreDroppedBeforeAnythingSeesTheName()
    {
        // Windows drops them silently, so a plan that kept them would describe a path that never
        // appears on disk.
        var plan = PlanFolder("Reports. ");
        Assert.True(plan.HasWork);
        Assert.Equal("Reports", plan.Name);
    }

    // --- refusals ---

    [Fact]
    public void ANameHeldByAFile_IsRefused()
    {
        _probe.AddFile(@"C:\p\notes.txt");
        Assert.Equal(NewItemRejection.NameTaken, ReasonOf(PlanFile("notes.txt")));
    }

    [Fact]
    public void ANameHeldByAFolder_RefusesAFileToo_BecauseTheyShareOneNamespace()
    {
        _probe.AddDirectory(@"C:\p\notes.txt");
        Assert.Equal(NewItemRejection.NameTaken, ReasonOf(PlanFile("notes.txt")));
    }

    [Fact]
    public void AFolderThatHasGoneAway_IsRefused()
    {
        var plan = _planner.Plan(@"C:\gone", "Reports", NewItemKind.Folder);
        Assert.Equal(NewItemRejection.ParentMissing, ReasonOf(plan));
    }

    [Fact]
    public void ATemplateThatIsNotThere_IsRefusedBeforeAnythingIsWritten()
    {
        var plan = PlanFile("letter.rtf", @"C:\templates\letter.rtf");
        Assert.Equal(NewItemRejection.TemplateMissing, ReasonOf(plan));
    }

    [Fact]
    public void ATemplateThatIsThere_IsAccepted()
    {
        _probe.AddFile(@"C:\templates\letter.rtf");
        Assert.True(PlanFile("letter.rtf", @"C:\templates\letter.rtf").HasWork);
    }

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("a/b")]
    [InlineData("a:b")]
    [InlineData("a?b")]
    [InlineData("CON")]
    [InlineData("NUL.txt")]
    public void AnIllegalName_IsRefused(string name) =>
        Assert.Equal(NewItemRejection.InvalidName, ReasonOf(PlanFolder(name)));

    [Fact]
    public void AnIllegalNameIsRefusedInThePatternsOwnWords()
    {
        // Mutate NewItemPattern into a second implementation of the naming rule and this goes red —
        // which is the point of it delegating to RenamePattern rather than restating it.
        var plan = PlanFolder("a?b");
        Assert.Equal(RenamePattern.Validate("a?b"), plan.Rejected!.Message);
    }

    // --- the suggested name ---

    [Fact]
    public void TheSuggestionIsTheBaseNameWhenNothingIsInTheWay() =>
        Assert.Equal("New folder", _planner.SuggestName(@"C:\p", "New folder", NewItemKind.Folder));

    [Fact]
    public void ASuggestionThatIsTakenStepsAsideRatherThanOpeningOnARefusal()
    {
        _probe.AddDirectory(@"C:\p\New folder");
        Assert.Equal("New folder (2)", _planner.SuggestName(@"C:\p", "New folder", NewItemKind.Folder));
    }

    [Fact]
    public void ASuggestedFileNumbersBeforeItsExtension()
    {
        _probe.AddFile(@"C:\p\New Text Document.txt");
        Assert.Equal(
            "New Text Document (2).txt",
            _planner.SuggestName(@"C:\p", "New Text Document", NewItemKind.File, ".txt"));
    }
}

/// <summary>An in-memory filesystem: which paths exist, and whether each is a file or a folder.</summary>
internal sealed class FakeNewItemProbe : INewItemProbe
{
    private readonly HashSet<string> _directories = new(StringComparer.Ordinal);
    private readonly HashSet<string> _files = new(StringComparer.Ordinal);

    public void AddDirectory(string path) => _directories.Add(PathKey.Canonicalize(path));

    public void AddFile(string path) => _files.Add(PathKey.Canonicalize(path));

    public bool DirectoryExists(string path) => _directories.Contains(PathKey.Canonicalize(path));

    public bool FileExists(string path) => _files.Contains(PathKey.Canonicalize(path));
}
