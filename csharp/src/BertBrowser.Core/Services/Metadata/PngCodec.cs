using System.Buffers.Binary;
using System.Security.Cryptography;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// EXIF in a PNG: the <c>eXIf</c> chunk, which holds the same block a JPEG carries.
/// </summary>
/// <remarks>
/// <para>
/// A PNG is a signature and then chunks, each with a length, a four-letter type and a checksum.
/// The picture is the <c>IDAT</c> chunks, which can be most of a large file — so chunks are found
/// by walking their headers and copied through from the source, never held in memory.
/// </para>
/// <para>
/// The older text chunks (<c>tEXt</c>, <c>iTXt</c>) are not read as fields: they are free-form
/// keyword/value pairs with no agreed meaning beyond a handful, and writing a title to one of them
/// as well as to EXIF would be inventing a convention. An XMP packet in an <c>iTXt</c> is kept in
/// step, exactly as in a JPEG. All of them go when everything is removed.
/// </para>
/// </remarks>
internal sealed class PngCodec : IMetadataCodec
{
    private static ReadOnlySpan<byte> Signature => [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    private static ReadOnlySpan<byte> XmpKeyword => "XML:com.adobe.xmp\0"u8;

    /// <summary>More than any real block or packet; a chunk claiming more is not read into memory.</summary>
    private const int MaxMetadataChunk = 16 << 20;

    public MetadataFamily Family => MetadataFamily.Image;

    public IReadOnlySet<MetadataField> Fields => ExifFields.Fields;

    public MetadataDocument Read(Stream source)
    {
        var chunks = Walk(source);
        var block = chunks.FirstOrDefault(c => c.Type == "eXIf") is { } exif
            ? ExifBlock.Parse(Load(source, exif))
            : null;

        var located = block?.HasLocation == true ||
                      chunks.Where(c => c.Type == "iTXt").Any(c => XmpOf(Load(source, c)) switch
                      {
                          { Compressed: true } => true,
                          { } xmp => XmpPacket.HasLocation(xmp.Packet),
                          null => false,
                      });

        return new MetadataDocument(Family, block is null ? [] : ExifFields.Read(block), located);
    }

    public void Write(Stream source, Stream destination, MetadataEdit edit, CancellationToken ct)
    {
        var chunks = Walk(source);
        var existing = chunks.FirstOrDefault(c => c.Type == "eXIf");

        ExifBlock block;
        bool wanted;

        if (edit.RemoveAll)
        {
            ushort? orientation = null;
            try
            {
                if (existing is not null) orientation = ExifFields.Orientation(ExifBlock.Parse(Load(source, existing)));
            }
            catch (MetadataFormatException)
            {
            }

            block = ExifBlock.Empty();
            if (orientation is > 1 and <= 8) ExifFields.SetOrientation(block, orientation.Value);
            wanted = orientation is > 1 and <= 8 || edit.Changes.Count > 0;
        }
        else
        {
            block = existing is not null ? ExifBlock.Parse(Load(source, existing)) : ExifBlock.Empty();
            wanted = existing is not null || edit.Changes.Count > 0;
            if (edit.RemoveLocation) block.RemoveLocation();
        }

        ExifFields.Apply(block, edit);
        var exif = wanted ? block.ToArray() : null;

        destination.Write(Signature);
        var placed = false;

        foreach (var chunk in chunks)
        {
            ct.ThrowIfCancellationRequested();

            if (chunk.Type == "eXIf") continue;
            if (edit.RemoveAll && IsMetadata(chunk.Type)) continue;

            // Before the picture data, where the specification asks for it; before the end
            // marker at the latest, for a file that somehow has no picture data at all.
            if (!placed && exif is not null && chunk.Type is "IDAT" or "IEND")
            {
                WriteChunk(destination, "eXIf", exif);
                placed = true;
            }

            if (chunk.Type == "iTXt" && XmpOf(Load(source, chunk)) is { } xmp)
            {
                if (xmp.Compressed)
                {
                    if (edit.RemoveLocation)
                        throw new MetadataFormatException(
                            "This picture's location is stored in a form that cannot be safely removed.");
                }
                else
                {
                    var packet = xmp.Packet;
                    if (edit.RemoveLocation) packet = XmpPacket.WithoutLocation(packet) ?? packet;
                    packet = XmpPacket.WithChanges(packet, edit.Changes) ?? packet;

                    if (!ReferenceEquals(packet, xmp.Packet))
                    {
                        WriteChunk(destination, "iTXt", [.. xmp.Prefix, .. packet]);
                        continue;
                    }
                }
            }

            Copy(source, destination, chunk.Offset - 8, chunk.Length + 12, ct);
        }
    }

    public byte[] PayloadDigest(Stream source)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var buffer = new byte[128 * 1024];

        foreach (var chunk in Walk(source))
        {
            if (IsMetadata(chunk.Type)) continue;

            hash.AppendData(System.Text.Encoding.ASCII.GetBytes(chunk.Type));
            source.Position = chunk.Offset;
            for (long left = chunk.Length; left > 0;)
            {
                var read = source.Read(buffer, 0, (int)Math.Min(buffer.Length, left));
                if (read <= 0) throw NotPng();
                hash.AppendData(buffer, 0, read);
                left -= read;
            }
        }

        return hash.GetHashAndReset();
    }

