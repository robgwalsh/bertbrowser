using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// EXIF in a WebP: a RIFF file whose <c>EXIF</c> chunk holds the block a JPEG carries.
/// </summary>
/// <remarks>
/// <para>
/// <b>A WebP with metadata has to be the "extended" kind.</b> The simple form is one image chunk
/// and nothing else; anything more needs a <c>VP8X</c> header in front that states the canvas size
/// and has a flag bit for each optional chunk. So giving a simple file its first EXIF means
/// writing that header too, with the size read out of the image chunk's own first bytes — and
/// keeping the flags true on every later edit, since a decoder believes them over what it finds.
/// </para>
/// <para>
/// The image chunks are copied through from the source, never held or decoded.
/// </para>
/// </remarks>
internal sealed class WebPCodec : IMetadataCodec
{
    private const byte ExifFlag = 0x08;
    private const byte XmpFlag = 0x04;
    private const byte AlphaFlag = 0x10;

    /// <summary>The flags that mean a decoder draws the file differently: a colour profile, animation.</summary>
    private const byte DrawingFlags = 0x20 | 0x02;

    private const int MaxMetadataChunk = 16 << 20;

    public MetadataFamily Family => MetadataFamily.Image;

    public IReadOnlySet<MetadataField> Fields => ExifFields.Fields;

    public MetadataDocument Read(Stream source)
    {
        var chunks = Walk(source);
        var block = chunks.FirstOrDefault(c => c.Type == "EXIF") is { } exif
            ? ExifBlock.Parse(ExifOf(Load(source, exif)))
            : null;

        var located = block?.HasLocation == true ||
                      chunks.Where(c => c.Type == "XMP ").Any(c => XmpPacket.HasLocation(Load(source, c)));

        return new MetadataDocument(Family, block is null ? [] : ExifFields.Read(block), located);
    }

    public void Write(Stream source, Stream destination, MetadataEdit edit, CancellationToken ct)
    {
        var chunks = Walk(source);
        var existing = chunks.FirstOrDefault(c => c.Type == "EXIF");

        ExifBlock block;
        bool wanted;

        if (edit.RemoveAll)
        {
            ushort? orientation = null;
            try
            {
                if (existing is not null)
                    orientation = ExifFields.Orientation(ExifBlock.Parse(ExifOf(Load(source, existing))));
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
            block = existing is not null ? ExifBlock.Parse(ExifOf(Load(source, existing))) : ExifBlock.Empty();
            wanted = existing is not null || edit.Changes.Count > 0;
            if (edit.RemoveLocation) block.RemoveLocation();
        }

        ExifFields.Apply(block, edit);
        var exif = wanted ? block.ToArray() : null;

        var packets = new List<byte[]>();
        if (!edit.RemoveAll)
        {
            foreach (var chunk in chunks.Where(c => c.Type == "XMP "))
            {
                var packet = Load(source, chunk);
                if (edit.RemoveLocation) packet = XmpPacket.WithoutLocation(packet) ?? packet;
                packets.Add(XmpPacket.WithChanges(packet, edit.Changes) ?? packet);
            }
        }

        // Nothing optional left and none there before: the file stays the simple kind it was.
        var header = chunks.FirstOrDefault(c => c.Type == "VP8X");
        byte[]? extended = null;
        if (header is not null || exif is not null || packets.Count > 0)
        {
            extended = header is not null ? Load(source, header) : NewHeader(source, chunks);
            if (extended.Length < 10) throw NotWebP();

            extended[0] = (byte)((extended[0] & ~(ExifFlag | XmpFlag))
                                 | (exif is not null ? ExifFlag : 0)
                                 | (packets.Count > 0 ? XmpFlag : 0));

            // And back again: a header left describing one plain image and nothing else is one a
            // simple file never had, so a picture stripped of everything is the file it started as.
            var rest = chunks.Where(c => c.Type is not ("VP8X" or "EXIF" or "XMP ")).ToList();
            if (exif is null && packets.Count == 0 && (extended[0] & DrawingFlags) == 0 &&
                rest is [{ Type: "VP8 " or "VP8L" }])
                extended = null;
        }

        destination.Write("RIFF\0\0\0\0WEBP"u8);

        if (extended is not null) WriteChunk(destination, "VP8X", extended);
        foreach (var chunk in chunks)
        {
            ct.ThrowIfCancellationRequested();
            if (chunk.Type is "VP8X" or "EXIF" or "XMP ") continue;

            Copy(source, destination, chunk.Offset - 8, 8 + chunk.Length + (chunk.Length & 1), ct);
        }

        // After the image, in the order the container asks for: EXIF, then XMP.
        if (exif is not null) WriteChunk(destination, "EXIF", exif);
        foreach (var packet in packets) WriteChunk(destination, "XMP ", packet);

        var size = destination.Position - 8;
        if (size > uint.MaxValue) throw NotWebP();

        Span<byte> total = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(total, (uint)size);
        destination.Position = 4;
        destination.Write(total);
        destination.Position = destination.Length;
    }

    public byte[] PayloadDigest(Stream source)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var buffer = new byte[128 * 1024];

        foreach (var chunk in Walk(source))
        {
            if (chunk.Type is "EXIF" or "XMP ") continue;

            if (chunk.Type == "VP8X")
            {
                // The header counts only when it says something about drawing. Its metadata flags
                // are ours to change, and a file given its first EXIF gains a header it never had
                // — one that says nothing the image chunk did not already.
                var data = Load(source, chunk);
                if (data.Length < 10) throw NotWebP();
                if ((data[0] & DrawingFlags) == 0) continue;

                data[0] &= unchecked((byte)~(ExifFlag | XmpFlag));
                hash.AppendData("VP8X"u8);
                hash.AppendData(data);
                continue;
            }

            hash.AppendData(Encoding.ASCII.GetBytes(chunk.Type));
            source.Position = chunk.Offset;
            for (var left = chunk.Length; left > 0;)
            {
                var read = source.Read(buffer, 0, (int)Math.Min(buffer.Length, left));
                if (read <= 0) throw NotWebP();
                hash.AppendData(buffer, 0, read);
                left -= read;
            }
        }

        return hash.GetHashAndReset();
    }

