using System.Buffers.Binary;
using System.Text;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// The IPTC record older software writes into a JPEG, inside Photoshop's resource segment.
/// </summary>
/// <remarks>
/// <para>
/// <b>Only ever brought into step, never created</b> — the rule <see cref="XmpPacket"/> follows,
/// for the same reason. A picture that already states its title here would otherwise go on
/// stating the old one to every program that reads this record first.
/// </para>
/// <para>
/// <b>What cannot be said truthfully is not said.</b> Each field here has a fixed maximum length
/// and, unless the record declares UTF-8, a single-byte character set. A value that does not fit
/// either is taken out of the record rather than cut short or written with question marks: a
/// reader then falls back to the EXIF or XMP, which hold it whole.
/// </para>
/// <para>
/// Photoshop also stores a checksum of this record so other programs can tell whether it was
/// edited behind XMP's back. It is removed whenever the record changes: a stale one says the
/// record is out of date, which after this edit it is not.
/// </para>
/// </remarks>
internal static class IptcBlock
{
    private const ushort IptcResource = 0x0404;
    private const ushort IptcDigestResource = 0x0425;

    private static ReadOnlySpan<byte> Label => "Photoshop 3.0\0"u8;

    /// <summary>Dataset number in record 2, and the most bytes it may hold.</summary>
    private static readonly Dictionary<MetadataField, (byte Dataset, int Max)> Singles = new()
    {
        [MetadataField.Title] = (5, 64),
        [MetadataField.Author] = (80, 32),
        [MetadataField.Copyright] = (116, 128),
    };

    private const byte Keywords = 25;
    private const byte Caption = 120;

    /// <summary>
    /// The segment with the same changes made to its IPTC record, or null when it has no such
    /// record, needs no change, or is not laid out as expected — in which case it is left alone.
    /// </summary>
    public static byte[]? WithChanges(ReadOnlySpan<byte> segment, IReadOnlyDictionary<MetadataField, string> changes)
    {
        if (changes.Count == 0 || !segment.StartsWith(Label)) return null;
        if (Resources(segment[Label.Length..]) is not { } resources) return null;

        var index = resources.FindIndex(r => r.Id == IptcResource);
        if (index < 0 || Datasets(resources[index].Data) is not { } datasets) return null;

        var utf8 = datasets.Any(d => d is { Record: 1, Number: 90 } && d.Data.AsSpan().SequenceEqual((ReadOnlySpan<byte>)[0x1B, 0x25, 0x47]));
        var touched = false;

        void Replace(byte number, IEnumerable<string> values, int max, bool onlyIfPresent = false)
        {
            var had = datasets.RemoveAll(d => d.Record == 2 && d.Number == number) > 0;
            if (onlyIfPresent && !had) return;
            touched = true;

            foreach (var value in values)
            {
                if (Encode(value, utf8) is { } bytes && bytes.Length <= max)
                    datasets.Add((2, number, bytes));
            }
        }

        foreach (var (field, value) in changes)
        {
            if (Singles.TryGetValue(field, out var single))
                Replace(single.Dataset, value.Length == 0 ? [] : [value], single.Max);

            if (field == MetadataField.Keywords)
                Replace(Keywords, MetadataFields.SplitList(value), 64);

            // The caption follows the title only where there already is one: see XmpPacket.
            if (field == MetadataField.Title)
                Replace(Caption, value.Length == 0 ? [] : [value], 2000, onlyIfPresent: true);
        }

        if (!touched) return null;

        // Records in order, as the standard asks; within a record, as they were.
        var record = new List<byte>();
        foreach (var (rec, number, data) in datasets.OrderBy(d => d.Record))
        {
            record.AddRange([0x1C, rec, number, (byte)(data.Length >> 8), (byte)data.Length]);
            record.AddRange(data);
        }

        resources[index] = resources[index] with { Data = [.. record] };
        resources.RemoveAll(r => r.Id == IptcDigestResource);

        var output = new List<byte>(Label.ToArray());
        foreach (var resource in resources)
        {
            output.AddRange("8BIM"u8.ToArray());
            output.AddRange([(byte)(resource.Id >> 8), (byte)resource.Id]);
            output.AddRange(resource.Name);
            output.AddRange([(byte)(resource.Data.Length >> 24), (byte)(resource.Data.Length >> 16), (byte)(resource.Data.Length >> 8), (byte)resource.Data.Length]);
            output.AddRange(resource.Data);
            if (resource.Data.Length % 2 != 0) output.Add(0);
        }

        return [.. output];
    }

    private static byte[]? Encode(string value, bool utf8)
    {
        if (utf8) return Encoding.UTF8.GetBytes(value);
        return value.All(c => c <= 0xFF) ? Encoding.Latin1.GetBytes(value) : null;
    }

    /// <param name="Name">The resource's name exactly as stored: a length byte, the characters,
    /// and the padding that makes the whole an even number of bytes.</param>
    private sealed record Resource(ushort Id, byte[] Name, byte[] Data);

    private static List<Resource>? Resources(ReadOnlySpan<byte> data)
    {
        var resources = new List<Resource>();
        var at = 0;

        while (at < data.Length)
        {
            if (data.Length - at < 12 || !data.Slice(at, 4).SequenceEqual("8BIM"u8)) return null;

            var id = BinaryPrimitives.ReadUInt16BigEndian(data[(at + 4)..]);
            var nameLength = 1 + data[at + 6];
            if (nameLength % 2 != 0) nameLength++;
            if (at + 6 + nameLength + 4 > data.Length) return null;

            var name = data.Slice(at + 6, nameLength).ToArray();
            var size = BinaryPrimitives.ReadUInt32BigEndian(data[(at + 6 + nameLength)..]);
            var start = at + 6 + nameLength + 4;
            if (size > data.Length - start) return null;

            resources.Add(new Resource(id, name, data.Slice(start, (int)size).ToArray()));
            at = start + (int)size + (int)(size % 2);
        }

        return resources;
    }

    private static List<(byte Record, byte Number, byte[] Data)>? Datasets(ReadOnlySpan<byte> data)
    {
        var datasets = new List<(byte, byte, byte[])>();
        var at = 0;

        while (at < data.Length)
        {
            // Trailing padding is common; anything else that is not a dataset is not ours to rewrite.
            if (data[at] == 0 && data[at..].IndexOfAnyExcept((byte)0) < 0) break;
            if (data.Length - at < 5 || data[at] != 0x1C) return null;

            var length = BinaryPrimitives.ReadUInt16BigEndian(data[(at + 3)..]);
            // The extended-length form, for datasets over 32 KB. Nothing this app writes needs
            // one, and a record that uses one is carried through untouched instead.
            if ((length & 0x8000) != 0 || at + 5 + length > data.Length) return null;

            datasets.Add((data[at + 1], data[at + 2], data.Slice(at + 5, length).ToArray()));
            at += 5 + length;
        }

        return datasets;
    }
}
