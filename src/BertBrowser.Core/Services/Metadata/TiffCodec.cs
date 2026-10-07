using System.Buffers.Binary;
using System.Security.Cryptography;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// A TIFF picture — which <em>is</em> an EXIF block, from its first byte to its last: the same
/// directories, with the picture's own strips of pixels hanging off the first one.
/// </summary>
/// <remarks>
/// <para>
/// So it is edited by <see cref="ExifBlock"/> exactly as a JPEG's block is, and for the same
/// reason nothing existing may move: the directory that says where each strip of pixels is holds
/// their positions as offsets from the start of the file. Changed values go over their own old
/// bytes or on the end; the pixels are never read, moved or rewritten.
/// </para>
/// <para>
/// <b>The digest reads the file its own way</b>, not through that class: a walk of the directory
/// chain that hashes each picture's shape and every strip or tile its offsets point at. It is the
/// check on the editor, so it must not share the editor's reading.
/// </para>
/// <para>
/// The whole file is held in memory while it is edited, so it is bounded; a larger one is refused
/// by name. BigTIFF and the camera raw formats built on TIFF are not this codec's to touch.
/// </para>
/// </remarks>
internal sealed class TiffCodec : IMetadataCodec
{
    /// <summary>The largest file edited. Past this it is a scan or a stitched panorama, and
    /// holding two copies of it in memory to change a title is the wrong trade.</summary>
    public const int MaxBytes = 256 << 20;

    private const ushort XmpTag = 700;

    /// <summary>What goes from the main directory when everything is removed: descriptions,
    /// names, software and dates, and the three places other programs keep their own records.</summary>
    private static readonly ushort[] Personal =
        [270, 271, 272, 305, 306, 315, 33432, 0x4746, 0x4749, 0x9C9B, 0x9C9C, 0x9C9D, 0x9C9E, 0x9C9F, XmpTag, 33723, 34377, 37724, 50341];

    public MetadataFamily Family => MetadataFamily.Image;

    public IReadOnlySet<MetadataField> Fields => ExifFields.Fields;

    public MetadataDocument Read(Stream source)
    {
        var block = ExifBlock.Parse(Load(source));
        var located = block.HasLocation ||
                      (block.Get(ExifDirectory.Image, XmpTag) is { } xmp && XmpPacket.HasLocation(xmp));

        return new MetadataDocument(Family, ExifFields.Read(block), located);
    }

    public void Write(Stream source, Stream destination, MetadataEdit edit, CancellationToken ct)
    {
        var block = ExifBlock.Parse(Load(source));

        if (edit.RemoveAll)
        {
            block.RemovePhotoDirectory();
            block.RemoveLocation();
            foreach (var tag in Personal) block.Remove(ExifDirectory.Image, tag);
        }
        else
        {
            if (block.Get(ExifDirectory.Image, XmpTag) is { } packet)
            {
                var synced = packet;
                if (edit.RemoveLocation) synced = XmpPacket.WithoutLocation(synced) ?? synced;
                synced = XmpPacket.WithChanges(synced, edit.Changes) ?? synced;

                if (!ReferenceEquals(synced, packet))
                    block.Set(ExifDirectory.Image, XmpTag, ExifBlock.TypeByte, synced);
            }

            if (edit.RemoveLocation) block.RemoveLocation();
        }

        ExifFields.Apply(block, edit);

        ct.ThrowIfCancellationRequested();
        destination.Write(block.ToArray(int.MaxValue));
    }

    public byte[] PayloadDigest(Stream source)
    {
        var data = Load(source);
        var little = data[0] == 'I';
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);

        var seen = new HashSet<long>();
        var pending = new Queue<long>();
        pending.Enqueue(U32(data, 4, little));

