using BertBrowser.Core.Services.Preview;
using Xunit;

namespace BertBrowser.Core.Tests;

public class PreviewMetadataTests
{
    private static ShellPropertyRow P(string canonical, string name, string value) => new(canonical, name, value);

    [Fact]
    public void ImagePropertiesComeBackInAUsefulOrder()
    {
        // Deliberately scrambled going in: the order out is the rule under test.
        List<ShellPropertyRow> properties =
        [
            P("System.Photo.CameraModel", "Camera model", "X100V"),
            P("System.Image.Dimensions", "Dimensions", "6240 x 4160"),
            P("System.Photo.FNumber", "F-stop", "f/2"),
        ];

        var rows = PreviewMetadata.Select(PreviewKind.Image, properties);
        string[] labels = ["Dimensions", "Camera model", "F-stop"];
        Assert.Equal(labels, rows.Select(r => r.Label).ToArray());
        Assert.Equal("6240 x 4160", rows[0].Value);
    }

    [Fact]
    public void AKeyWeDoNotCareAboutIsDropped()
    {
        List<ShellPropertyRow> properties =
        [
            P("System.Image.Dimensions", "Dimensions", "800 x 600"),
            P("System.FileAttributes", "Attributes", "A"),
            P("System.ItemFolderPathDisplay", "Folder path", @"C:\Pictures"),
        ];

        var row = Assert.Single(PreviewMetadata.Select(PreviewKind.Image, properties));
        Assert.Equal("Dimensions", row.Label);
    }

    [Fact]
    public void SelectionIsByCanonicalName_NotByTheLocalisedLabel()
    {
        // The whole reason this class exists: on a German Windows the label is "Abmessungen"
        // and the canonical key is unchanged. Matching on the label would return nothing.
        List<ShellPropertyRow> properties = [P("System.Image.Dimensions", "Abmessungen", "800 x 600")];

        var row = Assert.Single(PreviewMetadata.Select(PreviewKind.Image, properties));
        Assert.Equal("Abmessungen", row.Label);
        Assert.Equal("800 x 600", row.Value);
    }

    [Fact]
    public void AudioTagsLeadAMediaFile()
    {
        List<ShellPropertyRow> properties =
        [
            P("System.Audio.SampleRate", "Audio sample rate", "44100"),
            P("System.Music.Artist", "Contributing artists", "Someone"),
            P("System.Media.Duration", "Length", "00:03:41"),
        ];

        var rows = PreviewMetadata.Select(PreviewKind.Media, properties);
        string[] labels = ["Contributing artists", "Length", "Audio sample rate"];
        Assert.Equal(labels, rows.Select(r => r.Label).ToArray());
    }

    [Fact]
    public void AnEmptyValueIsNotARow() =>
        Assert.Empty(PreviewMetadata.Select(PreviewKind.Image, [P("System.Image.Dimensions", "Dimensions", "")]));

    [Fact]
    public void APropertyWithNoCanonicalNameIsDropped() =>
        Assert.Empty(PreviewMetadata.Select(PreviewKind.Image, [P("", "Dimensions", "800 x 600")]));

    [Fact]
    public void APropertyWithNoLabelFallsBackToTheTailOfItsKey()
    {
        var row = Assert.Single(PreviewMetadata.Select(PreviewKind.Image, [P("System.Image.Dimensions", "", "800 x 600")]));
        Assert.Equal("Dimensions", row.Label);
    }

    [Theory]
    [InlineData(PreviewKind.Text)]
    [InlineData(PreviewKind.Archive)]
    [InlineData(PreviewKind.Font)]
    [InlineData(PreviewKind.None)]
    public void KindsWithBetterNumbersOfTheirOwnAskTheShellForNothing(PreviewKind kind) =>
        Assert.Empty(PreviewMetadata.Select(kind, [P("System.Image.Dimensions", "Dimensions", "800 x 600")]));

    [Fact]
    public void TheStripIsCapped()
    {
        var properties = new List<ShellPropertyRow>();
        foreach (var canonical in new[]
        {
            "System.Image.Dimensions", "System.Image.BitDepth", "System.Image.ColorSpace",
            "System.Photo.DateTaken", "System.Photo.CameraManufacturer", "System.Photo.CameraModel",
            "System.Photo.LensModel", "System.Photo.FNumber", "System.Photo.ExposureTime",
            "System.Photo.ISOSpeed", "System.Photo.FocalLength", "System.GPS.Latitude",
            "System.GPS.Longitude",
        })
            properties.Add(P(canonical, canonical, "value"));

        Assert.Equal(PreviewMetadata.MaxRows, PreviewMetadata.Select(PreviewKind.Image, properties).Count);
    }

    [Fact]
    public void NothingInMeansNothingOut() =>
        Assert.Empty(PreviewMetadata.Select(PreviewKind.Image, []));
}
