using System.Globalization;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>What kind of file a codec describes.</summary>
public enum MetadataFamily
{
    Image,
    /// <summary>Sound, and video in the containers that tag the same way.</summary>
    Audio,
    Document,
}

/// <summary>
/// The fields this app can write. One vocabulary for every codec, the editor and the rename tokens.
/// </summary>
public enum MetadataField
{
    Title,
    Artist,
    AlbumArtist,
    Album,
    TrackNumber,
    TrackTotal,
    DiscNumber,
    DiscTotal,
    Year,
    Genre,
    Composer,
    Comment,
    Copyright,
    Author,
    Subject,
    Keywords,
    Rating,
    DateTaken,
    CameraMake,
    CameraModel,
    LensModel,
    Iso,
    Aperture,
    ExposureTime,
    FocalLength,
    Publisher,
    Conductor,
    Bpm,
    Lyrics,
    Category,
}

public enum MetadataFieldKind
{
    Text,
    /// <summary>A whole number above zero.</summary>
    Number,
    /// <summary>A local date and time, with no zone — what a camera writes.</summary>
    Date,
    /// <summary>Several values, shown and typed separated by semicolons.</summary>
    List,
    /// <summary>Stars, one to five.</summary>
    Rating,
    /// <summary>A measurement above zero, to one decimal place: an f-number, a focal length.</summary>
    Decimal,
    /// <summary>A shutter time in seconds, written the way a camera shows it: <c>1/250</c>, or
    /// <c>2.5</c> for a long one.</summary>
    Exposure,
}

/// <summary>One row of the field catalogue.</summary>
/// <param name="Id">Stable lowercase name — what a script, a rename token or a saved setting says.</param>
public sealed record MetadataFieldSpec(MetadataField Field, string Id, string Label, MetadataFieldKind Kind);

/// <summary>
/// The field catalogue, and the one place a typed value becomes the form every codec stores and
/// compares.
/// </summary>
/// <remarks>
/// <para>
/// <b>Values are strings in a canonical form</b>, and the executor's read-back compares them as
/// such — so normalising happens here, once, before a plan exists. Two spellings of one value
/// reaching a codec would make a correct write look like a failed one.
/// </para>
/// <para>
/// Which of these a given file can hold is not decided here: each codec states its own set
/// (<see cref="IMetadataCodec.Fields"/>), because a WAV and an MP3 are both audio and do not have
/// room for the same things.
/// </para>
/// </remarks>
public static class MetadataFields
{
    /// <summary>How a <see cref="MetadataFieldKind.Date"/> is held: sortable, zoneless.</summary>
    public const string DateFormat = "yyyy-MM-dd HH:mm:ss";

    /// <summary>What separates the values of a <see cref="MetadataFieldKind.List"/>.</summary>
    public const string ListSeparator = "; ";

