using System.Text;
using System.Xml;
using System.Xml.Linq;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// An XMP packet — the XML description many programs write beside a picture's EXIF.
/// </summary>
/// <remarks>
/// <para>
/// <b>Untrusted XML</b>, so it is read with DTDs prohibited and a size limit: a picture from the
/// internet must not be able to make this app fetch a URL or expand a billion entities.
/// </para>
/// <para>
/// <b>When in doubt, the answer is that a location is there.</b> A packet this cannot parse but
/// which mentions GPS is reported as holding one, and removing it is then refused — the opposite
/// mistake is telling somebody a photograph is safe to share when it still says where they live.
/// </para>
/// </remarks>
internal static class XmpPacket
{
    private const string ExifNamespace = "http://ns.adobe.com/exif/1.0/";
    private const int MaxCharacters = 8 << 20;

    private static readonly UTF8Encoding StrictUtf8 = new(false, throwOnInvalidBytes: true);

    /// <summary>Whether the packet says where the picture was taken.</summary>
    public static bool HasLocation(ReadOnlySpan<byte> packet)
    {
        if (!MentionsGps(packet)) return false;
        return Parse(packet) is not { } document || LocationNodes(document).Any();
    }

    /// <summary>
    /// The packet with its GPS properties taken out, or null when it had none.
    /// </summary>
    /// <exception cref="MetadataFormatException">It mentions a location in a form that cannot be
    /// parsed, and so cannot be shown to be gone.</exception>
    public static byte[]? WithoutLocation(ReadOnlySpan<byte> packet)
    {
        if (!MentionsGps(packet)) return null;

        var document = Parse(packet) ?? throw new MetadataFormatException(
            "This picture's location is stored in a form that cannot be safely removed.");

        var nodes = LocationNodes(document).ToList();
        if (nodes.Count == 0) return null;

        foreach (var node in nodes)
        {
            if (node is XAttribute attribute) attribute.Remove();
            else if (node is XElement element) element.Remove();
        }

        var written = Serialize(document);

        // Checked on the bytes that will be written, not on the tree they came from.
        if (HasLocation(written))
            throw new MetadataFormatException(
                "This picture's location is stored in a form that cannot be safely removed.");

        return written;
    }

    /// <summary>
    /// The packet with the same changes made to it that were made to the EXIF, or null when it
    /// did not need any or could not be parsed.
    /// </summary>
    /// <remarks>
    /// <b>Only ever brought into step, never created.</b> Many programs — Windows among them —
    /// read a title from here before they look at EXIF, so a packet that already states one has to
    /// change with it, or the edit appears not to have happened everywhere but in this app. A
    /// picture with no packet is left without one: EXIF alone says the same thing to everybody.
    /// </remarks>
    public static byte[]? WithChanges(ReadOnlySpan<byte> packet, IReadOnlyDictionary<MetadataField, string> changes)
    {
        if (changes.Count == 0 || Parse(packet) is not { } document) return null;

        var descriptions = document.Descendants(Rdf + "Description").ToList();
        if (descriptions.Count == 0) return null;

        foreach (var (field, value) in changes)
        {
            switch (field)
            {
                // The EXIF tag a title is written to is the one XMP calls the description, and
                // Windows writes a title to both. So a description that is already there follows
                // the title; one is not invented for a picture that has only a title.
                case MetadataField.Title:
                    SetArray(descriptions, Dc + "title", "Alt", value.Length == 0 ? [] : [value]);
                    SetArray(descriptions, Dc + "description", "Alt", value.Length == 0 ? [] : [value], onlyIfPresent: true);
                    break;
                case MetadataField.Copyright: SetArray(descriptions, Dc + "rights", "Alt", value.Length == 0 ? [] : [value]); break;
                case MetadataField.Author: SetArray(descriptions, Dc + "creator", "Seq", value.Length == 0 ? [] : [value]); break;
                case MetadataField.Keywords: SetArray(descriptions, Dc + "subject", "Bag", MetadataFields.SplitList(value)); break;
                case MetadataField.Comment: SetArray(descriptions, Exif + "UserComment", "Alt", value.Length == 0 ? [] : [value]); break;
                case MetadataField.Rating: SetSimple(descriptions, Xmp + "Rating", value); break;
                case MetadataField.CameraMake: SetSimple(descriptions, Tiff + "Make", value); break;
                case MetadataField.CameraModel: SetSimple(descriptions, Tiff + "Model", value); break;
                case MetadataField.DateTaken:
                    var iso = MetadataFields.TryParseDate(value, out var date)
                        ? date.ToString("yyyy-MM-ddTHH:mm:ss", System.Globalization.CultureInfo.InvariantCulture)
                        : "";
                    SetSimple(descriptions, Exif + "DateTimeOriginal", iso);
                    SetSimple(descriptions, Photoshop + "DateCreated", iso, onlyIfPresent: true);
                    break;
            }
        }

        return Serialize(document);
    }

