using System.Globalization;
using System.Text;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// Which EXIF tags each field is read from and written to.
/// </summary>
/// <remarks>
/// <b>A text field is written twice, on purpose.</b> The standard tags (<c>ImageDescription</c>,
/// <c>Artist</c>) are typed ASCII and are what cameras and most software read; the <c>XP*</c> tags
/// are UTF-16 and are what Windows reads first. Writing one and not the other leaves two programs
/// disagreeing about a picture's title, so both are kept in step and the UTF-16 one wins on read.
/// </remarks>
internal static class ExifFields
{
    private const ushort ImageDescription = 0x010E;
    private const ushort Make = 0x010F;
    private const ushort Model = 0x0110;
    private const ushort OrientationTag = 0x0112;
    private const ushort Artist = 0x013B;
    private const ushort Rating = 0x4746;
    private const ushort RatingPercent = 0x4749;
    private const ushort Copyright = 0x8298;
    private const ushort XpTitle = 0x9C9B;
    private const ushort XpComment = 0x9C9C;
    private const ushort XpAuthor = 0x9C9D;
    private const ushort XpKeywords = 0x9C9E;
    private const ushort XpSubject = 0x9C9F;
    private const ushort DateTimeOriginal = 0x9003;
    private const ushort UserComment = 0x9286;
    private const ushort ExposureTime = 0x829A;
    private const ushort FNumber = 0x829D;
    private const ushort IsoSpeed = 0x8827;
    private const ushort FocalLength = 0x920A;
    private const ushort LensModel = 0xA434;

    /// <summary>Every field EXIF has a tag for — the same set whichever picture format carries it.</summary>
    public static IReadOnlySet<MetadataField> Fields { get; } = new HashSet<MetadataField>
    {
        MetadataField.Title, MetadataField.Author, MetadataField.Subject, MetadataField.Comment,
        MetadataField.Copyright, MetadataField.CameraMake, MetadataField.CameraModel, MetadataField.Keywords,
        MetadataField.Rating, MetadataField.DateTaken, MetadataField.LensModel, MetadataField.Iso,
        MetadataField.Aperture, MetadataField.ExposureTime, MetadataField.FocalLength,
    };

    private const string ExifDateFormat = "yyyy:MM:dd HH:mm:ss";

    private static readonly UTF8Encoding StrictUtf8 = new(false, throwOnInvalidBytes: true);

    public static Dictionary<MetadataField, string> Read(ExifBlock block)
    {
        var values = new Dictionary<MetadataField, string>();

        void Put(MetadataField field, string? value)
        {
            if (!string.IsNullOrWhiteSpace(value)) values[field] = value.Trim();
        }

        Put(MetadataField.Title, Xp(block, XpTitle) ?? Ascii(block, ExifDirectory.Image, ImageDescription));
        Put(MetadataField.Author, Xp(block, XpAuthor) ?? Ascii(block, ExifDirectory.Image, Artist));
        Put(MetadataField.Subject, Xp(block, XpSubject));
        Put(MetadataField.Comment, ReadUserComment(block) ?? Xp(block, XpComment));
        Put(MetadataField.Copyright, Ascii(block, ExifDirectory.Image, Copyright));
        Put(MetadataField.CameraMake, Ascii(block, ExifDirectory.Image, Make));
        Put(MetadataField.CameraModel, Ascii(block, ExifDirectory.Image, Model));

        Put(MetadataField.LensModel, Ascii(block, ExifDirectory.Photo, LensModel));

        if (block.Get(ExifDirectory.Photo, IsoSpeed) is { Length: >= 2 } iso &&
            block.TypeOf(ExifDirectory.Photo, IsoSpeed) == ExifBlock.TypeShort && block.ReadU16(iso) > 0)
            Put(MetadataField.Iso, block.ReadU16(iso).ToString(CultureInfo.InvariantCulture));

        if (Rational(block, FNumber) is (var fTop, var fBottom))
            Put(MetadataField.Aperture, MetadataFields.FormatDecimal(Math.Round((double)fTop / fBottom, 1)));
        if (Rational(block, FocalLength) is (var mmTop, var mmBottom))
            Put(MetadataField.FocalLength, MetadataFields.FormatDecimal(Math.Round((double)mmTop / mmBottom, 1)));
        if (Rational(block, ExposureTime) is (var sTop, var sBottom))
            Put(MetadataField.ExposureTime, MetadataFields.FormatExposure(sTop, sBottom));

        if (Xp(block, XpKeywords) is { } keywords)
            Put(MetadataField.Keywords, string.Join(MetadataFields.ListSeparator, MetadataFields.SplitList(keywords)));

        if (block.Get(ExifDirectory.Image, Rating) is { Length: >= 2 } rating &&
            block.TypeOf(ExifDirectory.Image, Rating) == ExifBlock.TypeShort &&
            block.ReadU16(rating) is >= 1 and <= 5 and var stars)
            Put(MetadataField.Rating, stars.ToString(CultureInfo.InvariantCulture));

        if (Ascii(block, ExifDirectory.Photo, DateTimeOriginal) is { } taken &&
            DateTime.TryParseExact(taken, ExifDateFormat, CultureInfo.InvariantCulture,
                DateTimeStyles.None, out var date) && date.Year > 1)
            Put(MetadataField.DateTaken, date.ToString(MetadataFields.DateFormat, CultureInfo.InvariantCulture));

        return values;
    }

