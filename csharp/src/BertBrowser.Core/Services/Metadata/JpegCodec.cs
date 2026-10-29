using System.Security.Cryptography;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// EXIF in a JPEG, edited by splicing one header segment. The picture is never decoded.
/// </summary>
/// <remarks>
/// A JPEG is a run of labelled segments, then the compressed scan, which runs to the end of the
/// file. Only the segments are parsed; everything from the first scan marker on is copied through
/// as it is, which is also why a phone's second image, depth map or motion clip appended after the
/// picture survives an edit untouched.
/// </remarks>
internal sealed class JpegCodec : IMetadataCodec
{
    private const byte App0 = 0xE0;
    private const byte App1 = 0xE1;
    private const byte App13 = 0xED;
    private const byte Comment = 0xFE;
    private const byte StartOfScan = 0xDA;
    private const byte EndOfImage = 0xD9;

    /// <summary>More header than any real picture carries. A file that is all header and no scan
    /// is refused rather than read into memory whole.</summary>
    private const long MaxHeaderBytes = 32 << 20;

    private static ReadOnlySpan<byte> ExifLabel => "Exif\0\0"u8;
    private static ReadOnlySpan<byte> XmpLabel => "http://ns.adobe.com/xap/1.0/\0"u8;
    private static ReadOnlySpan<byte> ExtendedXmpLabel => "http://ns.adobe.com/xmp/extension/\0"u8;
    private static ReadOnlySpan<byte> PhotoshopLabel => "Photoshop 3.0\0"u8;

    public MetadataFamily Family => MetadataFamily.Image;

    public IReadOnlySet<MetadataField> Fields => ExifFields.Fields;

    public MetadataDocument Read(Stream source)
    {
        var header = Header.Parse(source);
        var block = header.Exif is { } exif ? ExifBlock.Parse(exif.Data!.AsSpan(ExifLabel.Length)) : null;

        var located = block?.HasLocation == true ||
                      header.Segments.Any(s =>
                          (IsXmp(s) && XmpPacket.HasLocation(s.Data.AsSpan(XmpLabel.Length))) ||
                          (IsExtendedXmp(s) && s.Data.AsSpan().IndexOf("GPS"u8) >= 0));

        return new MetadataDocument(Family, block is null ? [] : ExifFields.Read(block), located);
    }

    public void Write(Stream source, Stream destination, MetadataEdit edit, CancellationToken ct)
    {
        var header = Header.Parse(source);
        var segments = header.Segments.ToList();
        var existing = header.Exif;

        ExifBlock block;
        bool wanted;

        if (edit.RemoveAll)
        {
            // Read before everything goes, and from a block that may be too damaged to read: a
            // picture nobody can parse the EXIF of must still be strippable.
            ushort? orientation = null;
            try
            {
                if (existing is not null)
                    orientation = ExifFields.Orientation(ExifBlock.Parse(existing.Data!.AsSpan(ExifLabel.Length)));
            }
            catch (MetadataFormatException)
            {
            }

            segments.RemoveAll(IsMetadata);
            existing = null;

            block = ExifBlock.Empty();
            if (orientation is > 1 and <= 8) ExifFields.SetOrientation(block, orientation.Value);
            wanted = orientation is > 1 and <= 8 || edit.Changes.Count > 0;
        }
        else
        {
            block = existing is not null
                ? ExifBlock.Parse(existing.Data!.AsSpan(ExifLabel.Length))
                : ExifBlock.Empty();
            wanted = existing is not null || edit.Changes.Count > 0;

            if (edit.RemoveLocation) RemoveLocation(block, segments);
        }

        ExifFields.Apply(block, edit);

        // A packet that already states a field has to change with it; see XmpPacket.WithChanges.
        for (var i = 0; i < segments.Count; i++)
        {
            if (IsXmp(segments[i]) &&
                XmpPacket.WithChanges(segments[i].Data.AsSpan(XmpLabel.Length), edit.Changes) is { } synced)
                segments[i] = segments[i] with { Data = [.. XmpLabel, .. synced] };

            // And the older IPTC record, on the same terms.
            if (segments[i].Marker == App13 && segments[i].Data is { } resources &&
                IptcBlock.WithChanges(resources, edit.Changes) is { } record)
                segments[i] = segments[i] with { Data = record };
        }

        if (wanted)
        {
            var replacement = new Segment(App1, 0, [.. ExifLabel, .. block.ToArray()]);
            if (existing is not null)
            {
                segments[segments.IndexOf(existing)] = replacement with { Fill = existing.Fill };
            }
            else
            {
                // After the JFIF header when there is one — readers expect it first — else at the top.
                var at = 0;
                while (at < segments.Count && segments[at].Marker == App0) at++;
                segments.Insert(at, replacement);
            }
        }

        destination.Write([0xFF, 0xD8]);
        foreach (var segment in segments)
        {
            ct.ThrowIfCancellationRequested();
            segment.WriteTo(destination);
        }

        source.Position = header.TailOffset;
        var buffer = new byte[128 * 1024];
        int read;
        while ((read = source.Read(buffer, 0, buffer.Length)) > 0)
        {
            ct.ThrowIfCancellationRequested();
            destination.Write(buffer, 0, read);
        }
    }

