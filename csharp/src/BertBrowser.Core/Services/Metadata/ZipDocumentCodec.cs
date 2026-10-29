using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using System.Xml;
using System.Xml.Linq;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// Documents that are a zip of XML parts — Word, Excel and PowerPoint files, and EPUB books — whose
/// properties live in one small part.
/// </summary>
/// <remarks>
/// <para>
/// The whole container is written out again with that one part changed. Every other part is
/// copied entry for entry, in the same order, and the digest is over each one's name and
/// <em>uncompressed</em> bytes: recompressing an entry may produce different bytes on disk for the
/// same content, and it is the content that must not change.
/// </para>
/// <para>
/// A document is somebody else's zip, so it is bounded like one: an entry count and a total size
/// past which it is refused rather than inflated into memory or onto disk.
/// </para>
/// </remarks>
internal abstract class ZipDocumentCodec : IMetadataCodec
{
    private const int MaxEntries = 100_000;
    private const long MaxTotalBytes = 4L << 30;
    private const int MaxPartBytes = 16 << 20;

    public MetadataFamily Family => MetadataFamily.Document;

    public abstract IReadOnlySet<MetadataField> Fields { get; }

    /// <summary>The entry that holds the properties, or null when the document has none.</summary>
    protected abstract string? PropertiesPart(ZipArchive archive);

    protected abstract Dictionary<MetadataField, string> ReadPart(XDocument part);

    /// <summary>Applies <paramref name="edit"/> to the part in place.</summary>
    protected abstract void EditPart(XDocument part, MetadataEdit edit);

    /// <summary>What in the properties part is <em>not</em> a property, for the digest: an EPUB's
    /// part also lists the book's files and their reading order.</summary>
    protected virtual string StructureOf(XDocument part) => "";

    public MetadataDocument Read(Stream source)
    {
        using var archive = Open(source);
        var name = PropertiesPart(archive);
        return new MetadataDocument(Family, name is null ? [] : ReadPart(Parse(Load(archive, name))));
    }

    public void Write(Stream source, Stream destination, MetadataEdit edit, CancellationToken ct)
    {
        using var archive = Open(source);
        var name = PropertiesPart(archive) ?? throw new MetadataFormatException(
            "This document has no properties part to write to, so it was left as it is.");

        var part = Parse(Load(archive, name));
        EditPart(part, edit.RemoveAll
            ? edit with { Changes = Fields.ToDictionary(f => f, f => edit.Changes.GetValueOrDefault(f, "")) }
            : edit);

        using (var output = new ZipArchive(destination, ZipArchiveMode.Create, leaveOpen: true))
        {
            foreach (var entry in archive.Entries)
            {
                ct.ThrowIfCancellationRequested();

                // An EPUB's first entry names its type and must be stored, not compressed, or
                // readers that sniff the first bytes of the file stop recognising it.
                var copy = output.CreateEntry(entry.FullName,
                    entry.FullName == "mimetype" ? CompressionLevel.NoCompression : CompressionLevel.Optimal);
                copy.LastWriteTime = entry.LastWriteTime;

                using var target = copy.Open();
                if (entry.FullName == name)
                {
                    using var writer = XmlWriter.Create(target, new XmlWriterSettings { Encoding = new UTF8Encoding(false) });
                    part.Save(writer);
                }
                else
                {
                    using var from = entry.Open();
                    from.CopyTo(target);
                }
            }
        }

        destination.Flush();
    }

    public byte[] PayloadDigest(Stream source)
    {
        using var archive = Open(source);
        var name = PropertiesPart(archive);

        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var buffer = new byte[128 * 1024];

        foreach (var entry in archive.Entries)
        {
            hash.AppendData(Encoding.UTF8.GetBytes(entry.FullName));
            hash.AppendData([0]);

            if (entry.FullName == name)
            {
                hash.AppendData(Encoding.UTF8.GetBytes(StructureOf(Parse(Load(archive, name)))));
                continue;
            }

            using var content = entry.Open();
            int read;
            while ((read = content.Read(buffer, 0, buffer.Length)) > 0) hash.AppendData(buffer, 0, read);
        }

        return hash.GetHashAndReset();
    }