    private static readonly XNamespace Rdf = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
    private static readonly XNamespace Dc = "http://purl.org/dc/elements/1.1/";
    private static readonly XNamespace Xmp = "http://ns.adobe.com/xap/1.0/";
    private static readonly XNamespace Exif = ExifNamespace;
    private static readonly XNamespace Tiff = "http://ns.adobe.com/tiff/1.0/";
    private static readonly XNamespace Photoshop = "http://ns.adobe.com/photoshop/1.0/";

    /// <summary>A property is written either as an attribute or as a child element, and a packet
    /// may say it in more than one description. Every statement of it goes; one comes back.</summary>
    private static bool Clear(List<XElement> descriptions, XName name)
    {
        var found = false;
        foreach (var description in descriptions)
        {
            if (description.Attribute(name) is { } attribute)
            {
                attribute.Remove();
                found = true;
            }

            foreach (var element in description.Elements(name).ToList())
            {
                element.Remove();
                found = true;
            }
        }
        return found;
    }

    private static void SetSimple(List<XElement> descriptions, XName name, string value, bool onlyIfPresent = false)
    {
        var had = Clear(descriptions, name);
        if (value.Length == 0 || (onlyIfPresent && !had)) return;

        Declare(descriptions[0], name);
        descriptions[0].Add(new XElement(name, value));
    }

    private static void SetArray(
        List<XElement> descriptions, XName name, string kind, IReadOnlyList<string> values, bool onlyIfPresent = false)
    {
        var had = Clear(descriptions, name);
        if (values.Count == 0 || (onlyIfPresent && !had)) return;

        Declare(descriptions[0], name);
        descriptions[0].Add(new XElement(name,
            new XElement(Rdf + kind, values.Select(v => kind == "Alt"
                ? new XElement(Rdf + "li", new XAttribute(XNamespace.Xml + "lang", "x-default"), v)
                : new XElement(Rdf + "li", v)))));
    }

    /// <summary>Gives a namespace its usual prefix where the packet has not declared one, so the
    /// result reads <c>dc:title</c> rather than an invented <c>p1:title</c>.</summary>
    private static void Declare(XElement description, XName name)
    {
        if (description.GetPrefixOfNamespace(name.Namespace) is not null) return;

        var prefix = name.NamespaceName switch
        {
            "http://purl.org/dc/elements/1.1/" => "dc",
            "http://ns.adobe.com/xap/1.0/" => "xmp",
            ExifNamespace => "exif",
            "http://ns.adobe.com/tiff/1.0/" => "tiff",
            "http://ns.adobe.com/photoshop/1.0/" => "photoshop",
            _ => null,
        };

        if (prefix is not null) description.Add(new XAttribute(XNamespace.Xmlns + prefix, name.NamespaceName));
    }

    private static byte[] Serialize(XDocument document)
    {
        using var buffer = new MemoryStream();
        using (var writer = XmlWriter.Create(buffer, new XmlWriterSettings
               {
                   Encoding = new UTF8Encoding(false),
                   OmitXmlDeclaration = true,
               }))
        {
            document.Save(writer);
        }

        return buffer.ToArray();
    }

    private static IEnumerable<XObject> LocationNodes(XDocument document) =>
        document.Descendants()
            .SelectMany(e => e.Attributes().Cast<XObject>().Append(e))
            .Where(IsLocation);

    private static bool IsLocation(XObject node)
    {
        var name = node switch
        {
            XAttribute attribute => attribute.Name,
            XElement element => element.Name,
            _ => null,
        };

        return name is not null &&
               name.NamespaceName == ExifNamespace &&
               name.LocalName.StartsWith("GPS", StringComparison.Ordinal);
    }

    /// <summary>A cheap test before the expensive one: no packet without these three letters has
    /// a GPS property, in any encoding XMP allows.</summary>
    private static bool MentionsGps(ReadOnlySpan<byte> packet) =>
        packet.IndexOf("GPS"u8) >= 0 ||
        packet.IndexOf("G\0P\0S"u8) >= 0;

    private static XDocument? Parse(ReadOnlySpan<byte> packet)
    {
        string text;
        try
        {
            text = StrictUtf8.GetString(packet);
        }
        catch (DecoderFallbackException)
        {
            return null;
        }

        try
        {
            using var reader = XmlReader.Create(new StringReader(text.TrimEnd('\0', ' ', '\r', '\n', '\t')),
                new XmlReaderSettings
                {
                    DtdProcessing = DtdProcessing.Prohibit,
                    XmlResolver = null,
                    MaxCharactersInDocument = MaxCharacters,
                });

            return XDocument.Load(reader, LoadOptions.PreserveWhitespace);
        }
        catch (XmlException)
        {
            return null;
        }
    }
}
