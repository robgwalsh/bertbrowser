using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// What the menu at the end of a right-drag offers. Entries are greyed rather than dropped, so the
/// menu never rearranges itself under the cursor between one drag and the next — a menu whose items
/// move is a menu you misclick.
/// </summary>
public sealed class RightDropMenuRulesTests
{
    private static RightDropContext Ordinary => new(
        DestinationIsArchive: false,
        SourcesAreVirtual: false,
        MoveHasWork: true,
        CopyHasWork: true,
        ItemCount: 1);

    [Fact]
    public void AnOrdinaryDropOffersMoveThenCopyThenShortcut()
    {
        var entries = RightDropMenuRules.Build(Ordinary);

        Assert.Equal(
            [RightDropVerb.Move, RightDropVerb.Copy, RightDropVerb.Shortcut],
            entries.Select(e => e.Verb));
        Assert.All(entries, e => Assert.True(e.Enabled));
    }

    /// <summary>
    /// Adding to a container rewrites it, and that is always additive — there is no move to offer,
    /// and a shortcut is a file pointing at a path, which an entry inside an archive has not got.
    /// </summary>
    [Fact]
    public void IntoAnArchiveOnlyCopyIsOffered()
    {
        var entries = RightDropMenuRules.Build(Ordinary with { DestinationIsArchive = true });

        Assert.False(Enabled(entries, RightDropVerb.Move));
        Assert.True(Enabled(entries, RightDropVerb.Copy));
        Assert.False(Enabled(entries, RightDropVerb.Shortcut));
    }

    /// <summary>
    /// The counterpart: dragging <em>out</em> of a container. The paths are virtual, so nothing a
    /// shortcut could point at exists — but the copy that extracts them is real work.
    /// </summary>
    [Fact]
    public void VirtualSourcesCannotBeShortcutTo()
    {
        var entries = RightDropMenuRules.Build(Ordinary with { SourcesAreVirtual = true });

        Assert.False(Enabled(entries, RightDropVerb.Shortcut));
        Assert.True(Enabled(entries, RightDropVerb.Copy));
    }

    /// <summary>Dropping items into the folder they already live in: there is nothing to move, but
    /// copying still produces "name (2)" and a shortcut is still a real thing to make.</summary>
    [Fact]
    public void AMoveWithNothingToDoIsGreyed_ButTheOthersStand()
    {
        var entries = RightDropMenuRules.Build(Ordinary with { MoveHasWork = false });

        Assert.False(Enabled(entries, RightDropVerb.Move));
        Assert.True(Enabled(entries, RightDropVerb.Copy));
        Assert.True(Enabled(entries, RightDropVerb.Shortcut));
    }

    [Fact]
    public void ACopyWithNothingToDoIsGreyed()
    {
        var entries = RightDropMenuRules.Build(Ordinary with { CopyHasWork = false });

        Assert.False(Enabled(entries, RightDropVerb.Copy));
        Assert.True(Enabled(entries, RightDropVerb.Move));
    }

    // --- Wording ---

    [Fact]
    public void OneItemGetsASingularShortcutLabel()
    {
        var entries = RightDropMenuRules.Build(Ordinary with { ItemCount = 1 });

        Assert.Equal("Move here", Label(entries, RightDropVerb.Move));
        Assert.Equal("Copy here", Label(entries, RightDropVerb.Copy));
        Assert.Equal("Create shortcut here", Label(entries, RightDropVerb.Shortcut));
    }

    [Fact]
    public void SeveralItemsGetAPluralShortcutLabel()
    {
        var entries = RightDropMenuRules.Build(Ordinary with { ItemCount = 3 });

        Assert.Equal("Create shortcuts here", Label(entries, RightDropVerb.Shortcut));
    }

    private static bool Enabled(IReadOnlyList<RightDropEntry> entries, RightDropVerb verb) =>
        entries.Single(e => e.Verb == verb).Enabled;

    private static string Label(IReadOnlyList<RightDropEntry> entries, RightDropVerb verb) =>
        entries.Single(e => e.Verb == verb).Label;
}