    /// <summary>
    /// The extended header for a file that had none: no flags yet, and the canvas size taken from
    /// the one image chunk a simple file consists of.
    /// </summary>
    private static byte[] NewHeader(Stream source, List<Chunk> chunks)
    {
        var image = chunks.FirstOrDefault(c => c.Type is "VP8 " or "VP8L") ?? throw NotWebP();

        var head = new byte[10];
        source.Position = image.Offset;
        if (source.ReadAtLeast(head, head.Length, throwOnEndOfStream: false) != head.Length) throw NotWebP();

        int width, height;
        var alpha = false;

        if (image.Type == "VP8L")
        {
            if (head[0] != 0x2F) throw NotWebP();
            var bits = BinaryPrimitives.ReadUInt32LittleEndian(head.AsSpan(1));
            width = (int)(bits & 0x3FFF) + 1;
            height = (int)((bits >> 14) & 0x3FFF) + 1;
            alpha = ((bits >> 28) & 1) != 0;
        }
        else
        {
            // A key frame: three bytes of frame tag, a start code, then both sizes in 14 bits each.
            if (head[3] != 0x9D || head[4] != 0x01 || head[5] != 0x2A) throw NotWebP();
            width = BinaryPrimitives.ReadUInt16LittleEndian(head.AsSpan(6)) & 0x3FFF;
            height = BinaryPrimitives.ReadUInt16LittleEndian(head.AsSpan(8)) & 0x3FFF;
        }

        if (width < 1 || height < 1) throw NotWebP();

        var header = new byte[10];
        header[0] = alpha ? AlphaFlag : (byte)0;
        header[4] = (byte)(width - 1);
        header[5] = (byte)((width - 1) >> 8);
        header[6] = (byte)((width - 1) >> 16);
        header[7] = (byte)(height - 1);
        header[8] = (byte)((height - 1) >> 8);
        header[9] = (byte)((height - 1) >> 16);
        return header;
    }

    /// <summary>Some writers put a JPEG's <c>Exif\0\0</c> label in front of the block; the block
    /// itself starts at its byte-order mark either way.</summary>
    private static ReadOnlySpan<byte> ExifOf(byte[] data) =>
        data.AsSpan().StartsWith("Exif\0\0"u8) ? data.AsSpan(6) : data;

    /// <param name="Offset">Where the chunk's data starts.</param>
    private sealed record Chunk(string Type, long Offset, long Length);

    private static List<Chunk> Walk(Stream source)
    {
        var head = new byte[12];
        source.Position = 0;
        if (source.ReadAtLeast(head, 12, throwOnEndOfStream: false) != 12 ||
            !head.AsSpan(0, 4).SequenceEqual("RIFF"u8) || !head.AsSpan(8, 4).SequenceEqual("WEBP"u8))
            throw NotWebP();

        var end = Math.Min(source.Length, 8L + BinaryPrimitives.ReadUInt32LittleEndian(head.AsSpan(4)));
        var chunks = new List<Chunk>();
        long position = 12;

        while (position + 8 <= end)
        {
            source.Position = position;
            if (source.ReadAtLeast(head.AsSpan(0, 8), 8, throwOnEndOfStream: false) != 8) throw NotWebP();

            long length = BinaryPrimitives.ReadUInt32LittleEndian(head.AsSpan(4));
            if (position + 8 + length > source.Length || chunks.Count > 1_000_000) throw NotWebP();

            chunks.Add(new Chunk(Encoding.Latin1.GetString(head, 0, 4), position + 8, length));
            position += 8 + length + (length & 1);
        }

        if (!chunks.Any(c => c.Type is "VP8 " or "VP8L" or "ANMF")) throw NotWebP();
        return chunks;
    }

    private static byte[] Load(Stream source, Chunk chunk)
    {
        if (chunk.Length > MaxMetadataChunk) throw NotWebP();

        var data = new byte[chunk.Length];
        source.Position = chunk.Offset;
        if (source.ReadAtLeast(data, data.Length, throwOnEndOfStream: false) != data.Length) throw NotWebP();
        return data;
    }

    private static void WriteChunk(Stream destination, string type, byte[] data)
    {
        Span<byte> length = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(length, (uint)data.Length);

        destination.Write(Encoding.ASCII.GetBytes(type));
        destination.Write(length);
        destination.Write(data);
        if ((data.Length & 1) != 0) destination.WriteByte(0);
    }

    private static void Copy(Stream source, Stream destination, long offset, long length, CancellationToken ct)
    {
        var buffer = new byte[128 * 1024];
        source.Position = offset;
        length = Math.Min(length, source.Length - offset);
        while (length > 0)
        {
            ct.ThrowIfCancellationRequested();
            var read = source.Read(buffer, 0, (int)Math.Min(buffer.Length, length));
            if (read <= 0) throw NotWebP();
            destination.Write(buffer, 0, read);
            length -= read;
        }

        // A last chunk of odd length whose padding byte the file left off still needs one here.
        if ((destination.Position & 1) != 0) destination.WriteByte(0);
    }

    private static MetadataFormatException NotWebP() =>
        new("This is not a readable WebP picture, so it was left as it is.");
}
