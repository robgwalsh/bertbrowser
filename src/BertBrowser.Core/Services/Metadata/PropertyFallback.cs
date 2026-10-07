using System.Globalization;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// Windows' own property handlers, for the files no codec here writes — what Explorer's Details
/// tab edits through.
/// </summary>
/// <remarks>
/// <para>
/// <b>This is the one write whose payload nobody checks.</b> A codec's rewrite is only swapped in
/// once everything that is not metadata hashes the same as the original; a property handler is
/// somebody else's code writing a format this app cannot read, so there is nothing to hash. What
/// is kept: the handler is only ever pointed at a <em>copy</em> beside the file, the fields are
/// read back from that copy, and it is swapped in the same way — so a handler that fails or
/// refuses costs a temporary file, and one that succeeds is one undo from never having happened.
/// What is lost is the proof that the rest of the file is untouched, and the pane says so on
/// every file edited this way.
/// </para>
/// <para>
/// Never offered where a codec exists: the extension decides, and a codec always wins.
/// </para>
/// </remarks>
public interface IPropertyFallback
{
    /// <summary>What the file holds and which fields Windows will take, or null when its handler
    /// writes none of them (or it has no handler at all).</summary>
    PropertyFallbackReading? Read(string path);

    /// <summary>Writes into the file at <paramref name="path"/>, in place.</summary>
    /// <exception cref="MetadataFormatException">The handler refused, in words fit to show.</exception>
    void Write(string path, IReadOnlyDictionary<MetadataField, string> changes);
}

/// <param name="Fields">The fields the handler said it would write — per file, not per type: the
/// answer is the handler's, and the same extension can get a different one on another PC.</param>
public sealed record PropertyFallbackReading(MetadataDocument Document, IReadOnlySet<MetadataField> Fields);

/// <summary>A property value on its way to Windows: exactly one part set, or none to clear it.</summary>
public readonly record struct WindowsPropertyValue(string? Text = null, uint? Number = null, DateTime? Utc = null)
{
    public bool IsClear => Text is null && Number is null && Utc is null;
}

/// <summary>
/// Which Windows property each field is, and how a value crosses in each direction. Pure, so the
/// part of the fallback that can be wrong without Windows being involved is tested without it.
/// </summary>
public static class WindowsPropertyFields
{
    /// <summary>Canonical property names, which are the same in every language of Windows. Only
    /// fields whose value is text, a whole number, a date or stars: nothing here guesses at how a
    /// handler wants a fraction.</summary>
    public static IReadOnlyDictionary<MetadataField, string> Canonical { get; } = new Dictionary<MetadataField, string>
    {
        [MetadataField.Title] = "System.Title",
        [MetadataField.Artist] = "System.Music.Artist",
        [MetadataField.AlbumArtist] = "System.Music.AlbumArtist",
        [MetadataField.Album] = "System.Music.AlbumTitle",
        [MetadataField.TrackNumber] = "System.Music.TrackNumber",
        [MetadataField.Year] = "System.Media.Year",
        [MetadataField.Genre] = "System.Music.Genre",
        [MetadataField.Composer] = "System.Music.Composer",
        [MetadataField.Conductor] = "System.Music.Conductor",
        [MetadataField.Publisher] = "System.Media.Publisher",
        [MetadataField.Author] = "System.Author",
        [MetadataField.DateTaken] = "System.Photo.DateTaken",
        [MetadataField.Subject] = "System.Subject",
        [MetadataField.Category] = "System.Category",
        [MetadataField.Keywords] = "System.Keywords",
        [MetadataField.Rating] = "System.Rating",
        [MetadataField.Comment] = "System.Comment",
        [MetadataField.Copyright] = "System.Copyright",
        [MetadataField.CameraMake] = "System.Photo.CameraManufacturer",
        [MetadataField.CameraModel] = "System.Photo.CameraModel",
    };

    private static readonly Dictionary<string, MetadataField> ByCanonical = Reverse();

    private static Dictionary<string, MetadataField> Reverse()
    {
        var map = Canonical.ToDictionary(p => p.Value, p => p.Key, StringComparer.OrdinalIgnoreCase);

        // Read-only here — nothing writes a fraction through a handler — but still the same
        // fact as a field a codec offers.
        map["System.Photo.LensModel"] = MetadataField.LensModel;
        map["System.Photo.FNumber"] = MetadataField.Aperture;
        map["System.Photo.ExposureTime"] = MetadataField.ExposureTime;
        map["System.Photo.ISOSpeed"] = MetadataField.Iso;
        map["System.Photo.FocalLength"] = MetadataField.FocalLength;
        return map;
    }

    /// <summary>The field a Windows property is the same fact as, if any — so a read-only list
    /// of properties can leave out what already has a box.</summary>
    public static MetadataField? FieldOf(string canonical) =>
        ByCanonical.TryGetValue(canonical, out var field) ? field : null;

    /// <summary>
    /// What Windows returned, as this app's canonical value; empty for nothing, or for something
    /// that is not a value of the field's kind.
    /// </summary>
    /// <param name="text">The value as Windows renders it, a list joined with semicolons.</param>
    public static string FromWindows(MetadataField field, string? text, double? number, DateTime? utc)
    {
        var raw = MetadataFields.Get(field).Kind switch
        {
            MetadataFieldKind.Rating => number is { } rating ? Stars(rating) : "",
            MetadataFieldKind.Date => utc is { } date
                ? date.ToLocalTime().ToString(MetadataFields.DateFormat, CultureInfo.InvariantCulture)
                : "",
            MetadataFieldKind.Number when number is { } whole && whole == Math.Floor(whole) && whole is > 0 and < uint.MaxValue =>
                ((uint)whole).ToString(CultureInfo.InvariantCulture),
            _ => text ?? "",
        };

        return MetadataFields.TryNormalize(field, raw, out var value) ? value : "";
    }

    /// <summary>A canonical value as Windows wants it; an empty one clears the property.</summary>
    /// <exception cref="MetadataFormatException">Not a value of the field's kind.</exception>
    public static WindowsPropertyValue ToWindows(MetadataField field, string value)
    {
        if (value.Length == 0) return default;

        var spec = MetadataFields.Get(field);
        switch (spec.Kind)
        {
            case MetadataFieldKind.Rating when uint.TryParse(value, out var stars) && stars <= 5:
                return stars == 0 ? default : new(Number: Rating(stars));

            case MetadataFieldKind.Number when uint.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out var number):
                return new(Number: number);

            case MetadataFieldKind.Date when MetadataFields.TryParseDate(value, out var date):
                return new(Utc: DateTime.SpecifyKind(date, DateTimeKind.Local).ToUniversalTime());

            case MetadataFieldKind.Text or MetadataFieldKind.List:
                return new(Text: value);

            default:
                throw new MetadataFormatException($"'{value}' is not a value {spec.Label} can take.");
        }
    }

    // Windows keeps stars as 1 to 99, and these are the five numbers Explorer itself writes; the
    // bands are the ones it reads them back with.
    private static uint Rating(uint stars) => stars switch { 1 => 1, 2 => 25, 3 => 50, 4 => 75, _ => 99 };

    private static string Stars(double rating) => rating switch
    {
        < 1 => "",
        < 13 => "1",
        < 38 => "2",
        < 63 => "3",
        < 88 => "4",
        _ => "5",
    };
}
