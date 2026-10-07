using BertBrowser.Core.Services.Metadata;
using BertBrowser.Core.Services.Rename;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>The rename tokens that draw on a file's own metadata: <c>{artist}</c>, <c>{track:00}</c>,
/// <c>{taken:yyyy-MM-dd}</c>.</summary>
public sealed class RenameTagTests
{
    private static RenameSource Song(string name, params (MetadataField Field, string Value)[] tags) =>
        new(@"C:\music\" + name, IsDirectory: false) { Tags = tags.ToDictionary(t => t.Field, t => t.Value) };

    private static RenamedName One(RenameSource source, string template) =>
        RenamePattern.Apply([source], new RenameRule(template))[0];

    [Fact]
    public void A_name_is_built_from_what_the_file_says_about_itself()
    {
        var song = Song("track03.mp3",
            (MetadataField.Artist, "Björk"), (MetadataField.Album, "Homogenic"),
            (MetadataField.Title, "Jóga"), (MetadataField.TrackNumber, "2"), (MetadataField.Year, "1997"));

        var named = One(song, "{track:00} {artist} - {title} ({year}){ext}");

        Assert.Equal("02 Björk - Jóga (1997).mp3", named.Name);
        Assert.Null(named.Problem);
        Assert.Equal("2 Homogenic.mp3", One(song, "{track} {album}{ext}").Name);
    }

    [Fact]
    public void A_date_taken_takes_a_format_and_has_a_sortable_default()
    {
        var photo = new RenameSource(@"C:\p\IMG_0042.jpg", false)
        {
            Tags = new Dictionary<MetadataField, string> { [MetadataField.DateTaken] = "2021-06-05 04:03:02" },
        };

        Assert.Equal("2021-06-05 IMG_0042.jpg", One(photo, "{taken} {name}").Name);
        Assert.Equal("2021-06 0403.jpg", One(photo, "{taken:yyyy-MM HHmm}{ext}").Name);
    }

    [Fact]
    public void A_missing_tag_refuses_that_item_rather_than_leaving_a_hole_in_its_name()
    {
        var untitled = Song("a.mp3", (MetadataField.Artist, "Somebody"));
        var named = One(untitled, "{artist} - {title}{ext}");

        Assert.Contains("has no title", named.Problem);

        // Tags nobody read are the same refusal, never a crash and never an empty name.
        var unread = new RenameSource(@"C:\music\b.mp3", false);
        Assert.Contains("has no artist", One(unread, "{artist}{ext}").Problem);

        // And the planner turns it into a rejection, which blocks the whole batch.
        var plan = new RenamePlanner(new NothingExists()).Plan([untitled], new RenameRule("{artist} - {title}{ext}"));
        Assert.False(plan.HasWork);
        Assert.Contains("has no title", Assert.Single(plan.Rejected).Message);
    }

    [Fact]
    public void What_a_tag_holds_is_made_safe_for_a_name()
    {
        var song = Song("a.mp3", (MetadataField.Artist, "AC/DC"), (MetadataField.Title, "Who Made Who?: Live"));

        var named = One(song, "{artist} - {title}{ext}");

        Assert.Equal("AC_DC - Who Made Who__ Live.mp3", named.Name);
        Assert.Null(RenamePattern.Validate(named.Name));
    }

    [Theory]
    [InlineData("{track:abc}", "not a width")]
    [InlineData("{artist:x}", "does not take")]
    [InlineData("{taken:d}", "can't hold")]
    [InlineData("{nosuchtag}", "not a name template token")]
    public void A_tag_token_that_cannot_be_used_says_why_while_it_is_being_typed(string template, string expected)
    {
        Assert.Contains(expected, RenamePattern.ValidateRule(new RenameRule(template + "{ext}")));
    }

    [Fact]
    public void Only_a_template_that_names_a_tag_asks_for_the_tags_to_be_read()
    {
        Assert.True(RenameTemplate.UsesTags("{artist} - {title}{ext}"));
        Assert.True(RenameTemplate.UsesTags("{TAKEN:yyyy}{ext}"));
        Assert.False(RenameTemplate.UsesTags("{name}"));
        Assert.False(RenameTemplate.UsesTags("{n:000} {modified}{ext}"));
        Assert.False(RenameTemplate.UsesTags("{artist"));
    }

    private sealed class NothingExists : IRenameProbe
    {
        public bool DirectoryExists(string path) => false;

        public bool FileExists(string path) => path.StartsWith(@"C:\music\", StringComparison.OrdinalIgnoreCase)
                                               && !path.Contains(" - ");
    }
}
