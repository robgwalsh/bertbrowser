using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// Finds the sound in an audio file — the bytes no tag edit may change — without asking the tag
/// library where it is.
/// </summary>
/// <remarks>
/// Deliberately our own reading rather than the library's. The digest of this range is what the
/// executor compares before and after a rewrite, and a check that asks the code under suspicion
/// where to look is not much of a check.
/// </remarks>
internal static class AudioPayload
{
    /// <summary>The extensions a locator exists for. A container with no locator cannot be
    /// edited, however well the library reads it: there would be nothing to check its work with.</summary>
    public static IReadOnlyList<string> Extensions { get; } =
        [".mp3", ".flac", ".wav", ".m4a", ".m4b", ".mp4", ".m4v", ".wma", ".aiff", ".aif", ".opus", ".ogg", ".oga"];

    public static byte[] Digest(string extension, Stream source)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);

        switch (extension)
        {
            case ".mp3":
                var (start, end) = Mp3(source);
                Append(hash, source, start, end - start);
                break;
            case ".flac": Flac(source, hash); break;
            case ".wav": Wave(source, hash); break;
            case ".m4a" or ".m4b" or ".mp4" or ".m4v": Mpeg4(source, hash); break;
            case ".wma": Asf(source, hash); break;
            case ".aiff" or ".aif": Aiff(source, hash); break;
            case ".opus" or ".ogg" or ".oga": Ogg(source, hash); break;
            default: throw new MetadataFormatException("This kind of media file cannot be edited.");
        }

        return hash.GetHashAndReset();
    }

    private static void Append(IncrementalHash hash, Stream source, long start, long length)
    {
        if (start < 0 || length < 0 || start + length > source.Length) throw NotAudio();
        source.Position = start;

        var buffer = new byte[128 * 1024];
        while (length > 0)
        {
            var read = source.Read(buffer, 0, (int)Math.Min(buffer.Length, length));
            if (read <= 0) throw NotAudio();
            hash.AppendData(buffer, 0, read);
            length -= read;
        }
    }

    /// <summary>
    /// FLAC: a marker, a chain of metadata blocks, then frames to the end. The first block says
    /// what the frames are (sample rate, channels, a checksum of the decoded sound) and so counts
    /// as sound; every other block is description.
    /// </summary>
    private static void Flac(Stream source, IncrementalHash hash)
    {
        var position = SkipId3v2(source, 0);
        var head = new byte[4];
        if (ReadAt(source, position, head) != 4 || !head.AsSpan().SequenceEqual("fLaC"u8)) throw NotAudio();
        position += 4;

        while (true)
        {
            if (ReadAt(source, position, head) != 4) throw NotAudio();
            var length = (head[1] << 16) | (head[2] << 8) | head[3];
            if ((head[0] & 0x7F) == 0) Append(hash, source, position + 4, length);

            position += 4 + length;
            if ((head[0] & 0x80) != 0) break;
        }

        var end = source.Length;
        var tail = new byte[3];
        if (end - position >= 128 && ReadAt(source, end - 128, tail) == 3 && tail.AsSpan().SequenceEqual("TAG"u8))
            end -= 128;

        if (end <= position) throw NotAudio();
        Append(hash, source, position, end - position);
    }

    /// <summary>WAV: a list of chunks, of which two are the sound — how it is encoded, and it.</summary>
    private static void Wave(Stream source, IncrementalHash hash)
    {
        var head = new byte[12];
        if (ReadAt(source, 0, head) != 12 ||
            !head.AsSpan(0, 4).SequenceEqual("RIFF"u8) || !head.AsSpan(8, 4).SequenceEqual("WAVE"u8))
            throw NotAudio();

        var found = false;
        long position = 12;
        var chunk = new byte[8];

        while (ReadAt(source, position, chunk) == 8)
        {
            long size = BinaryPrimitives.ReadUInt32LittleEndian(chunk.AsSpan(4));
            var isData = chunk.AsSpan(0, 4).SequenceEqual("data"u8);

            if (isData || chunk.AsSpan(0, 4).SequenceEqual("fmt "u8))
            {
                // A data chunk's size is routinely wrong in a file cut short; take what is there.
                var available = Math.Min(size, source.Length - position - 8);
                hash.AppendData(chunk.AsSpan(0, 4));
                Append(hash, source, position + 8, available);
                found |= isData;
            }

            position += 8 + size + (size & 1);
        }

        if (!found) throw NotAudio();
    }

    /// <summary>
    /// MPEG-4 (M4A, MP4): the sound and picture live in <c>mdat</c>, and a table in <c>moov</c>
    /// says where in the file each piece of it starts.
    /// </summary>
    /// <remarks>
    /// <b>Hashing <c>mdat</c> is not enough here</b>, and this is the format where that matters.
    /// The tags sit in <c>moov</c>, which usually comes first — so growing them pushes <c>mdat</c>
    /// down the file, and every offset in every track's table has to be moved by the same amount.
    /// A writer that misses one leaves the bytes perfect and the file unplayable. So the digest
    /// also takes the first bytes found <em>at</em> each table entry: unchanged only if every
    /// entry still points at what it pointed at before.
    /// </remarks>
    private static void Mpeg4(Stream source, IncrementalHash hash)
    {
        var sawData = false;
        var sawMovie = false;
        var offsets = new List<long>();

        foreach (var (type, start, length) in Atoms(source, 0, source.Length))
        {
            switch (type)
            {
                case "mdat":
                    Append(hash, source, start, length);
                    sawData = true;
                    break;
                case "moov":
                    ChunkOffsets(source, start, start + length, offsets, depth: 0);
                    sawMovie = true;
                    break;
                case "moof":
                    // Fragmented: the tables are spread through the file in a form this does not check.
                    throw new MetadataFormatException("This is a fragmented MP4, which cannot be edited safely.");
            }
        }

        if (!sawData || !sawMovie) throw NotAudio();

        var probe = new byte[16];
        hash.AppendData(BitConverter.GetBytes(offsets.Count));
        foreach (var offset in offsets)
        {
            Array.Clear(probe);
            ReadAt(source, offset, probe);
            hash.AppendData(probe);
        }
    }

    private static void ChunkOffsets(Stream source, long start, long end, List<long> offsets, int depth)
    {
        if (depth > 8) throw NotAudio();

        foreach (var (type, at, length) in Atoms(source, start, end))
        {
            switch (type)
            {
                case "trak" or "mdia" or "minf" or "stbl":
                    ChunkOffsets(source, at, at + length, offsets, depth + 1);
                    break;

                case "stco" or "co64":
                    var wide = type == "co64";
                    var header = new byte[8];
                    if (ReadAt(source, at, header) != 8) throw NotAudio();

                    long count = BinaryPrimitives.ReadUInt32BigEndian(header.AsSpan(4));
                    if (count > 5_000_000 || count * (wide ? 8 : 4) > length - 8) throw NotAudio();

                    var table = new byte[count * (wide ? 8 : 4)];
                    if (ReadAt(source, at + 8, table) != table.Length) throw NotAudio();

                    for (var i = 0; i < count; i++)
                        offsets.Add(wide
                            ? (long)BinaryPrimitives.ReadUInt64BigEndian(table.AsSpan(i * 8))
                            : BinaryPrimitives.ReadUInt32BigEndian(table.AsSpan(i * 4)));
                    break;
            }
        }
    }

    /// <summary>The atoms in a range: each one's type, and where its content starts and how long it is.</summary>
    private static IEnumerable<(string Type, long Start, long Length)> Atoms(Stream source, long start, long end)
    {
        var head = new byte[16];
        var position = start;
        var seen = 0;

        while (position + 8 <= end)
        {
            if (++seen > 100_000 || ReadAt(source, position, head.AsSpan(0, 8)) != 8) throw NotAudio();

            long size = BinaryPrimitives.ReadUInt32BigEndian(head);
            var headerLength = 8;

            if (size == 1)
            {
                if (ReadAt(source, position + 8, head.AsSpan(8, 8)) != 8) throw NotAudio();
                size = (long)BinaryPrimitives.ReadUInt64BigEndian(head.AsSpan(8));
                headerLength = 16;
            }
            else if (size == 0)
            {
                size = end - position;
            }

            if (size < headerLength || position + size > end) throw NotAudio();

            yield return (Encoding.Latin1.GetString(head, 4, 4), position + headerLength, size - headerLength);
            position += size;
        }
    }

    /// <summary>ASF (WMA): a header object holding every tag, then the data object, then indexes.
    /// Everything from the data object on is the sound.</summary>
    private static void Asf(Stream source, IncrementalHash hash)
    {
        ReadOnlySpan<byte> headerObject = [0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C];
        ReadOnlySpan<byte> dataObject = [0x36, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C];

        var head = new byte[24];
        if (ReadAt(source, 0, head) != 24 || !head.AsSpan(0, 16).SequenceEqual(headerObject)) throw NotAudio();

        var data = (long)BinaryPrimitives.ReadUInt64LittleEndian(head.AsSpan(16));
        if (data < 24 || ReadAt(source, data, head) != 24 || !head.AsSpan(0, 16).SequenceEqual(dataObject)) throw NotAudio();

        Append(hash, source, data, source.Length - data);
    }

    /// <summary>AIFF: WAV's big-endian elder. Two chunks are the sound — its description and it.</summary>
    private static void Aiff(Stream source, IncrementalHash hash)
    {
        var head = new byte[12];
        if (ReadAt(source, 0, head) != 12 || !head.AsSpan(0, 4).SequenceEqual("FORM"u8) ||
            !(head.AsSpan(8, 4).SequenceEqual("AIFF"u8) || head.AsSpan(8, 4).SequenceEqual("AIFC"u8)))
            throw NotAudio();

        var found = false;
        long position = 12;
        var chunk = new byte[8];

        while (ReadAt(source, position, chunk) == 8)
        {
            long size = BinaryPrimitives.ReadUInt32BigEndian(chunk.AsSpan(4));
            var isSound = chunk.AsSpan(0, 4).SequenceEqual("SSND"u8);

            if (isSound || chunk.AsSpan(0, 4).SequenceEqual("COMM"u8))
            {
                hash.AppendData(chunk.AsSpan(0, 4));
                Append(hash, source, position + 8, Math.Min(size, source.Length - position - 8));
                found |= isSound;
            }

            position += 8 + size + (size & 1);
        }

        if (!found) throw NotAudio();
    }

    /// <summary>
    /// Ogg (Opus, Vorbis): the stream is cut into pages, and the codec's packets are laid across
    /// them. The second packet is the comments — the tags — and everything else is the sound or
    /// what is needed to decode it.
    /// </summary>
    /// <remarks>
    /// <b>Packets, not pages.</b> Growing the comments moves every later page boundary and
    /// renumbers every page, so no page survives a tag edit byte for byte and hashing pages would
    /// refuse every edit. The packets inside them do survive, so those are what is hashed: every
    /// packet but the second, in order, together with each page's position in time — which a
    /// writer that re-cut the pages carelessly would get wrong while keeping every packet intact.
    /// One logical stream only; anything multiplexed or chained is refused.
    /// </remarks>
    private static void Ogg(Stream source, IncrementalHash hash)
    {
        long position = SkipId3v2(source, 0);
        var head = new byte[27];
        var table = new byte[255];
        var packet = new MemoryStream();
        uint? serial = null;
        var index = 0;
        var pages = 0;

        while (position < source.Length)
        {
            if (ReadAt(source, position, head) != 27 || !head.AsSpan(0, 4).SequenceEqual("OggS"u8) || head[4] != 0)
            {
                // A trailing ID3v1 tag, which some taggers append even here, is not the stream.
                if (pages > 0 && source.Length - position == 128) break;
                throw NotAudio();
            }

            var stream = BinaryPrimitives.ReadUInt32LittleEndian(head.AsSpan(14));
            if (serial is null) serial = stream;
            else if (serial != stream) throw new MetadataFormatException("This Ogg file holds more than one stream, which cannot be edited safely.");
            if (++pages > 10_000_000) throw NotAudio();

            int segments = head[26];
            if (ReadAt(source, position + 27, table.AsSpan(0, segments)) != segments) throw NotAudio();

            var granule = BinaryPrimitives.ReadInt64LittleEndian(head.AsSpan(6));
            long at = position + 27 + segments;
            var completed = false;

            for (var i = 0; i < segments; i++)
            {
                int length = table[i];
                if (length > 0)
                {
                    var piece = new byte[length];
                    if (ReadAt(source, at, piece) != length) throw NotAudio();

                    // The comment packet is the one thing left out; it can also be megabytes of
                    // embedded cover, which there is no reason to hold.
                    if (index != 1) packet.Write(piece);
                    at += length;
                }

                if (length == 255) continue; // the packet runs on into the next segment or page

                if (index != 1)
                {
                    hash.AppendData(BitConverter.GetBytes(index == 0 ? 0 : 2));
                    hash.AppendData(BitConverter.GetBytes((int)packet.Length));
                    hash.AppendData(packet.GetBuffer(), 0, (int)packet.Length);
                    if (index >= 2) completed = true;
                }

                packet.SetLength(0);
                index++;
            }

            // Where in time the page ends, for the pages that end a piece of sound.
            if (completed && granule > 0) hash.AppendData(BitConverter.GetBytes(granule));

            position = at;
        }

        if (index < 3) throw NotAudio();
    }

    private static long SkipId3v2(Stream source, long start)
    {
        var head = new byte[10];
        while (ReadAt(source, start, head) == head.Length &&
               head[0] == 'I' && head[1] == 'D' && head[2] == '3' &&
               (head[6] | head[7] | head[8] | head[9]) < 0x80)
        {
            var size = (head[6] << 21) | (head[7] << 14) | (head[8] << 7) | head[9];
            start += 10 + size + ((head[5] & 0x10) != 0 ? 10 : 0);
        }
        return start;
    }

    /// <summary>
    /// An MP3 is frames with tags bolted on either end: ID3v2 in front, and ID3v1, APE and Lyrics3
    /// behind in any combination.
    /// </summary>
    private static (long Start, long End) Mp3(Stream source)
    {
        var length = source.Length;
        var start = SkipId3v2(source, 0);

        // Padding a tagger left between the tag and the first frame is not sound, and a rewrite
        // may tidy it away. Zeros cannot be the start of a frame, so they cannot be mistaken.
        start = SkipZeros(source, start, length);

        var end = length;
        var tail = new byte[32];
        while (true)
        {
            if (end - start >= 128 && ReadAt(source, end - 128, tail.AsSpan(0, 3)) == 3 &&
                tail[0] == 'T' && tail[1] == 'A' && tail[2] == 'G')
            {
                end -= 128;
                continue;
            }

            if (end - start >= 32 && ReadAt(source, end - 32, tail) == 32 &&
                tail.AsSpan(0, 8).SequenceEqual("APETAGEX"u8))
            {
                long size = BinaryPrimitives.ReadUInt32LittleEndian(tail.AsSpan(12, 4));
                var hasHeader = (BinaryPrimitives.ReadUInt32LittleEndian(tail.AsSpan(20, 4)) & 0x8000_0000) != 0;
                var whole = size + (hasHeader ? 32 : 0);
                if (whole < 32 || whole > end - start) break;
                end -= whole;
                continue;
            }

            if (end - start >= 15 && ReadAt(source, end - 15, tail.AsSpan(0, 15)) == 15 &&
                tail.AsSpan(6, 9).SequenceEqual("LYRICS200"u8) &&
                long.TryParse(Encoding.ASCII.GetString(tail, 0, 6), out var lyrics) &&
                lyrics + 15 <= end - start)
            {
                end -= lyrics + 15;
                continue;
            }

            break;
        }

        if (end <= start || !StartsWithFrame(source, start, end)) throw NotAudio();
        return (start, end);
    }

    /// <summary>
    /// Whether real MPEG audio turns up early in the range: a frame header whose fields are all
    /// legal, followed exactly one frame later by another.
    /// </summary>
    /// <remarks>
    /// Eleven set bits alone is no test at all — every JPEG marker is <c>FF</c> and then something,
    /// and a picture renamed <c>.mp3</c> is full of them. Two headers the right distance apart is
    /// what a decoder itself looks for before it believes it has found the stream.
    /// </remarks>
    private static bool StartsWithFrame(Stream source, long start, long end)
    {
        var window = new byte[(int)Math.Min(64 * 1024, end - start)];
        var read = ReadAt(source, start, window);

        for (var i = 0; i + 4 <= read; i++)
        {
            if (FrameLength(window.AsSpan(i, 4)) is not { } length) continue;

            var next = i + length;
            if (start + next + 4 > end) return true; // the last frame of a very short file
            if (next + 4 <= read && FrameLength(window.AsSpan(next, 4)) is not null) return true;
        }

        return false;
    }

    /// <summary>The length of the Layer III frame this header starts, or null when the four bytes
    /// are not a header.</summary>
    private static int? FrameLength(ReadOnlySpan<byte> header)
    {
        if (header[0] != 0xFF || (header[1] & 0xE0) != 0xE0) return null;

        var version = (header[1] >> 3) & 3;   // 3 = MPEG-1, 2 = MPEG-2, 0 = MPEG-2.5, 1 = reserved
        var layer = (header[1] >> 1) & 3;     // 1 = Layer III
        var bitrateIndex = header[2] >> 4;
        var rateIndex = (header[2] >> 2) & 3;
        var padding = (header[2] >> 1) & 1;

        if (version == 1 || layer != 1 || bitrateIndex is 0 or 15 || rateIndex == 3) return null;

        ReadOnlySpan<int> first = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
        ReadOnlySpan<int> later = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
        ReadOnlySpan<int> rates = [44100, 48000, 32000];

        var bitrate = (version == 3 ? first : later)[bitrateIndex] * 1000;
        var rate = rates[rateIndex] >> (version switch { 3 => 0, 2 => 1, _ => 2 });

        return (version == 3 ? 144 : 72) * bitrate / rate + padding;
    }

    private static long SkipZeros(Stream source, long start, long length)
    {
        var buffer = new byte[4096];
        while (start < length)
        {
            var read = ReadAt(source, start, buffer);
            if (read <= 0) break;

            for (var i = 0; i < read; i++)
                if (buffer[i] != 0)
                    return start + i;

            start += read;
        }
        return start;
    }

    private static int ReadAt(Stream source, long position, Span<byte> into)
    {
        if (position < 0 || position >= source.Length) return 0;
        source.Position = position;
        return source.ReadAtLeast(into, into.Length, throwOnEndOfStream: false);
    }

    private static MetadataFormatException NotAudio() =>
        new("This is not a readable audio file, so it was left as it is.");
}
