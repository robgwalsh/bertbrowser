using BertBrowser.Core.Services.CommandBars;
using Xunit;

namespace BertBrowser.Core.Tests;

public class SavedCommandBarRulesTests
{
    private static readonly Func<string, bool> NoNameTaken = _ => false;

    private static CommandBarSet With(string command) =>
        new(1, [BarItem.Menu("&File", BarItem.Command(command))], new ToolbarSpec([]), []);

    private static SavedCommandBarSet Saved(string name, string command = "tab.new") => new(name, With(command));

    [Fact]
    public void ANameIsRequiredAndKeptShort()
    {
        Assert.NotNull(SavedCommandBarRules.Validate("   ", NoNameTaken));
        Assert.NotNull(SavedCommandBarRules.Validate(new string('x', SavedCommandBarRules.MaxNameLength + 1), NoNameTaken));
        Assert.Null(SavedCommandBarRules.Validate(" Minimal ", NoNameTaken));
    }

    [Fact]
    public void ATakenNameIsRefusedByItsTrimmedSpelling()
    {
        var error = SavedCommandBarRules.Validate(" Minimal ", name => name == "Minimal");

        Assert.Contains("\"Minimal\"", error);
    }

    [Fact]
    public void SavingUnderANameAlreadyThereReplacesItAndTakesTheNewCasing()
    {
        var saved = SavedCommandBarRules.Upsert([Saved("work"), Saved("Alpha")], Saved(" WORK ", "tab.close"));

        Assert.Equal(["Alpha", "WORK"], saved.Select(s => s.Name));
        Assert.True(CommandBarRules.Same(With("tab.close"), saved[1].Set));
    }

    [Fact]
    public void ARenameMovesTheArrangementAndRefusesAnotherOnesName()
    {
        IReadOnlyList<SavedCommandBarSet> saved = [Saved("Alpha"), Saved("Work")];

        Assert.Equal(["Alpha", "Zed"], SavedCommandBarRules.Rename(saved, "work", " Zed ")!.Select(s => s.Name));
        Assert.Equal(["Alpha", "WORK"], SavedCommandBarRules.Rename(saved, "Work", "WORK")!.Select(s => s.Name));
        Assert.Null(SavedCommandBarRules.Rename(saved, "Work", "alpha"));
        Assert.Null(SavedCommandBarRules.Rename(saved, "Missing", "Zed"));
    }

    [Fact]
    public void RemovingIgnoresCase()
    {
        Assert.Equal(["Alpha"], SavedCommandBarRules.Remove([Saved("Alpha"), Saved("Work")], "WORK").Select(s => s.Name));
    }

    [Fact]
    public void TheCurrentArrangementIsRecognisedAsTheSavedOneItEquals()
    {
        SavedCommandBarSet[] saved = [Saved("Alpha", "tab.new"), Saved("Work", "tab.close")];

        Assert.Equal("Work", SavedCommandBarRules.NameMatching(saved, With("tab.close")));
        Assert.Null(SavedCommandBarRules.NameMatching(saved, With("nav.up")));
    }

    [Fact]
    public void OnlyAnArrangementKeptNowhereElseNeedsAskingAbout()
    {
        SavedCommandBarSet[] saved = [Saved("Work", "tab.close")];

        Assert.False(SavedCommandBarRules.NeedsConfirmBeforeReplace(CommandBarDefaults.Default, saved));
        Assert.False(SavedCommandBarRules.NeedsConfirmBeforeReplace(With("tab.close"), saved));
        Assert.True(SavedCommandBarRules.NeedsConfirmBeforeReplace(With("nav.up"), saved));
    }

    [Fact]
    public void TheShapeCountsTheTitleBarAsAToolbar()
    {
        Assert.Equal("6 menus, 1 toolbar", SavedCommandBarRules.ShapeText(CommandBarDefaults.Default));
        Assert.Equal("1 menu, 2 toolbars",
            SavedCommandBarRules.ShapeText(With("tab.new") with { Rows = [new ToolbarSpec([])] }));
    }
}
