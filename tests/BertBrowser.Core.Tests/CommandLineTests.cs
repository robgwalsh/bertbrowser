using BertBrowser.Core.Cli;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The command line is how another process asks this one to open something, so a misread argument
/// shows the user the wrong folder — or, if an unrecognised flag were treated as a path, somewhere
/// they never named. Parsing is pure and touches no disk, so all of it is pinned here.
/// </summary>
public sealed class CommandLineTests
{
    private static CommandLineRequest Parse(params string[] args) => CommandLine.Parse(args);

    // --- the ordinary cases ---

    [Fact]
    public void NoArguments_AsksForNothing()
    {
        var request = Parse();

        Assert.False(request.HasTargets);
        Assert.Equal(OpenIn.Default, request.Mode);
        Assert.Empty(request.Errors);
    }

    [Fact]
    public void ABareDirectory_IsTheTarget()
    {
        var target = Assert.Single(Parse(@"C:\Dir").Targets);

        Assert.Equal(@"C:\Dir", target.Path);
        Assert.False(target.Select);
    }

    [Fact]
    public void SeveralPaths_KeepTheirOrder()
    {
        var request = Parse(@"C:\One", @"C:\Two", @"C:\Three");

        Assert.Equal([@"C:\One", @"C:\Two", @"C:\Three"], request.Targets.Select(t => t.Path));
    }

    [Fact]
    public void AUncPath_IsATargetLikeAnyOther()
    {
        Assert.Equal(@"\\server\share", Assert.Single(Parse(@"\\server\share").Targets).Path);
    }

    // --- Explorer's /select ---

    /// <summary>Explorer emits this as one token when no space follows the comma.</summary>
    [Fact]
    public void SelectInOneToken_HighlightsTheItem()
    {
        var target = Assert.Single(Parse(@"/select,C:\Dir\file.txt").Targets);

        Assert.Equal(@"C:\Dir\file.txt", target.Path);
        Assert.True(target.Select);
    }

    /// <summary>…and as two when one does.</summary>
    [Fact]
    public void SelectSplitAcrossTwoTokens_HighlightsTheItem()
    {
        var target = Assert.Single(Parse("/select,", @"C:\Dir\file.txt").Targets);

        Assert.Equal(@"C:\Dir\file.txt", target.Path);
        Assert.True(target.Select);
    }

    [Fact]
    public void TheLongFormOfSelect_WorksToo()
    {
        Assert.True(Assert.Single(Parse("--select", @"C:\Dir\file.txt").Targets).Select);
    }

    [Fact]
    public void SelectAppliesToTheNextPathOnly()
    {
        var request = Parse("--select", @"C:\Dir\file.txt", @"C:\Other");

        Assert.True(request.Targets[0].Select);
        Assert.False(request.Targets[1].Select);
    }

    [Fact]
    public void SelectWithNothingAfterIt_IsReported()
    {
        var request = Parse("--select");

        Assert.Empty(request.Targets);
        Assert.Single(request.Errors);
    }

    [Fact]
    public void SelectIsCaseInsensitive_BecauseExplorerIsNotConsistent()
    {
        Assert.True(Assert.Single(Parse(@"/SELECT,C:\Dir\file.txt").Targets).Select);
    }

    // --- where it opens ---

    [Theory]
    [InlineData("--new-tab")]
    [InlineData("-t")]
    public void NewTab_IsRequestable(string flag) => Assert.Equal(OpenIn.NewTab, Parse(flag).Mode);

    [Theory]
    [InlineData("--new-pane")]
    [InlineData("-p")]
    public void NewPane_IsRequestable(string flag) =>
        Assert.Equal(OpenIn.NewPane, Parse(flag).Mode);

    /// <summary>Silently picking one of two contradictory flags looks exactly like the other one
    /// being ignored.</summary>
    [Fact]
    public void AskingForBothATabAndAPane_IsAnErrorRatherThanACoinFlip()
    {
        Assert.Single(Parse("--new-tab", "--new-pane", @"C:\Dir").Errors);
    }

    [Fact]
    public void RepeatingTheSameFlag_IsNotAContradiction()
    {
        var request = Parse("--new-tab", "-t", @"C:\Dir");

        Assert.Empty(request.Errors);
        Assert.Equal(OpenIn.NewTab, request.Mode);
    }

    [Fact]
    public void FlagsMayComeAfterPaths()
    {
        var request = Parse(@"C:\Dir", "--new-tab");

        Assert.Equal(OpenIn.NewTab, request.Mode);
        Assert.Single(request.Targets);
    }

    // --- what must never happen ---

    /// <summary>A mistyped flag opening the user's profile folder is worse than an error message.</summary>
    [Theory]
    [InlineData("--nwe-tab")]
    [InlineData("--help")]
    [InlineData("/x")]
    [InlineData("-")]
    public void AnUnrecognizedOption_IsReported_NeverTreatedAsAPath(string arg)
    {
        var request = Parse(arg);

        Assert.Empty(request.Targets);
        Assert.Single(request.Errors);
    }

    [Fact]
    public void AnUnrecognizedOption_DoesNotCostTheValidPathsBesideIt()
    {
        var request = Parse("--nope", @"C:\Dir");

        Assert.Equal(@"C:\Dir", Assert.Single(request.Targets).Path);
        Assert.Single(request.Errors);
    }

    /// <summary>
    /// The one everybody hits: <c>"C:\Dir\"</c> reaches argv as <c>C:\Dir"</c>, because the
    /// backslash escaped the closing quote.
    /// </summary>
    [Fact]
    public void ATrailingSeparatorMangledIntoAQuote_IsRepaired()
    {
        Assert.Equal(@"C:\Dir", Assert.Single(Parse(@"C:\Dir""").Targets).Path);
    }

    [Fact]
    public void TheSameManglingIsRepairedAfterSelect()
    {
        Assert.Equal(
            @"C:\Program Files",
            Assert.Single(Parse(@"/select,C:\Program Files""").Targets).Path);
    }

    [Fact]
    public void BlankArguments_AreIgnored()
    {
        var request = Parse("", "   ", @"C:\Dir");

        Assert.Single(request.Targets);
        Assert.Empty(request.Errors);
    }
}
