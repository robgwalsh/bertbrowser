using BertBrowser.Core.Layout;
using BertBrowser.Core.Services.SavedWorkspaces;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class SavedWorkspaceRulesTests
{
    private static readonly Func<string, bool> NoNameTaken = _ => false;

    [Fact]
    public void AValidNameHasNoProblem() => Assert.Null(SavedWorkspaceRules.Validate("Work", NoNameTaken));

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    public void ABlankNameIsRefused(string name) =>
        Assert.Contains("name", SavedWorkspaceRules.Validate(name, NoNameTaken)!, StringComparison.OrdinalIgnoreCase);

    [Fact]
    public void ATakenNameIsRefused()
    {
        var problem = SavedWorkspaceRules.Validate("Work", n => n == "Work");
        Assert.NotNull(problem);
        Assert.Contains("Work", problem);
    }

    [Fact]
    public void ANameOverTheLengthCapIsRefused()
    {
        var tooLong = new string('a', SavedWorkspaceRules.MaxNameLength + 1);
        Assert.NotNull(SavedWorkspaceRules.Validate(tooLong, NoNameTaken));
    }

    [Fact]
    public void ANameAtTheLengthCapIsAccepted()
    {
        var exact = new string('a', SavedWorkspaceRules.MaxNameLength);
        Assert.Null(SavedWorkspaceRules.Validate(exact, NoNameTaken));
    }

    [Fact]
    public void TheNameIsTrimmedBeforeValidating()
    {
        Assert.Null(SavedWorkspaceRules.Validate("  Work  ", NoNameTaken));
    }

    [Fact]
    public void DefaultNameIncludesTheTimestamp()
    {
        var now = new DateTime(2026, 3, 5, 14, 30, 0);
        Assert.Equal("Workspace 2026-03-05 14:30", SavedWorkspaceRules.DefaultName(now));
    }

    private static SessionLayout TwoPanes() => new()
    {
        Orientation = SplitOrientation.Horizontal,
        Children =
        [
            new SessionLayout { Tabs = [new SessionTab { Path = @"C:\A" }, new SessionTab { Path = @"C:\B" }] },
            new SessionLayout { Tabs = [new SessionTab { Path = @"c:\a" }, new SessionTab { Path = @"D:\C" }] },
        ],
    };

    [Fact]
    public void ShapeTextCountsPanesAndTabs()
    {
        Assert.Equal("2 panes, 4 tabs", SavedWorkspaceRules.ShapeText(TwoPanes()));
        Assert.Equal("1 pane, 1 tab",
            SavedWorkspaceRules.ShapeText(new SessionLayout { Tabs = [new SessionTab { Path = @"C:\" }] }));
    }

    [Fact]
    public void FoldersAreListedOnceInPaneThenTabOrderIgnoringCase() =>
        Assert.Equal([@"C:\A", @"C:\B", @"D:\C"], SavedWorkspaceRules.Folders(TwoPanes()));
}