    public byte[] PayloadDigest(Stream source)
    {
        var header = Header.Parse(source);
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);

        foreach (var segment in header.Segments)
        {
            if (IsMetadata(segment)) continue;
            hash.AppendData([0xFF, segment.Marker]);
            if (segment.Data is { } data) hash.AppendData(data);
        }

        source.Position = header.TailOffset;
        var buffer = new byte[128 * 1024];
        int read;
        while ((read = source.Read(buffer, 0, buffer.Length)) > 0) hash.AppendData(buffer, 0, read);

        return hash.GetHashAndReset();
    }

    /// <summary>
    /// A location is written in up to two places, and removing it from one is not removing it.
    /// </summary>
    private static void RemoveLocation(ExifBlock block, List<Segment> segments)
    {
        block.RemoveLocation();

        for (var i = 0; i < segments.Count; i++)
        {
            var segment = segments[i];

            // The overflow of a packet too large for one segment arrives in pieces that are not
            // XML on their own. Nothing writes coordinates there in practice; if something has,
            // it cannot be edited safely and saying so beats claiming it is gone.
            if (IsExtendedXmp(segment) && segment.Data.AsSpan().IndexOf("GPS"u8) >= 0)
                throw new MetadataFormatException(
                    "This picture's location is stored in a form that cannot be safely removed.");

            if (!IsXmp(segment)) continue;

            if (XmpPacket.WithoutLocation(segment.Data.AsSpan(XmpLabel.Length)) is { } cleaned)
                segments[i] = segment with { Data = [.. XmpLabel, .. cleaned] };
        }
    }

    private static bool IsExif(Segment segment) =>
        segment.Marker == App1 && segment.Data is { } data && data.AsSpan().StartsWith(ExifLabel);

    private static bool IsXmp(Segment segment) =>
        segment.Marker == App1 && segment.Data is { } data && data.AsSpan().StartsWith(XmpLabel);

    private static bool IsExtendedXmp(Segment segment) =>
        segment.Marker == App1 && segment.Data is { } data && data.AsSpan().StartsWith(ExtendedXmpLabel);

    /// <summary>The segments that describe the picture rather than being it. A colour profile is
    /// not among them: change it and the same pixels are a different colour.</summary>
    private static bool IsMetadata(Segment segment)
    {
        if (segment.Data is not { } data) return false;
        var span = data.AsSpan();

        return segment.Marker switch
        {
            App1 => span.StartsWith(ExifLabel) || span.StartsWith(XmpLabel) || span.StartsWith(ExtendedXmpLabel),
            App13 => span.StartsWith(PhotoshopLabel),
            Comment => true,
            _ => false,
        };
    }

    /// <param name="Fill">Padding <c>FF</c> bytes before the marker, which the format allows and
    /// which are written back so nothing but the metadata differs.</param>
    /// <param name="Data">The segment's content, or null for a marker that carries none.</param>
    private sealed record Segment(byte Marker, int Fill, byte[]? Data)
    {
        public void WriteTo(Stream destination)
        {
            for (var i = 0; i < Fill; i++) destination.WriteByte(0xFF);
            destination.Write([0xFF, Marker]);
            if (Data is null) return;

            var length = Data.Length + 2;
            if (length > ushort.MaxValue)
                throw new MetadataFormatException("There is no room left in this picture's EXIF block.");

            destination.Write([(byte)(length >> 8), (byte)length]);
            destination.Write(Data);
        }
    }

    private sealed record Header(IReadOnlyList<Segment> Segments, long TailOffset)
    {
        /// <summary>The first EXIF segment. A second one is carried through untouched: readers
        /// take the first, so that is the one worth editing.</summary>
        public Segment? Exif => Segments.FirstOrDefault(IsExif);

        public static Header Parse(Stream source)
        {
            source.Position = 0;
            if (source.ReadByte() != 0xFF || source.ReadByte() != 0xD8) throw NotJpeg();

            var segments = new List<Segment>();
            long held = 0;

            while (true)
            {
                var start = source.Position;
                if (source.ReadByte() != 0xFF) throw NotJpeg();

                var fill = 0;
                int marker;
                while ((marker = source.ReadByte()) == 0xFF) fill++;
                if (marker < 0) throw NotJpeg();

                if (marker is StartOfScan or EndOfImage)
                    return new Header(segments, start);

                // Restart markers and TEM stand alone; nothing else before the scan does.
                if (marker is (>= 0xD0 and <= 0xD7) or 0x01)
                {
                    segments.Add(new Segment((byte)marker, fill, null));
                    continue;
                }

                int high = source.ReadByte(), low = source.ReadByte();
                if (low < 0) throw NotJpeg();

                var length = (high << 8) | low;
                if (length < 2) throw NotJpeg();

                held += length;
                if (held > MaxHeaderBytes) throw NotJpeg();

                var data = new byte[length - 2];
                try
                {
                    source.ReadExactly(data);
                }
                catch (EndOfStreamException)
                {
                    throw NotJpeg();
                }

                segments.Add(new Segment((byte)marker, fill, data));
            }
        }

        private static MetadataFormatException NotJpeg() =>
            new("This is not a readable JPEG picture, so it was left as it is.");
    }
}