    public static void Apply(ExifBlock block, MetadataEdit edit)
    {
        foreach (var (field, value) in edit.Changes)
        {
            switch (field)
            {
                case MetadataField.Title:
                    SetAscii(block, ExifDirectory.Image, ImageDescription, value);
                    SetXp(block, XpTitle, value);
                    break;
                case MetadataField.Author:
                    SetAscii(block, ExifDirectory.Image, Artist, value);
                    SetXp(block, XpAuthor, value);
                    break;
                case MetadataField.Subject:
                    SetXp(block, XpSubject, value);
                    break;
                case MetadataField.Comment:
                    SetXp(block, XpComment, value);
                    SetUserComment(block, value);
                    break;
                case MetadataField.Copyright:
                    SetAscii(block, ExifDirectory.Image, Copyright, value);
                    break;
                case MetadataField.CameraMake:
                    SetAscii(block, ExifDirectory.Image, Make, value);
                    break;
                case MetadataField.CameraModel:
                    SetAscii(block, ExifDirectory.Image, Model, value);
                    break;
                case MetadataField.LensModel:
                    SetAscii(block, ExifDirectory.Photo, LensModel, value);
                    break;
                case MetadataField.Iso:
                    if (ushort.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out var speed) && speed > 0)
                        block.Set(ExifDirectory.Photo, IsoSpeed, ExifBlock.TypeShort, block.WriteU16(speed));
                    else if (value.Length == 0)
                        block.Remove(ExifDirectory.Photo, IsoSpeed);
                    else
                        throw new MetadataFormatException("An ISO above 65,535 cannot be stored in this tag.");
                    break;
                case MetadataField.Aperture:
                    SetTenths(block, FNumber, value);
                    break;
                case MetadataField.FocalLength:
                    SetTenths(block, FocalLength, value);
                    break;
                case MetadataField.ExposureTime:
                    if (MetadataFields.TryParseExposure(value, out var top, out var bottom))
                        SetRational(block, ExposureTime, top, bottom);
                    else
                        block.Remove(ExifDirectory.Photo, ExposureTime);
                    break;
                case MetadataField.Keywords:
                    // Windows separates these with a bare semicolon, and reads nothing else back.
                    SetXp(block, XpKeywords, string.Join(";", MetadataFields.SplitList(value)));
                    break;
                case MetadataField.Rating:
                    SetRating(block, value);
                    break;
                case MetadataField.DateTaken:
                    SetAscii(block, ExifDirectory.Photo, DateTimeOriginal,
                        MetadataFields.TryParseDate(value, out var date)
                            ? date.ToString(ExifDateFormat, CultureInfo.InvariantCulture)
                            : "");
                    break;
            }
        }
    }

    /// <summary>Which way up the picture is drawn. Not metadata in the sense the rest is: take it
    /// away and a portrait photograph lies on its side.</summary>
    public static ushort? Orientation(ExifBlock block) =>
        block.Get(ExifDirectory.Image, OrientationTag) is { Length: >= 2 } raw &&
        block.TypeOf(ExifDirectory.Image, OrientationTag) == ExifBlock.TypeShort
            ? block.ReadU16(raw)
            : null;

    public static void SetOrientation(ExifBlock block, ushort orientation) =>
        block.Set(ExifDirectory.Image, OrientationTag, ExifBlock.TypeShort, block.WriteU16(orientation));

    /// <summary>A fraction: two unsigned numbers, the second never zero for a value worth reading.</summary>
    private static (uint Top, uint Bottom)? Rational(ExifBlock block, ushort tag)
    {
        if (block.Get(ExifDirectory.Photo, tag) is not { Length: >= 8 } raw ||
            block.TypeOf(ExifDirectory.Photo, tag) != ExifBlock.TypeRational)
            return null;

        var (top, bottom) = (block.ReadU32(raw.AsSpan(0, 4)), block.ReadU32(raw.AsSpan(4, 4)));
        return top == 0 || bottom == 0 ? null : (top, bottom);
    }

    private static void SetRational(ExifBlock block, ushort tag, uint top, uint bottom) =>
        block.Set(ExifDirectory.Photo, tag, ExifBlock.TypeRational, [.. block.WriteU32(top), .. block.WriteU32(bottom)]);

    /// <summary>A measurement to one decimal place, stored as so-many tenths.</summary>
    private static void SetTenths(ExifBlock block, ushort tag, string value)
    {
        if (MetadataFields.TryParseDecimal(value, out var measure))
            SetRational(block, tag, (uint)Math.Round(measure * 10), 10);
        else
            block.Remove(ExifDirectory.Photo, tag);
    }

    private static string? Ascii(ExifBlock block, ExifDirectory directory, ushort tag)
    {
        if (block.Get(directory, tag) is not { } raw) return null;

        var end = Array.IndexOf(raw, (byte)0);
        var bytes = raw.AsSpan(0, end < 0 ? raw.Length : end);

        // Typed ASCII, but UTF-8 is what everything actually writes into it. Anything that is not
        // valid UTF-8 is from an older tool and a single-byte code page, where Latin-1 at least
        // never fails and gets Western text right.
        try
        {
            return StrictUtf8.GetString(bytes);
        }
        catch (DecoderFallbackException)
        {
            return Encoding.Latin1.GetString(bytes);
        }
    }

    private static void SetAscii(ExifBlock block, ExifDirectory directory, ushort tag, string value)
    {
        if (value.Length == 0)
        {
            block.Remove(directory, tag);
            return;
        }

        block.Set(directory, tag, ExifBlock.TypeAscii, [.. Encoding.UTF8.GetBytes(value), 0]);
    }

    private static string? Xp(ExifBlock block, ushort tag)
    {
        if (block.Get(ExifDirectory.Image, tag) is not { Length: >= 2 } raw) return null;

        var text = Encoding.Unicode.GetString(raw, 0, raw.Length & ~1);
        var end = text.IndexOf('\0');
        return end < 0 ? text : text[..end];
    }

    private static void SetXp(ExifBlock block, ushort tag, string value)
    {
        if (value.Length == 0)
        {
            block.Remove(ExifDirectory.Image, tag);
            return;
        }

        block.Set(ExifDirectory.Image, tag, ExifBlock.TypeByte, [.. Encoding.Unicode.GetBytes(value), 0, 0]);
    }

    private static void SetRating(ExifBlock block, string value)
    {
        if (!int.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out var stars) || stars is < 1 or > 5)
        {
            block.Remove(ExifDirectory.Image, Rating);
            block.Remove(ExifDirectory.Image, RatingPercent);
            return;
        }

        // The percentages Windows itself writes for one to five stars.
        ushort percent = stars switch { 1 => 1, 2 => 25, 3 => 50, 4 => 75, _ => 99 };
        block.Set(ExifDirectory.Image, Rating, ExifBlock.TypeShort, block.WriteU16((ushort)stars));
        block.Set(ExifDirectory.Image, RatingPercent, ExifBlock.TypeShort, block.WriteU16(percent));
    }

    /// <summary>
    /// <c>UserComment</c> carries its own eight-byte label saying how the rest is encoded.
    /// </summary>
    private static string? ReadUserComment(ExifBlock block)
    {
        if (block.Get(ExifDirectory.Photo, UserComment) is not { Length: > 8 } raw) return null;

        var label = Encoding.ASCII.GetString(raw, 0, 8);
        var body = raw.AsSpan(8);

        string text;
        if (label.StartsWith("UNICODE", StringComparison.Ordinal))
        {
            var encoding = block.IsLittleEndian ? Encoding.Unicode : Encoding.BigEndianUnicode;
            text = encoding.GetString(body[..(body.Length & ~1)]);
        }
        else if (label.StartsWith("ASCII", StringComparison.Ordinal) || label.Trim('\0').Length == 0)
        {
            text = Encoding.UTF8.GetString(body);
        }
        else
        {
            // JIS, or something private. Not worth guessing at; the XP tag may still answer.
            return null;
        }

        var end = text.IndexOf('\0');
        text = (end < 0 ? text : text[..end]).Trim();
        return text.Length == 0 ? null : text;
    }

    private static void SetUserComment(ExifBlock block, string value)
    {
        if (value.Length == 0)
        {
            block.Remove(ExifDirectory.Photo, UserComment);
            return;
        }

        var encoding = block.IsLittleEndian ? Encoding.Unicode : Encoding.BigEndianUnicode;
        block.Set(ExifDirectory.Photo, UserComment, ExifBlock.TypeUndefined,
            [.. "UNICODE\0"u8, .. encoding.GetBytes(value)]);
    }
}