        while (pending.TryDequeue(out var offset))
        {
            if (offset == 0) continue;
            if (!seen.Add(offset) || seen.Count > 10_000 || offset + 2 > data.Length) throw NotTiff();

            var count = U16(data, offset, little);
            var end = offset + 2 + 12L * count;
            if (end + 4 > data.Length) throw NotTiff();

            long[]? offsets = null, lengths = null;
            hash.AppendData("IFD"u8);

            for (var i = 0; i < count; i++)
            {
                var at = offset + 2 + 12L * i;
                var tag = U16(data, at, little);

                switch (tag)
                {
                    // Where the pixels are, and how much of them: strips, or tiles.
                    case 273 or 324: offsets = Values(data, at, little); break;
                    case 279 or 325: lengths = Values(data, at, little); break;

                    // More pictures hanging off this one — reduced copies, pages of a raw file.
                    case 330:
                        foreach (var child in Values(data, at, little)) pending.Enqueue(child);
                        break;

                    // What shape the pixels are and how to read them: size, depth, compression,
                    // colour model, channel count, layout, predictor, palette, tile size.
                    case 256 or 257 or 258 or 259 or 262 or 277 or 278 or 284 or 317 or 320 or 322 or 323 or 338 or 339:
                        hash.AppendData(BitConverter.GetBytes(tag));
                        foreach (var value in Values(data, at, little)) hash.AppendData(BitConverter.GetBytes(value));
                        break;
                }
            }

            if (offsets is not null || lengths is not null)
            {
                if (offsets is null || lengths is null || offsets.Length != lengths.Length) throw NotTiff();

                for (var i = 0; i < offsets.Length; i++)
                {
                    if (offsets[i] < 0 || lengths[i] < 0 || offsets[i] + lengths[i] > data.Length) throw NotTiff();
                    hash.AppendData(BitConverter.GetBytes(lengths[i]));
                    hash.AppendData(data, (int)offsets[i], (int)lengths[i]);
                }
            }

            pending.Enqueue(U32(data, end, little));
        }

        return hash.GetHashAndReset();
    }

    /// <summary>A tag's values as numbers, whichever of the three whole-number types holds them.</summary>
    private static long[] Values(byte[] data, long entry, bool little)
    {
        var type = U16(data, entry + 2, little);
        long count = U32(data, entry + 4, little);
        var size = type switch { 1 => 1, 3 => 2, 4 => 4, _ => 0 };
        if (size == 0 || count > 16_000_000) throw NotTiff();

        var at = size * count <= 4 ? entry + 8 : U32(data, entry + 8, little);
        if (at + size * count > data.Length) throw NotTiff();

        var values = new long[count];
        for (var i = 0; i < count; i++)
            values[i] = size switch
            {
                1 => data[at + i],
                2 => U16(data, at + 2L * i, little),
                _ => U32(data, at + 4L * i, little),
            };
        return values;
    }

    private static byte[] Load(Stream source)
    {
        if (source.Length > MaxBytes)
            throw new MetadataFormatException("This TIFF is larger than 256 MB, which is more than can be edited here.");

        var data = new byte[source.Length];
        source.Position = 0;
        if (source.ReadAtLeast(data, data.Length, throwOnEndOfStream: false) != data.Length || data.Length < 8) throw NotTiff();

        var little = data[0] == 'I' && data[1] == 'I';
        if (!little && !(data[0] == 'M' && data[1] == 'M')) throw NotTiff();

        return U16(data, 2, little) switch
        {
            42 => data,
            43 => throw new MetadataFormatException("This is a BigTIFF, which cannot be edited here."),
            _ => throw NotTiff(),
        };
    }

    private static ushort U16(byte[] data, long at, bool little) =>
        little
            ? BinaryPrimitives.ReadUInt16LittleEndian(data.AsSpan((int)at, 2))
            : BinaryPrimitives.ReadUInt16BigEndian(data.AsSpan((int)at, 2));

    private static uint U32(byte[] data, long at, bool little) =>
        little
            ? BinaryPrimitives.ReadUInt32LittleEndian(data.AsSpan((int)at, 4))
            : BinaryPrimitives.ReadUInt32BigEndian(data.AsSpan((int)at, 4));

    private static MetadataFormatException NotTiff() =>
        new("This is not a readable TIFF picture, so it was left as it is.");
}
