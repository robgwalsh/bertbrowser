using System.Text.Json;
using BertBrowser.Core.Layout;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Restoring a session reads a file that a previous version of this app wrote, or that somebody
/// edited by hand. Every rule here exists so that a layout which cannot be honoured degrades to the
/// ordinary single-pane start — a session that will not open is far worse than one that opens
/// somewhere unexpected.
/// </summary>
public sealed class SessionLayoutTests
{
    private static SessionLayout Pane(params string[] paths) => new()
    {
        Tabs = paths.Select(p => new SessionTab { Path = p }).ToList(),
    };

    private static SessionLayout Split(SplitOrientation orientation, params SessionLayout[] children) => new()
    {
        Orientation = orientation,
        Children = children.ToList(),
    };

    /// <summary>Everything exists.</summary>
    private static bool All(string _) => true;

    // --- Round trip ---

    [Fact]
    public void ANestedLayoutSurvivesJson()
    {
        var original = Split(SplitOrientation.Vertical,
            Pane(@"C:\a"),
            Split(SplitOrientation.Horizontal, Pane(@"C:\b", @"C:\c"), Pane(@"C:\d")));
        original.Children![1].Weight = 2.5;
        original.Children[1].Children![0].ActiveTabIndex = 1;

        var restored = JsonSerializer.Deserialize<SessionLayout>(JsonSerializer.Serialize(original))!;

        // Three panes, not four: the middle one holds two tabs.
        Assert.Equal(3, SessionLayoutRules.CountPanes(restored));
        Assert.Equal(2.5, restored.Children![1].Weight);
        Assert.Equal(1, restored.Children[1].Children![0].ActiveTabIndex);
        Assert.Equal(
            [@"C:\a", @"C:\b", @"C:\c", @"C:\d"],
            SessionLayoutRules.Panes(restored).SelectMany(p => p.Tabs!).Select(t => t.Path));
    }

    /// <summary>On-screen order, which is what "focus the next pane" walks.</summary>
    [Fact]
    public void PanesComeBackInScreenOrder()
    {
        var layout = Split(SplitOrientation.Vertical,
            Pane(@"C:\left"),
            Split(SplitOrientation.Horizontal, Pane(@"C:\top"), Pane(@"C:\bottom")));

        Assert.Equal(
            [@"C:\left", @"C:\top", @"C:\bottom"],
            SessionLayoutRules.Panes(layout).Select(p => p.Tabs![0].Path));
    }

    // --- Pruning ---

    [Fact]
    public void AMissingPathIsDroppedAndTheSurvivorsKeepTheirWeights()
    {
        var layout = Split(SplitOrientation.Vertical, Pane(@"C:\gone", @"C:\here"), Pane(@"C:\also"));
        layout.Children![0].Weight = 3;

        var pruned = SessionLayoutRules.Prune(layout, p => p != @"C:\gone")!;

        Assert.Equal(2, SessionLayoutRules.CountPanes(pruned));
        Assert.Equal(3, pruned.Children![0].Weight);
        Assert.Equal(@"C:\here", Assert.Single(pruned.Children[0].Tabs!).Path);
    }