    private static ZipArchive Open(Stream source)
    {
        try
        {
            source.Position = 0;
            var archive = new ZipArchive(source, ZipArchiveMode.Read, leaveOpen: true);

            if (archive.Entries.Count > MaxEntries || archive.Entries.Sum(e => e.Length) > MaxTotalBytes)
            {
                archive.Dispose();
                throw NotDocument();
            }

            return archive;
        }
        catch (InvalidDataException)
        {
            // Not a zip at all — which is what a password-protected Office file is.
            throw NotDocument();
        }
    }

    protected static byte[] Load(ZipArchive archive, string name)
    {
        var entry = archive.GetEntry(name) ?? throw NotDocument();
        if (entry.Length > MaxPartBytes) throw NotDocument();

        try
        {
            using var content = entry.Open();
            using var buffer = new MemoryStream();
            content.CopyTo(buffer);
            return buffer.ToArray();
        }
        catch (InvalidDataException)
        {
            throw NotDocument();
        }
    }

    protected static XDocument Parse(byte[] xml)
    {
        try
        {
            using var reader = XmlReader.Create(new MemoryStream(xml), new XmlReaderSettings
            {
                // Skipped, never processed: a document type declaration is how hostile XML
                // reaches outside itself, and nothing here needs what one says.
                DtdProcessing = DtdProcessing.Ignore,
                XmlResolver = null,
                MaxCharactersInDocument = MaxPartBytes,
            });
            return XDocument.Load(reader, LoadOptions.PreserveWhitespace);
        }
        catch (XmlException)
        {
            throw NotDocument();
        }
    }

    /// <summary>Sets an element's text, adding it under <paramref name="parent"/> when missing and
    /// removing it when the value is empty.</summary>
    protected static void SetElement(XElement parent, XName name, string value)
    {
        var existing = parent.Elements(name).ToList();
        if (value.Length == 0)
        {
            foreach (var element in existing) element.Remove();
            return;
        }

        if (existing.Count == 0)
        {
            parent.Add(new XElement(name, value));
            return;
        }

        existing[0].Value = value;
        foreach (var extra in existing.Skip(1)) extra.Remove();
    }

    protected static MetadataFormatException NotDocument() =>
        new("This is not a readable document of its kind, so it was left as it is.");
}

/// <summary>Word, Excel and PowerPoint files: the core properties part every one of them carries.</summary>
internal sealed class OpenXmlCodec : ZipDocumentCodec
{
    private const string Part = "docProps/core.xml";

    private static readonly XNamespace Cp = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
    private static readonly XNamespace Dc = "http://purl.org/dc/elements/1.1/";

    private static readonly Dictionary<MetadataField, XName> Elements = new()
    {
        [MetadataField.Title] = Dc + "title",
        [MetadataField.Author] = Dc + "creator",
        [MetadataField.Subject] = Dc + "subject",
        [MetadataField.Keywords] = Cp + "keywords",
        [MetadataField.Comment] = Dc + "description",
        [MetadataField.Category] = Cp + "category",
    };

    public override IReadOnlySet<MetadataField> Fields { get; } = Elements.Keys.ToHashSet();

    protected override string? PropertiesPart(ZipArchive archive) =>
        archive.GetEntry("[Content_Types].xml") is null ? throw NotDocument()
        : archive.GetEntry(Part) is null ? null
        : Part;

    protected override Dictionary<MetadataField, string> ReadPart(XDocument part)
    {
        var values = new Dictionary<MetadataField, string>();
        if (part.Root is not { } root) return values;

        foreach (var (field, name) in Elements)
        {
            if (root.Element(name)?.Value is not { } raw || string.IsNullOrWhiteSpace(raw)) continue;
            if (MetadataFields.TryNormalize(field, raw, out var value) && value.Length > 0) values[field] = value;
        }

        return values;
    }

