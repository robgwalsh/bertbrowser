using BertBrowser.Core.Services.Columns;
using BertBrowser.Core.Services.Preview;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ColumnCatalogTests
{
    // --- The two id spaces ---
    //
    // Everything about how an unknown id is treated rests on being able to tell a built-in id from a
    // canonical property name without a lookup. If these two ever overlap, a column from a newer
    // build stops being droppable and a property this machine lacks stops being keepable.

    [Fact]
    public void NoBuiltInIdLooksLikeACanonicalName() =>
        Assert.All(ColumnCatalog.BuiltIns, spec => Assert.False(ColumnId.LooksCanonical(spec.Id)));

    [Fact]
    public void EveryCuratedIdLooksLikeACanonicalName() =>
        Assert.All(ColumnCatalog.Curated, spec => Assert.True(ColumnId.LooksCanonical(spec.Id)));

    [Theory]
    [InlineData("System.Photo.DateTaken")]
    [InlineData("System.Size")]
    [InlineData("System.Image.Dimensions")]
    public void ACanonicalNameIsRecognised(string id) => Assert.True(ColumnId.LooksCanonical(id));

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("Name")]                  // a built-in, no dot
    [InlineData(".System")]               // leading dot
    [InlineData("System.")]               // trailing dot
    [InlineData("System.Photo Date")]     // whitespace
    [InlineData("System.Photo-Date")]     // punctuation a canonical name never carries
    [InlineData("C:\\Users\\notes.txt")]  // a path, which is the shape that must never pass
    public void AnythingElseIsNot(string? id) => Assert.False(ColumnId.LooksCanonical(id));

    [Fact]
    public void AnIdTooLongToBeARegisteredNameIsRefused() =>
        Assert.False(ColumnId.LooksCanonical("System." + new string('x', ColumnId.MaxLength)));

    // --- What is on offer ---

    [Fact]
    public void TheDefaultsAreTodaysColumnsAtTodaysWidths()
    {
        var defaults = ColumnCatalog.Defaults();

        Assert.Equal(["Name", "Size", "Type", "Modified"], defaults.Select(c => c.Id).ToArray());
        Assert.Equal([320d, 110, 120, 140], defaults.Select(c => c.Width).ToArray());
    }

    [Fact]
    public void TheCuratedSetIsComposedFromThePreviewPanesOwnLists()
    {
        // Not retyped: 38 canonical strings that would otherwise drift with nothing to notice.
        var curated = ColumnCatalog.Curated.Select(s => s.Id).ToHashSet(StringComparer.OrdinalIgnoreCase);

        foreach (var canonical in PreviewMetadata.ImageOrder
                     .Concat(PreviewMetadata.MediaOrder)
                     .Concat(PreviewMetadata.DocumentOrder))
        {
            if (ColumnCatalog.ShadowedByBuiltIn.Contains(canonical)) continue;
            Assert.Contains(canonical, curated);
        }
    }

    [Fact]
    public void TheCuratedSetOffersNothingABuiltInAlreadySays()
    {
        // Two Type columns — "PNG file" beside "PNG image", one of them blank until it hydrates — is
        // worse than one, and the free one always wins.
        Assert.All(ColumnCatalog.Curated,
            spec => Assert.DoesNotContain(spec.Id, ColumnCatalog.ShadowedByBuiltIn));
    }

    [Fact]
    public void ThereAreNoDuplicatesInTheCuratedSet() =>
        // Media and Document both list System.Title.
        Assert.Equal(
            ColumnCatalog.Curated.Count,
            ColumnCatalog.Curated.Select(s => s.Id).Distinct(StringComparer.OrdinalIgnoreCase).Count());

    [Fact]
    public void ACuratedColumnHeaderFallsBackToTheTailOfItsKey() =>
        Assert.Equal("Dimensions", ColumnCatalog.TryGet("System.Image.Dimensions")!.Header);

    // --- Resolving an id ---

    [Fact]
    public void AnUnknownBareWordHasNoSpec() => Assert.Null(ColumnCatalog.TryGet("Colour"));

    [Fact]
    public void AnUncuratedCanonicalNameGetsASynthesizedSpec()
    {
        var spec = ColumnCatalog.TryGet("System.Contact.NickName");

        Assert.NotNull(spec);
        Assert.Equal(ColumnKind.ShellProperty, spec.Kind);
        Assert.Equal("NickName", spec.Header);
    }

    [Fact]
    public void SortingByAnUnusableIdDegradesToNameRatherThanFailing()
    {
        Assert.Equal("Name", ColumnCatalog.SortSpec("Colour").Id);
        Assert.Equal("Name", ColumnCatalog.SortSpec(null).Id);
        Assert.Equal("Name", ColumnCatalog.SortSpec("").Id);
    }

    [Fact]
    public void SortingByMatchDegradesToNameBecauseMatchIsNotSortable() =>
        Assert.Equal("Name", ColumnCatalog.SortSpec("Match").Id);

    [Fact]
    public void TheSortIdsEveryPreviousBuildWroteStillResolve() =>
        // SessionTab.SortBy has always held these exact strings. Renaming one silently retires a
        // saved sort order, so this is the guard on that.
        Assert.All(
            new[] { "Name", "Size", "Type", "Modified", "RelativePath" },
            id => Assert.Equal(id, ColumnCatalog.SortSpec(id).Id));
}