    /// <summary>Every field, in the order the editor lists them.</summary>
    public static IReadOnlyList<MetadataFieldSpec> All { get; } =
    [
        new(MetadataField.Title, "title", "Title", MetadataFieldKind.Text),
        new(MetadataField.Artist, "artist", "Artist", MetadataFieldKind.Text),
        new(MetadataField.AlbumArtist, "albumartist", "Album artist", MetadataFieldKind.Text),
        new(MetadataField.Album, "album", "Album", MetadataFieldKind.Text),
        new(MetadataField.TrackNumber, "track", "Track", MetadataFieldKind.Number),
        new(MetadataField.TrackTotal, "tracktotal", "Tracks in total", MetadataFieldKind.Number),
        new(MetadataField.DiscNumber, "disc", "Disc", MetadataFieldKind.Number),
        new(MetadataField.DiscTotal, "disctotal", "Discs in total", MetadataFieldKind.Number),
        new(MetadataField.Year, "year", "Year", MetadataFieldKind.Number),
        new(MetadataField.Genre, "genre", "Genre", MetadataFieldKind.Text),
        new(MetadataField.Composer, "composer", "Composer", MetadataFieldKind.Text),
        new(MetadataField.Conductor, "conductor", "Conductor", MetadataFieldKind.Text),
        new(MetadataField.Publisher, "publisher", "Publisher", MetadataFieldKind.Text),
        new(MetadataField.Bpm, "bpm", "Beats per minute", MetadataFieldKind.Number),
        new(MetadataField.Author, "author", "Author", MetadataFieldKind.Text),
        new(MetadataField.DateTaken, "taken", "Date taken", MetadataFieldKind.Date),
        new(MetadataField.Subject, "subject", "Subject", MetadataFieldKind.Text),
        new(MetadataField.Category, "category", "Category", MetadataFieldKind.Text),
        new(MetadataField.Keywords, "keywords", "Tags", MetadataFieldKind.List),
        new(MetadataField.Rating, "rating", "Rating", MetadataFieldKind.Rating),
        new(MetadataField.Comment, "comment", "Comment", MetadataFieldKind.Text),
        new(MetadataField.Lyrics, "lyrics", "Lyrics", MetadataFieldKind.Text),
        new(MetadataField.Copyright, "copyright", "Copyright", MetadataFieldKind.Text),
        new(MetadataField.CameraMake, "make", "Camera maker", MetadataFieldKind.Text),
        new(MetadataField.CameraModel, "model", "Camera model", MetadataFieldKind.Text),
        new(MetadataField.LensModel, "lens", "Lens", MetadataFieldKind.Text),
        new(MetadataField.FocalLength, "focal", "Focal length (mm)", MetadataFieldKind.Decimal),
        new(MetadataField.Aperture, "aperture", "Aperture (f/)", MetadataFieldKind.Decimal),
        new(MetadataField.ExposureTime, "exposure", "Exposure (s)", MetadataFieldKind.Exposure),
        new(MetadataField.Iso, "iso", "ISO", MetadataFieldKind.Number),
    ];

    private static readonly Dictionary<MetadataField, MetadataFieldSpec> ByField =
        All.ToDictionary(s => s.Field);

    private static readonly Dictionary<string, MetadataFieldSpec> ById =
        All.ToDictionary(s => s.Id, StringComparer.OrdinalIgnoreCase);

    public static MetadataFieldSpec Get(MetadataField field) => ByField[field];

    public static MetadataFieldSpec? Find(string id) => ById.GetValueOrDefault(id);

    /// <summary>
    /// Turns what somebody typed into the canonical value, or says it is not one.
    /// </summary>
    /// <returns>False when the text cannot be this kind of field. An empty result is a valid
    /// value and means <em>clear the field</em>.</returns>
    public static bool TryNormalize(MetadataField field, string? text, out string value) =>
        TryNormalize(Get(field).Kind, text, out value);

    /// <inheritdoc cref="TryNormalize(MetadataField, string?, out string)"/>
    public static bool TryNormalize(MetadataFieldKind kind, string? text, out string value)
    {
        value = "";
        var trimmed = (text ?? "").Trim();
        if (trimmed.Length == 0) return true;

        // A control character cannot be typed and no tag format has a place for one; refusing it
        // here keeps a pasted NUL from truncating the value inside a codec.
        if (trimmed.Any(c => char.IsControl(c) && c is not ('\r' or '\n' or '\t'))) return false;

        switch (kind)
        {
            case MetadataFieldKind.Number:
                if (!int.TryParse(trimmed, NumberStyles.None, CultureInfo.InvariantCulture, out var number)
                    || number is <= 0 or > 9_999_999)
                    return false;
                value = number.ToString(CultureInfo.InvariantCulture);
                return true;

            case MetadataFieldKind.Rating:
                if (!int.TryParse(trimmed, NumberStyles.None, CultureInfo.InvariantCulture, out var stars)
                    || stars is < 0 or > 5)
                    return false;
                value = stars == 0 ? "" : stars.ToString(CultureInfo.InvariantCulture);
                return true;

            case MetadataFieldKind.Date:
                if (!TryParseDate(trimmed, out var date)) return false;
                value = date.ToString(DateFormat, CultureInfo.InvariantCulture);
                return true;

            case MetadataFieldKind.List:
                value = string.Join(ListSeparator, SplitList(trimmed));
                return true;

            case MetadataFieldKind.Decimal:
                if (!TryParseDecimal(trimmed, out var measure)) return false;
                value = FormatDecimal(measure);
                return true;

            case MetadataFieldKind.Exposure:
                if (!TryParseExposure(trimmed, out var numerator, out var denominator)) return false;
                value = FormatExposure(numerator, denominator);
                return true;

            default:
                // One line ending, whichever the text arrived with: tag formats disagree about
                // them, and a value that differs only there must not read back as a different one.
                value = trimmed.Replace("\r\n", "\n").Replace('\r', '\n');
                return true;
        }
    }