    protected override void EditPart(XDocument part, MetadataEdit edit)
    {
        var root = part.Root ?? throw NotDocument();
        foreach (var (field, value) in edit.Changes)
            if (Elements.TryGetValue(field, out var name))
                SetElement(root, name, value);

        // Who last saved it is the one property here nobody types and everybody means by "all".
        if (edit.RemoveAll) SetElement(root, Cp + "lastModifiedBy", "");
    }
}

/// <summary>EPUB books: the metadata block of the package document.</summary>
internal sealed class EpubCodec : ZipDocumentCodec
{
    private static readonly XNamespace Opf = "http://www.idpf.org/2007/opf";
    private static readonly XNamespace Dc = "http://purl.org/dc/elements/1.1/";
    private static readonly XNamespace Container = "urn:oasis:names:tc:opendocument:xmlns:container";

    private static readonly Dictionary<MetadataField, XName> Elements = new()
    {
        [MetadataField.Title] = Dc + "title",
        [MetadataField.Author] = Dc + "creator",
        [MetadataField.Comment] = Dc + "description",
        [MetadataField.Publisher] = Dc + "publisher",
        [MetadataField.Copyright] = Dc + "rights",
    };

    public override IReadOnlySet<MetadataField> Fields { get; } =
        Elements.Keys.Append(MetadataField.Keywords).ToHashSet();

    /// <summary>The package document, which the container file names — it has no fixed path.</summary>
    protected override string? PropertiesPart(ZipArchive archive)
    {
        if (archive.GetEntry("META-INF/container.xml") is null) throw NotDocument();

        var path = Parse(Load(archive, "META-INF/container.xml"))
            .Descendants(Container + "rootfile")
            .Select(r => (string?)r.Attribute("full-path"))
            .FirstOrDefault(p => !string.IsNullOrEmpty(p));

        return path is not null && archive.GetEntry(path) is not null ? path : throw NotDocument();
    }

    protected override Dictionary<MetadataField, string> ReadPart(XDocument part)
    {
        var values = new Dictionary<MetadataField, string>();
        if (part.Root?.Element(Opf + "metadata") is not { } metadata) return values;

        foreach (var (field, name) in Elements)
        {
            if (metadata.Element(name)?.Value is { } raw &&
                MetadataFields.TryNormalize(field, raw, out var value) && value.Length > 0)
                values[field] = value;
        }

        // A book's subjects are one element each; a semicolon inside one would split it on the
        // way back, so it is written out as the separator it will be read as.
        var subjects = metadata.Elements(Dc + "subject").Select(s => s.Value.Replace(';', ',').Trim()).Where(s => s.Length > 0);
        if (MetadataFields.TryNormalize(MetadataField.Keywords, string.Join(";", subjects), out var keywords) && keywords.Length > 0)
            values[MetadataField.Keywords] = keywords;

        return values;
    }

    protected override void EditPart(XDocument part, MetadataEdit edit)
    {
        var metadata = part.Root?.Element(Opf + "metadata") ?? throw NotDocument();

        foreach (var (field, value) in edit.Changes)
        {
            if (Elements.TryGetValue(field, out var name))
            {
                // A book must have a title; an emptied one is left as an empty element.
                if (field == MetadataField.Title && value.Length == 0 && metadata.Element(name) is { } title) title.Value = "";
                else SetElement(metadata, name, value);
            }
            else if (field == MetadataField.Keywords)
            {
                foreach (var subject in metadata.Elements(Dc + "subject").ToList()) subject.Remove();
                foreach (var subject in MetadataFields.SplitList(value)) metadata.Add(new XElement(Dc + "subject", subject));
            }
        }
    }

    /// <summary>Everything in the package document but its metadata: the list of the book's files
    /// and the order they are read in.</summary>
    protected override string StructureOf(XDocument part) =>
        string.Concat((part.Root?.Elements() ?? [])
            .Where(e => e.Name != Opf + "metadata")
            .Select(e => e.ToString(SaveOptions.DisableFormatting)));
}
