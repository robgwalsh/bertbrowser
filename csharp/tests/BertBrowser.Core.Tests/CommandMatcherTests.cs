using BertBrowser.Core.Services.Commands;
using Xunit;

namespace BertBrowser.Core.Tests;

public class CommandMatcherTests
{
    private static CommandMatch Match(
        string query, string name, string gesture = "", string category = "Tabs", params string[] aliases) =>
        CommandMatcher.Match(query, name, aliases, category, gesture);

    private static string Marked(string query, string name)
    {
        var match = Match(query, name);
        var text = name;
        foreach (var range in match.Highlights.OrderByDescending(r => r.Start))
            text = text[..range.Start] + "[" + text.Substring(range.Start, range.Length) + "]" + text[(range.Start + range.Length)..];
        return text;
    }

    [Theory]
    [InlineData("new", "New tab", MatchTier.Start)]
    [InlineData("NEW T", "New tab", MatchTier.Word)]
    [InlineData("tab", "New tab", MatchTier.Word)]
    [InlineData("nt", "New tab", MatchTier.Word)]
    [InlineData("ew", "New tab", MatchTier.Word)]
    [InlineData("nwtb", "New tab", MatchTier.Loose)]
    [InlineData("tabs", "New tab", MatchTier.Loose)]
    [InlineData("xyz", "New tab", MatchTier.None)]
    public void Where_the_words_were_found_decides_the_tier(string query, string name, MatchTier tier)
    {
        Assert.Equal(tier, Match(query, name).Tier);
    }

    [Fact]
    public void Every_word_typed_has_to_be_found()
    {
        Assert.True(Match("close tab", "Close tab").IsMatch);
        Assert.True(Match("tab close", "Close tab").IsMatch);
        Assert.False(Match("close pane", "Close tab").IsMatch);
    }

    [Fact]
    public void A_match_is_only_as_strong_as_its_weakest_word()
    {
        Assert.Equal(MatchTier.Start, Match("close", "Close other tabs").Tier);
        Assert.Equal(MatchTier.Word, Match("close tabs", "Close other tabs").Tier);
    }

    [Fact]
    public void A_competitors_word_for_it_finds_it_but_ranks_under_its_own_name()
    {
        var byAlias = Match("branch", "Toggle flat view", aliases: ["branch view", "flatten"]);
        var byName = Match("flat", "Toggle flat view", aliases: ["branch view", "flatten"]);

        Assert.Equal(MatchTier.Loose, byAlias.Tier);
        Assert.Empty(byAlias.Highlights);
        Assert.True(byName.Tier > byAlias.Tier);
    }

    [Fact]
    public void A_shortcut_typed_whole_is_a_lookup()
    {
        Assert.Equal(MatchTier.Start, Match("ctrl+b", "Toggle flat view", "Ctrl+B").Tier);
        Assert.Equal(MatchTier.Start, Match("Ctrl + B", "Toggle flat view", "Ctrl+B").Tier);
        Assert.Equal(MatchTier.Start, Match("f7", "Compare with other pane", "F7").Tier);
        Assert.False(Match("ctrl+b", "New tab", "Ctrl+T").IsMatch);
    }

    [Fact]
    public void Part_of_a_shortcut_lists_what_is_on_it()
    {
        Assert.True(Match("ctrl+shift", "Search this PC", "Ctrl+Shift+F").IsMatch);
        Assert.False(Match("ctrl+shift", "New tab", "Ctrl+T").IsMatch);
    }

    [Fact]
    public void Two_letters_in_order_are_not_a_match_by_themselves()
    {
        // "nb" is in order in New tab, and in order in nearly everything else too.
        Assert.False(Match("nb", "New tab").IsMatch);
    }

    [Fact]
    public void A_shorter_name_is_the_closer_match_for_the_same_word()
    {
        Assert.True(Match("copy", "Copy").Score > Match("copy", "Copy as path").Score);
    }

    [Fact]
    public void Letters_sitting_together_beat_the_same_letters_scattered()
    {
        Assert.True(Match("dup", "Find duplicates").Score > Match("dpl", "Find duplicates").Score);
    }

    [Theory]
    [InlineData("new", "New tab", "[New] tab")]
    [InlineData("tab", "New tab", "New [tab]")]
    [InlineData("nt", "New tab", "[N]ew [t]ab")]
    [InlineData("new tab", "New tab", "[New] [tab]")]
    [InlineData("pth", "Copy as path", "Copy as [p]a[th]")]
    public void The_name_is_marked_where_it_matched(string query, string name, string expected)
    {
        Assert.Equal(expected, Marked(query, name));
    }

    [Fact]
    public void Nothing_typed_matches_nothing()
    {
        Assert.False(Match("", "New tab").IsMatch);
        Assert.False(Match("   ", "New tab").IsMatch);
    }
}