    /// <summary>The values of a list field, trimmed, with blanks and repeats dropped.</summary>
    public static IReadOnlyList<string> SplitList(string value) =>
        [.. value.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Distinct(StringComparer.OrdinalIgnoreCase)];

    public static bool TryParseDate(string text, out DateTime date) =>
        DateTime.TryParseExact(
            text,
            [DateFormat, "yyyy-MM-dd HH:mm", "yyyy-MM-dd", "yyyy-MM-ddTHH:mm:ss", "yyyy:MM:dd HH:mm:ss"],
            CultureInfo.InvariantCulture, DateTimeStyles.None, out date);

    /// <summary>A measurement to one decimal place, above zero and below a million.</summary>
    public static bool TryParseDecimal(string text, out double value) =>
        double.TryParse(text, NumberStyles.AllowDecimalPoint, CultureInfo.InvariantCulture, out value) &&
        (value = Math.Round(value, 1)) is > 0 and < 1_000_000;

    public static string FormatDecimal(double value) => value.ToString("0.#", CultureInfo.InvariantCulture);

    /// <summary>
    /// A shutter time as the fraction a file stores: <c>1/250</c> is 1 over 250, <c>2.5</c> is 25
    /// over 10. A time below a second that is one over a whole number is kept as that fraction,
    /// because that is how every camera and every photographer writes it.
    /// </summary>
    public static bool TryParseExposure(string text, out uint numerator, out uint denominator)
    {
        numerator = denominator = 0;

        var slash = text.IndexOf('/');
        if (slash > 0)
        {
            if (!uint.TryParse(text[..slash].Trim(), NumberStyles.None, CultureInfo.InvariantCulture, out var top) ||
                !uint.TryParse(text[(slash + 1)..].Trim(), NumberStyles.None, CultureInfo.InvariantCulture, out var bottom) ||
                top == 0 || bottom == 0 || bottom > 1_000_000 || top > 1_000_000)
                return false;

            // Anything that is not one-over-something is the same time written as a decimal.
            if (top == 1)
            {
                (numerator, denominator) = (1, bottom);
                return true;
            }

            return FromSeconds((double)top / bottom, out numerator, out denominator);
        }

        return double.TryParse(text, NumberStyles.AllowDecimalPoint, CultureInfo.InvariantCulture, out var seconds) &&
               FromSeconds(seconds, out numerator, out denominator);
    }

    public static string FormatExposure(uint numerator, uint denominator)
    {
        if (numerator == 0 || denominator == 0) return "";
        if (numerator == 1 && denominator > 1) return "1/" + denominator.ToString(CultureInfo.InvariantCulture);

        return FromSeconds((double)numerator / denominator, out var top, out var bottom) && top == 1 && bottom > 1
            ? "1/" + bottom.ToString(CultureInfo.InvariantCulture)
            : FormatDecimal(Math.Round((double)numerator / denominator, 1));
    }

    private static bool FromSeconds(double seconds, out uint numerator, out uint denominator)
    {
        numerator = denominator = 0;
        if (seconds is not (> 0 and <= 86_400)) return false;

        var inverse = 1 / seconds;
        if (seconds < 1 && Math.Abs(inverse - Math.Round(inverse)) < 1e-6 && inverse <= 1_000_000)
        {
            (numerator, denominator) = (1, (uint)Math.Round(inverse));
            return true;
        }

        var tenths = Math.Round(seconds * 10);
        if (tenths < 1) return false;
        (numerator, denominator) = ((uint)tenths, 10);

        // 0.5 is tidier, and more usual, as 1/2.
        if (numerator < 10 && 10 % numerator == 0) (numerator, denominator) = (1, 10 / numerator);
        return true;
    }
}