    /// <summary>An unplugged drive takes its pane with it rather than leaving an empty one.</summary>
    [Fact]
    public void APaneWhoseEveryTabIsGoneIsDropped()
    {
        var layout = Split(SplitOrientation.Vertical, Pane(@"D:\removable"), Pane(@"C:\here"));

        var pruned = SessionLayoutRules.Prune(layout, p => p.StartsWith(@"C:\"))!;

        Assert.Equal(1, SessionLayoutRules.CountPanes(pruned));
        Assert.Equal(@"C:\here", pruned.Tabs![0].Path);
    }

    /// <summary>
    /// A split left with one child is not a split. Keeping the empty level would restore an
    /// arrangement the live tree forbids — splitters with nothing on one side.
    /// </summary>
    [Fact]
    public void ASplitLeftWithOneChildCollapsesIntoIt()
    {
        var layout = Split(SplitOrientation.Vertical, Pane(@"C:\gone"), Pane(@"C:\here"));
        layout.Weight = 4;

        var pruned = SessionLayoutRules.Prune(layout, p => p == @"C:\here")!;

        Assert.False(pruned.IsSplit);
        Assert.Equal(4, pruned.Weight);
    }

    [Fact]
    public void NothingLeftIsNullSoTheCallerStartsNormally()
    {
        var layout = Split(SplitOrientation.Vertical, Pane(@"C:\gone"), Pane(@"D:\also-gone"));

        Assert.Null(SessionLayoutRules.Prune(layout, _ => false));
        Assert.False(SessionLayoutRules.IsUsable(null));
    }

    [Fact]
    public void AnActiveTabIndexPastTheEndIsClamped()
    {
        var pane = Pane(@"C:\a", @"C:\b");
        pane.ActiveTabIndex = 99;

        var pruned = SessionLayoutRules.Prune(pane, All)!;

        Assert.Equal(1, pruned.ActiveTabIndex);
    }

    /// <summary>The index follows what survived, not what was saved.</summary>
    [Fact]
    public void AnActiveTabIndexIsClampedAfterPruning()
    {
        var pane = Pane(@"C:\a", @"C:\gone", @"C:\gone2");
        pane.ActiveTabIndex = 2;

        var pruned = SessionLayoutRules.Prune(pane, p => p == @"C:\a")!;

        Assert.Equal(0, pruned.ActiveTabIndex);
    }

    /// <summary>
    /// A weight of zero, NaN or infinity would give a Grid a column it can never lay out. Hand
    /// editing settings.json is the expected source of these.
    /// </summary>
    [Theory]
    [InlineData(0)]
    [InlineData(-1)]
    [InlineData(double.NaN)]
    [InlineData(double.PositiveInfinity)]
    public void AnUnusableWeightBecomesAnEvenShare(double weight)
    {
        var pane = Pane(@"C:\a");
        pane.Weight = weight;

        Assert.Equal(1, SessionLayoutRules.Prune(pane, All)!.Weight);
    }

    [Fact]
    public void EmptyAndBlankPathsAreDroppedRatherThanRestoredAsBlankTabs()
    {
        var pane = new SessionLayout
        {
            Tabs = [new SessionTab { Path = "" }, new SessionTab { Path = @"C:\real" }],
        };

        var pruned = SessionLayoutRules.Prune(pane, All)!;

        Assert.Equal(@"C:\real", Assert.Single(pruned.Tabs!).Path);
    }

    // --- Guards against a file claiming an absurd arrangement ---

    [Fact]
    public void MorePanesThanTheCapIsRefusedRatherThanBuilt()
    {
        var children = Enumerable.Range(0, SessionLayoutRules.MaxPanes + 1)
            .Select(i => Pane($@"C:\p{i}"))
            .ToArray();

        var layout = SessionLayoutRules.Prune(Split(SplitOrientation.Vertical, children), All);

        Assert.False(SessionLayoutRules.IsUsable(layout));
    }

    [Fact]
    public void TooManyTabsAreTruncatedRatherThanRefused()
    {
        var paths = Enumerable.Range(0, SessionLayoutRules.MaxTabsPerPane + 10)
            .Select(i => $@"C:\t{i}")
            .ToArray();

        var pruned = SessionLayoutRules.Prune(Pane(paths), All)!;

        Assert.Equal(SessionLayoutRules.MaxTabsPerPane, pruned.Tabs!.Count);
    }

    /// <summary>A node claiming to be both is read as a split, and the stray tabs are dropped —
    /// rather than a rebuild finding a pane with children hanging off it.</summary>
    [Fact]
    public void ANodeThatIsBothIsResolvedToASplit()
    {
        var confused = Split(SplitOrientation.Vertical, Pane(@"C:\a"), Pane(@"C:\b"));
        confused.Tabs = [new SessionTab { Path = @"C:\stray" }];

        var pruned = SessionLayoutRules.Prune(confused, All)!;

        Assert.True(pruned.IsSplit);
        Assert.Null(pruned.Tabs);
    }
}