    /// <summary>The chunks that describe the picture rather than being it. Everything that changes
    /// how it is drawn — palette, gamma, colour profile, transparency — is not among them.</summary>
    private static bool IsMetadata(string type) => type is "eXIf" or "tEXt" or "zTXt" or "iTXt" or "tIME";

    /// <param name="Offset">Where the chunk's data starts.</param>
    private sealed record Chunk(string Type, long Offset, long Length);

    private static List<Chunk> Walk(Stream source)
    {
        var head = new byte[8];
        source.Position = 0;
        if (source.ReadAtLeast(head, 8, throwOnEndOfStream: false) != 8 || !head.AsSpan().SequenceEqual(Signature))
            throw NotPng();

        var chunks = new List<Chunk>();
        long position = 8;

        while (true)
        {
            source.Position = position;
            if (source.ReadAtLeast(head, 8, throwOnEndOfStream: false) != 8) throw NotPng();

            long length = BinaryPrimitives.ReadUInt32BigEndian(head);
            var type = System.Text.Encoding.Latin1.GetString(head, 4, 4);
            if (position + 12 + length > source.Length || chunks.Count > 1_000_000) throw NotPng();

            chunks.Add(new Chunk(type, position + 8, length));
            position += 12 + length;

            if (type == "IEND") return chunks;
        }
    }

    private static byte[] Load(Stream source, Chunk chunk)
    {
        if (chunk.Length > MaxMetadataChunk) throw NotPng();

        var data = new byte[chunk.Length];
        source.Position = chunk.Offset;
        if (source.ReadAtLeast(data, data.Length, throwOnEndOfStream: false) != data.Length) throw NotPng();
        return data;
    }

    /// <summary>
    /// The XMP packet inside an international-text chunk, with the bytes before it, or null when
    /// the chunk is some other text.
    /// </summary>
    private static (byte[] Prefix, byte[] Packet, bool Compressed)? XmpOf(byte[] data)
    {
        if (!data.AsSpan().StartsWith(XmpKeyword)) return null;

        // keyword\0, compression flag, compression method, language\0, translated keyword\0, text
        var at = XmpKeyword.Length;
        if (data.Length < at + 2) return null;
        var compressed = data[at] != 0;
        at += 2;

        for (var terminators = 0; terminators < 2; at++)
        {
            if (at >= data.Length) return null;
            if (data[at] == 0) terminators++;
        }

        return (data[..at], data[at..], compressed);
    }

    private static void WriteChunk(Stream destination, string type, byte[] data)
    {
        Span<byte> length = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32BigEndian(length, (uint)data.Length);
        var name = System.Text.Encoding.ASCII.GetBytes(type);

        destination.Write(length);
        destination.Write(name);
        destination.Write(data);

        Span<byte> crc = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32BigEndian(crc, Crc(Crc(0xFFFFFFFF, name), data) ^ 0xFFFFFFFF);
        destination.Write(crc);
    }

    private static void Copy(Stream source, Stream destination, long offset, long length, CancellationToken ct)
    {
        var buffer = new byte[128 * 1024];
        source.Position = offset;
        while (length > 0)
        {
            ct.ThrowIfCancellationRequested();
            var read = source.Read(buffer, 0, (int)Math.Min(buffer.Length, length));
            if (read <= 0) throw NotPng();
            destination.Write(buffer, 0, read);
            length -= read;
        }
    }

    private static readonly uint[] CrcTable = BuildCrcTable();

    private static uint[] BuildCrcTable()
    {
        var table = new uint[256];
        for (uint n = 0; n < 256; n++)
        {
            var c = n;
            for (var k = 0; k < 8; k++) c = (c & 1) != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
            table[n] = c;
        }
        return table;
    }

    /// <summary>The checksum every chunk ends with. Twenty lines here rather than a second caller
    /// of <c>System.IO.Hashing</c>, which the checksum tool keeps to one class on purpose.</summary>
    private static uint Crc(uint crc, ReadOnlySpan<byte> data)
    {
        foreach (var b in data) crc = CrcTable[(crc ^ b) & 0xFF] ^ (crc >> 8);
        return crc;
    }

    private static MetadataFormatException NotPng() =>
        new("This is not a readable PNG picture, so it was left as it is.");
}
