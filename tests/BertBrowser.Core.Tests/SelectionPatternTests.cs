using BertBrowser.Core.Services.Commands;
using Xunit;

namespace BertBrowser.Core.Tests;

public class SelectionPatternTests
{
    private static readonly DateTime Day = new(2026, 3, 14, 9, 0, 0, DateTimeKind.Utc);

    private static readonly SelectionRow[] Rows =
    [
        new("Photos", @"C:\x\Photos", true, 0, Day, Day, FileAttributes.Directory),
        new("IMG_0001.jpg", @"C:\x\IMG_0001.jpg", false, 4_000_000, Day, Day, FileAttributes.Archive),
        new("IMG_0002.JPG", @"C:\x\IMG_0002.JPG", false, 500, Day, Day, FileAttributes.Archive),
        new("notes.txt", @"C:\x\notes.txt", false, 120, Day, Day, FileAttributes.Archive | FileAttributes.ReadOnly),
        new(".secret", @"C:\x\.secret", false, 10, Day, Day, FileAttributes.Hidden),
    ];

    private static string Picked(string pattern)
    {
        var match = SelectionPattern.Match(pattern, Rows);
        Assert.Null(match.Problem);
        return string.Join(" ", match.Indexes.Select(i => Rows[i].Name));
    }

    [Fact]
    public void A_wildcard_picks_by_name_whatever_the_case()
    {
        Assert.Equal("IMG_0001.jpg IMG_0002.JPG", Picked("*.jpg"));
        Assert.Equal("IMG_0001.jpg IMG_0002.JPG", Picked("img_"));
    }

    [Fact]
    public void The_search_boxs_filters_mean_the_same_thing_here()
    {
        Assert.Equal("IMG_0001.jpg", Picked("ext:jpg size:>1mb"));
        Assert.Equal("notes.txt", Picked("is:readonly"));
        Assert.Equal("IMG_0002.JPG", Picked(@"re:^IMG_\d+2"));
        Assert.Equal("Photos notes.txt", Picked("photos OR notes"));
        Assert.Equal("Photos notes.txt .secret", Picked("!img"));
    }

    [Fact]
    public void A_hidden_row_on_screen_is_as_selectable_as_its_neighbours()
    {
        Assert.Equal(".secret", Picked("secret"));
        Assert.Equal(".secret", Picked("is:hidden"));
    }

    [Fact]
    public void Nothing_typed_picks_nothing_and_is_not_a_problem()
    {
        var match = SelectionPattern.Match("   ", Rows);

        Assert.Empty(match.Indexes);
        Assert.Null(match.Problem);
    }

    [Fact]
    public void A_pattern_that_cannot_be_read_says_why()
    {
        var match = SelectionPattern.Match("re:(", Rows);

        Assert.Empty(match.Indexes);
        Assert.NotNull(match.Problem);
    }

    [Theory]
    [InlineData("content:todo")]
    [InlineData("jpg in:archives")]
    public void Terms_that_would_read_files_are_refused_rather_than_answered_wrongly(string pattern)
    {
        var match = SelectionPattern.Match(pattern, Rows);

        Assert.Empty(match.Indexes);
        Assert.NotNull(match.Problem);
    }
}
