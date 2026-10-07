using System.Globalization;
using BertBrowser.Core.Services.Metadata;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The Windows-property fallback as far as Core goes: which files it is offered for, the copy it
/// is pointed at, and how values cross. The handler itself is Windows' and is exercised by
/// <c>tools/ui/metadata.bbs</c>; here it is a fake that keeps a title on a file's first line.
/// </summary>
public sealed class PropertyFallbackTests : IDisposable
{
    private readonly string _root;

    public PropertyFallbackTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-fallback-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        try
        {
            foreach (var file in Directory.EnumerateFiles(_root)) File.SetAttributes(file, FileAttributes.Normal);
            Directory.Delete(_root, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    /// <summary>A "format" of one title line and then a body nothing should touch.</summary>
    private sealed class FakeHandler : IPropertyFallback
    {
        public List<string> Written { get; } = [];

        public Func<string, string>? Corrupt { get; init; }

        public bool Refuses { get; init; }

        public PropertyFallbackReading? Read(string path)
        {
            var lines = File.ReadAllText(path).Split('\n', 2);
            if (lines.Length < 2) return null;

            var values = new Dictionary<MetadataField, string>();
            if (lines[0].Length > 0) values[MetadataField.Title] = lines[0];
            return new PropertyFallbackReading(
                new MetadataDocument(MetadataFamily.Document, values),
                new HashSet<MetadataField> { MetadataField.Title });
        }

        public void Write(string path, IReadOnlyDictionary<MetadataField, string> changes)
        {
            Written.Add(path);
            if (Refuses) throw new MetadataFormatException("Windows could not write to it.");

            var body = File.ReadAllText(path).Split('\n', 2)[1];
            var title = changes[MetadataField.Title];
            File.WriteAllText(path, (Corrupt?.Invoke(title) ?? title) + "\n" + body);
        }
    }

    private string Film(string name = "film.mov", string title = "Old")
    {
        var path = Path.Combine(_root, name);
        File.WriteAllText(path, title + "\nthe frames");
        return path;
    }

    private static MetadataEditPlan Plan(string path, string title, MetadataEdit? edit = null) =>
        new MetadataEditPlanner().Plan(
            [path],
            edit ?? new MetadataEdit(new Dictionary<MetadataField, string>
            {
                [MetadataField.Title] = title,
                [MetadataField.Album] = "Not offered",
            }),
            new Dictionary<string, IReadOnlySet<MetadataField>>(StringComparer.OrdinalIgnoreCase)
            {
                [path] = new HashSet<MetadataField> { MetadataField.Title },
            });

    private string[] Leftovers() =>
        [.. Directory.EnumerateFiles(_root).Select(Path.GetFileName).Where(f => f!.Contains(".bertbrowser-"))!];

    [Fact]
    public void A_file_with_no_codec_is_planned_only_when_Windows_offered_fields_for_it()
    {
        var film = Film();

        var without = new MetadataEditPlanner().Plan([film], new MetadataEdit(
            new Dictionary<MetadataField, string> { [MetadataField.Title] = "New" }));
        Assert.Equal(MetadataRejection.Unsupported, Assert.Single(without.Rejected).Reason);

        var planned = Assert.Single(Plan(film, "New").Files);
        Assert.True(planned.ViaWindows);
        Assert.Equal([MetadataField.Title], planned.Edit.Changes.Keys);
    }

    [Fact]
    public void A_codec_always_wins_over_the_fallback()
    {
        var photo = Path.Combine(_root, "photo.jpg");
        File.WriteAllBytes(photo, MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg));

        Assert.False(Assert.Single(Plan(photo, "New").Files).ViaWindows);
    }

    [Fact]
    public void The_fallback_takes_fields_and_never_a_removal()
    {
        var film = Film();

        var plan = Plan(film, "", MetadataEdit.None with { RemoveAll = true });

        Assert.False(plan.HasWork);
        Assert.Equal(MetadataRejection.NothingApplies, Assert.Single(plan.Rejected).Reason);
    }

    [Fact]
    public void Every_other_refusal_still_stands()
    {
        var film = Film();
        File.SetAttributes(film, FileAttributes.ReadOnly);

        Assert.Equal(MetadataRejection.ReadOnly, Assert.Single(Plan(film, "New").Rejected).Reason);
        Assert.Equal(MetadataRejection.InsideArchive,
            MetadataEditPlanner.Refusal(@"C:\p\pack.zip\film.mov", null, viaWindows: true)!.Reason);
    }

    [Fact]
    public void The_handler_writes_a_copy_and_the_original_is_held_for_undo()
    {
        var film = Film();
        var before = File.ReadAllBytes(film);
        var handler = new FakeHandler();
        var executor = new MetadataEditExecutor(new FileSystemMetadataProbe(), handler);

        var outcome = executor.Execute(Plan(film, "New"));

        Assert.Empty(outcome.Failed);
        var held = Assert.Single(outcome.Edited);
        Assert.Equal("New\nthe frames", File.ReadAllText(film));
        Assert.Equal(before, File.ReadAllBytes(held.HeldPath));

        // Never the user's own file: the handler was pointed at the sibling, with its extension.
        var written = Assert.Single(handler.Written);
        Assert.NotEqual(film, written);
        Assert.Contains(MetadataEditExecutor.RewriteMarker, written);
        Assert.EndsWith(".mov", written);

        var undone = executor.Undo(outcome.Edited);
        Assert.Empty(undone.Failed);
        Assert.Equal(before, File.ReadAllBytes(film));

        MetadataEditExecutor.CommitStaging(undone.Swapped);
        Assert.Empty(Leftovers());
    }

    [Fact]
    public void A_handler_that_refuses_costs_a_temporary_file_and_nothing_else()
    {
        var film = Film();
        var before = File.ReadAllBytes(film);
        var executor = new MetadataEditExecutor(new FileSystemMetadataProbe(), new FakeHandler { Refuses = true });

        var outcome = executor.Execute(Plan(film, "New"));

        Assert.Empty(outcome.Edited);
        Assert.Contains("Windows", Assert.Single(outcome.Failed).Message);
        Assert.Equal(before, File.ReadAllBytes(film));
        Assert.Empty(Leftovers());
    }

    [Fact]
    public void A_value_that_does_not_read_back_is_not_swapped_in()
    {
        var film = Film();
        var before = File.ReadAllBytes(film);
        var executor = new MetadataEditExecutor(
            new FileSystemMetadataProbe(), new FakeHandler { Corrupt = title => title[..1] });

        var outcome = executor.Execute(Plan(film, "New"));

        Assert.Contains("did not read back", Assert.Single(outcome.Failed).Message);
        Assert.Equal(before, File.ReadAllBytes(film));
        Assert.Empty(Leftovers());
    }

    [Fact]
    public void Writing_what_is_already_there_is_unchanged_and_not_a_history_entry()
    {
        var film = Film(title: "Same");
        var executor = new MetadataEditExecutor(new FileSystemMetadataProbe(), new FakeHandler());

        var outcome = executor.Execute(Plan(film, "Same"));

        Assert.False(outcome.CanUndo);
        Assert.Equal([film], outcome.Unchanged);
        Assert.Empty(Leftovers());
    }

    [Fact]
    public void Without_a_fallback_the_file_fails_by_name_rather_than_throwing()
    {
        var film = Film();

        var outcome = new MetadataEditExecutor().Execute(Plan(film, "New"));

        Assert.Contains("film.mov", Assert.Single(outcome.Failed).Message);
        Assert.Empty(Leftovers());
    }

    // ---- values ---------------------------------------------------------------------------------

    [Theory]
    [InlineData("1", 1u)]
    [InlineData("2", 25u)]
    [InlineData("3", 50u)]
    [InlineData("4", 75u)]
    [InlineData("5", 99u)]
    public void Stars_cross_as_the_numbers_Explorer_writes_and_come_back_the_same(string stars, uint rating)
    {
        Assert.Equal(rating, WindowsPropertyFields.ToWindows(MetadataField.Rating, stars).Number);
        Assert.Equal(stars, WindowsPropertyFields.FromWindows(MetadataField.Rating, null, rating, null));
    }

    [Fact]
    public void A_rating_another_program_wrote_lands_in_the_band_Explorer_shows_it_in()
    {
        Assert.Equal("", WindowsPropertyFields.FromWindows(MetadataField.Rating, null, 0, null));
        Assert.Equal("1", WindowsPropertyFields.FromWindows(MetadataField.Rating, null, 12, null));
        Assert.Equal("2", WindowsPropertyFields.FromWindows(MetadataField.Rating, null, 13, null));
        Assert.Equal("4", WindowsPropertyFields.FromWindows(MetadataField.Rating, null, 87, null));
        Assert.Equal("5", WindowsPropertyFields.FromWindows(MetadataField.Rating, null, 88, null));
    }

    [Fact]
    public void A_date_crosses_as_universal_time_and_comes_back_as_it_was_typed()
    {
        const string typed = "2024-03-14 09:26:53";
        var local = DateTime.ParseExact(typed, MetadataFields.DateFormat, CultureInfo.InvariantCulture);

        var sent = WindowsPropertyFields.ToWindows(MetadataField.DateTaken, typed);

        Assert.Equal(DateTime.SpecifyKind(local, DateTimeKind.Local).ToUniversalTime(), sent.Utc);
        Assert.Equal(DateTimeKind.Utc, sent.Utc!.Value.Kind);
        Assert.Equal(typed, WindowsPropertyFields.FromWindows(MetadataField.DateTaken, "whatever Windows renders", null, sent.Utc));
    }

    [Fact]
    public void Text_lists_and_numbers_cross_as_themselves_and_an_empty_value_clears()
    {
        Assert.Equal("A title", WindowsPropertyFields.ToWindows(MetadataField.Title, "A title").Text);
        Assert.Equal("sea; boats", WindowsPropertyFields.ToWindows(MetadataField.Keywords, "sea; boats").Text);
        Assert.Equal(1999u, WindowsPropertyFields.ToWindows(MetadataField.Year, "1999").Number);
        Assert.True(WindowsPropertyFields.ToWindows(MetadataField.Title, "").IsClear);
        Assert.True(WindowsPropertyFields.ToWindows(MetadataField.Rating, "0").IsClear);

        Assert.Equal("sea; boats", WindowsPropertyFields.FromWindows(MetadataField.Keywords, "sea;boats", null, null));
        Assert.Equal("1999", WindowsPropertyFields.FromWindows(MetadataField.Year, "1,999", 1999, null));
        Assert.Equal("", WindowsPropertyFields.FromWindows(MetadataField.Year, "not a year", null, null));
        Assert.Throws<MetadataFormatException>(() => WindowsPropertyFields.ToWindows(MetadataField.Year, "soon"));
    }

    [Fact]
    public void Every_mapped_field_is_one_whose_value_can_cross()
    {
        Assert.All(WindowsPropertyFields.Canonical, pair =>
        {
            Assert.Contains(MetadataFields.Get(pair.Key).Kind, new[]
            {
                MetadataFieldKind.Text, MetadataFieldKind.List, MetadataFieldKind.Number,
                MetadataFieldKind.Date, MetadataFieldKind.Rating,
            });
            Assert.StartsWith("System.", pair.Value);
        });

        Assert.Equal(WindowsPropertyFields.Canonical.Count, WindowsPropertyFields.Canonical.Values.Distinct().Count());

        Assert.Equal(MetadataField.Title, WindowsPropertyFields.FieldOf("system.title"));
        Assert.Equal(MetadataField.Iso, WindowsPropertyFields.FieldOf("System.Photo.ISOSpeed"));
        Assert.Null(WindowsPropertyFields.FieldOf("System.Image.Dimensions"));
    }
}
