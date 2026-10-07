using System.Text;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// Small real files for the tests and the UI harness to edit.
/// </summary>
/// <remarks>
/// <para>
/// <b>None of them is written by the code under test.</b> A codec that reads back its own output
/// proves only that it agrees with itself, so the pictures come from Windows' own encoder and the
/// MP3 tags are laid out here byte by byte from the ID3 specification. Base64 in a C# file rather
/// than checked-in binaries, for the reason <c>ArchiveFixtures</c> gives: every shipped byte stays
/// visible to a reviewer.
/// </para>
/// <para>
/// Internal, and reachable only through <c>InternalsVisibleTo</c>.
/// </para>
/// </remarks>
internal static partial class MetadataFixtures
{
    /// <summary>
    /// An 8×8 JPEG with a JFIF header and no EXIF at all.
    /// <code>JpegBitmapEncoder { QualityLevel = 60 } over an 8×8 Bgr24 BitmapSource, no metadata</code>
    /// </summary>
    public const string PlainJpeg =
        "/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAA0JCgsKCA0LCgsODg0PEyAVExISEyccHhcgLikxMC4pLSwzOko+MzZGNywtQFdBRkxO" +
        "UlNSMj5aYVpQYEpRUk//2wBDAQ4ODhMREyYVFSZPNS01T09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09P" +
        "T09PT09PT0//wAARCAAIAAgDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUF" +
        "BAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVW" +
        "V1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi" +
        "4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAEC" +
        "AxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVm" +
        "Z2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq" +
        "8vP09fb3+Pn6/9oADAMBAAIRAxEAPwBjxQ2wjaSKGFI5jGDLA9qn08yPhzx9OpFFFFTcpKx//9k=";

    /// <summary>
    /// The same picture as a camera would leave it: big-endian EXIF with a title ("Old title"),
    /// maker ("TestCam"), model ("Model One"), a date taken (2021-06-05 04:03:02), a GPS directory,
    /// and a 48-byte MakerNote of <c>A0..AF</c> repeated — the blob that must not move.
    /// <code>
    /// BitmapMetadata("jpg"): /app1/ifd/{ushort=270,271,272}, /app1/ifd/exif/{ushort=36867},
    /// /app1/ifd/exif/{ushort=37500} = BitmapMetadataBlob(48 bytes), /app1/ifd/gps/{ushort=1,3}
    /// </code>
    /// </summary>
    public const string CameraJpeg =
        "/9j/4AAQSkZJRgABAQEAYABgAAD/4QD+RXhpZgAATU0AKgAAAAgABQEOAAIAAAAKAAAASgEPAAIAAAAIAAAAVAEQAAIAAAAKAAAA" +
        "XIdpAAQAAAABAAAAZoglAAQAAAABAAAA1gAAAABPbGQgdGl0bGUAVGVzdENhbQBNb2RlbCBPbmUAAAOQAwACAAAAFAAAAJCSfAAH" +
        "AAAAMAAAAKTqHQAJAAAAAQAAAAAAAAAAMjAyMTowNjowNSAwNDowMzowMgCgoaKjpKWmp6ipqqusra6voKGio6SlpqeoqaqrrK2u" +
        "r6ChoqOkpaanqKmqq6ytrq8AAAACAAEAAgAAAAJOAAAAAAMAAgAAAAJXAAAAAAAAAAAA/9sAQwANCQoLCggNCwoLDg4NDxMgFRMS" +
        "EhMnHB4XIC4pMTAuKS0sMzpKPjM2RjcsLUBXQUZMTlJTUjI+WmFaUGBKUVJP/9sAQwEODg4TERMmFRUmTzUtNU9PT09PT09PT09P" +
        "T09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09PT09P/8AAEQgACAAIAwEiAAIRAQMRAf/EAB8AAAEFAQEBAQEBAAAA" +
        "AAAAAAABAgMEBQYHCAkKC//EALUQAAIBAwMCBAMFBQQEAAABfQECAwAEEQUSITFBBhNRYQcicRQygZGhCCNCscEVUtHwJDNicoIJ" +
        "ChYXGBkaJSYnKCkqNDU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6g4SFhoeIiYqSk5SVlpeYmZqio6Slpqeo" +
        "qaqys7S1tre4ubrCw8TFxsfIycrS09TV1tfY2drh4uPk5ebn6Onq8fLz9PX29/j5+v/EAB8BAAMBAQEBAQEBAQEAAAAAAAABAgME" +
        "BQYHCAkKC//EALURAAIBAgQEAwQHBQQEAAECdwABAgMRBAUhMQYSQVEHYXETIjKBCBRCkaGxwQkjM1LwFWJy0QoWJDThJfEXGBka" +
        "JicoKSo1Njc4OTpDREVGR0hJSlNUVVZXWFlaY2RlZmdoaWpzdHV2d3h5eoKDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2" +
        "t7i5usLDxMXGx8jJytLT1NXW19jZ2uLj5OXm5+jp6vLz9PX29/j5+v/aAAwDAQACEQMRAD8AY8UNsI2kihhSOYxgywPap9PMj4c8" +
        "fTqRRRRU3KSsf//Z";

    /// <summary>The MakerNote <see cref="CameraJpeg"/> carries, to look for after an edit.</summary>
    public static byte[] MakerNote { get; } = [.. Enumerable.Range(0, 48).Select(i => (byte)(0xA0 + i % 16))];

    public static byte[] Jpeg(string base64) => Convert.FromBase64String(base64);

    /// <summary>
    /// Twenty silent MPEG-1 Layer III frames (128 kbit/s, 44.1 kHz) behind an ID3v2.3 tag.
    /// </summary>
    /// <param name="frames">Frame id and text, e.g. <c>("TIT2", "Song")</c>. Text that is not
    /// Latin-1 is written as UTF-16 with a byte-order mark, as the specification asks.</param>
    /// <param name="withV1">Also append an ID3v1 tag, so there is one at each end.</param>
    public static byte[] Mp3(IEnumerable<(string Id, string Text)> frames, bool withV1 = false)
    {
        var body = new List<byte>();
        foreach (var (id, text) in frames)
        {
            var latin = text.All(c => c <= 0xFF);
            byte[] data = latin
                ? [0, .. Encoding.Latin1.GetBytes(text)]
                : [1, 0xFF, 0xFE, .. Encoding.Unicode.GetBytes(text)];

            body.AddRange(Encoding.ASCII.GetBytes(id));
            body.AddRange(BigEndian(data.Length, 4));
            body.AddRange([0, 0]);
            body.AddRange(data);
        }

        // Padding, which every real tagger leaves and which the tag's size includes.
        body.AddRange(new byte[64]);

        List<byte> file = [(byte)'I', (byte)'D', (byte)'3', 3, 0, 0, .. Syncsafe(body.Count), .. body];
        file.AddRange(Audio());

        if (withV1)
        {
            var v1 = new byte[128];
            "TAG"u8.CopyTo(v1);
            Encoding.Latin1.GetBytes("Old v1 title").CopyTo(v1, 3);
            v1[127] = 255;
            file.AddRange(v1);
        }

        return [.. file];
    }

    /// <summary>The same sound behind an ID3v2.2 tag — three-letter frames, the oldest layout
    /// still found in the wild, which a writer has to read and may not write.</summary>
    public static byte[] Mp3V22(string title, string artist)
    {
        var body = new List<byte>();
        foreach (var (id, text) in new[] { ("TT2", title), ("TP1", artist) })
        {
            byte[] data = [0, .. Encoding.Latin1.GetBytes(text)];
            body.AddRange(Encoding.ASCII.GetBytes(id));
            body.AddRange(BigEndian(data.Length, 3));
            body.AddRange(data);
        }

        return [(byte)'I', (byte)'D', (byte)'3', 2, 0, 0, .. Syncsafe(body.Count), .. body, .. Audio()];
    }

    /// <summary>The frames alone, with no tag at either end.</summary>
    public static byte[] Audio()
    {
        // 144 * 128000 / 44100, unpadded. A header and then zeros is a valid frame of silence.
        const int frameLength = 417;
        var audio = new byte[frameLength * 20];
        for (var i = 0; i < 20; i++)
        {
            audio[i * frameLength] = 0xFF;
            audio[i * frameLength + 1] = 0xFB;
            audio[i * frameLength + 2] = 0x90;
            audio[i * frameLength + 3] = 0x64;
        }
        return audio;
    }

    /// <summary>A tenth of a second of 8 kHz mono 16-bit PCM in a WAV, with no tags: the format
    /// is forty-four bytes of header and then the samples, so it is simply written out.</summary>
    public static byte[] Wav()
    {
        var samples = new byte[1600];
        for (var i = 0; i < samples.Length; i += 2) samples[i] = (byte)(i % 200);

        return
        [
            .. "RIFF"u8, .. BitConverter.GetBytes(36 + samples.Length), .. "WAVEfmt "u8,
            .. BitConverter.GetBytes(16), .. BitConverter.GetBytes((short)1), .. BitConverter.GetBytes((short)1),
            .. BitConverter.GetBytes(8000), .. BitConverter.GetBytes(16000),
            .. BitConverter.GetBytes((short)2), .. BitConverter.GetBytes((short)16),
            .. "data"u8, .. BitConverter.GetBytes(samples.Length), .. samples,
        ];
    }

    /// <summary>
    /// An 8×8 PNG with an alpha channel and no metadata chunks of the kind this app writes.
    /// <code>PngBitmapEncoder over an 8×8 Bgra32 BitmapSource</code>
    /// </summary>
    public const string Png =
        "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAAXNSR0IArs4c6QAAAARnQU1BAACxjwv8YQUAAAAJcEhZcwAADsMAAA7DAcdvqGQAAABNSURBVChT" +
        "Y+RiZeAXwQMYVy1dsB5dEBkwerk6+KMLIgPGV08fvEcXRAaMXa0N/eiCyIBRS1VBH10QGTCeOnrgPLogMmDMSk3IRxdEBgDJVCB5IeABSQAAAABJRU5ErkJggg==";

    /// <summary>
    /// The well-known smallest lossless WebP: one pixel, the simple container form with no
    /// extended header — the case that has to be given one before it can hold metadata.
    /// </summary>
    public const string WebP = "UklGRhoAAABXRUJQVlA4TA0AAAAvAAAAEAcQERGIiP4HAA==";

    /// <summary>
    /// A Word document reduced to the parts a reader needs: content types, relationships, a
    /// one-paragraph body, and core properties with a title ("Quarterly report"), an author
    /// ("A. Writer"), keywords and who last saved it. Laid out by hand from the Open Packaging
    /// Conventions; zipped by <c>System.IO.Compression</c>, which the codec does not write with
    /// differently — so the properties part is what is under test, not the container.
    /// </summary>
    public static byte[] Docx() => Zip(
        ("[Content_Types].xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">" +
            "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>" +
            "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>" +
            "<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>"),
        ("_rels/.rels",
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">" +
            "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>" +
            "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>"),
        ("word/document.xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">" +
            "<w:body><w:p><w:r><w:t>The body of the document, which no edit may touch.</w:t></w:r></w:p></w:body></w:document>"),
        ("docProps/core.xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" " +
            "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">" +
            "<dc:title>Quarterly report</dc:title><dc:creator>A. Writer</dc:creator><cp:keywords>finance; q3</cp:keywords>" +
            "<cp:lastModifiedBy>A. Writer</cp:lastModifiedBy><cp:revision>4</cp:revision>" +
            "<dcterms:created xsi:type=\"dcterms:W3CDTF\">2024-03-14T09:26:53Z</dcterms:created></cp:coreProperties>"));

    /// <summary>
    /// A one-chapter EPUB: the stored <c>mimetype</c> entry first, a container file pointing at a
    /// package document that is <em>not</em> at a fixed path, and a title, an author and two
    /// subjects in its metadata. Laid out by hand from the EPUB specification.
    /// </summary>
    public static byte[] Epub() => Zip(
        ("mimetype", "application/epub+zip"),
        ("META-INF/container.xml",
            "<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">" +
            "<rootfiles><rootfile full-path=\"book/package.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>"),
        ("book/package.opf",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"id\">" +
            "<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"id\">urn:uuid:0</dc:identifier>" +
            "<dc:title>A Short Book</dc:title><dc:creator>Some Author</dc:creator><dc:language>en</dc:language>" +
            "<dc:subject>Fiction</dc:subject><dc:subject>Short stories</dc:subject></metadata>" +
            "<manifest><item id=\"c1\" href=\"chapter1.xhtml\" media-type=\"application/xhtml+xml\"/></manifest>" +
            "<spine><itemref idref=\"c1\"/></spine></package>"),
        ("book/chapter1.xhtml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>One</title></head>" +
            "<body><p>The only chapter, which no edit may touch.</p></body></html>"));

    private static byte[] Zip(params (string Name, string Content)[] entries)
    {
        using var buffer = new MemoryStream();
        using (var archive = new System.IO.Compression.ZipArchive(buffer, System.IO.Compression.ZipArchiveMode.Create, leaveOpen: true))
        {
            foreach (var (name, content) in entries)
            {
                var entry = archive.CreateEntry(name, name == "mimetype"
                    ? System.IO.Compression.CompressionLevel.NoCompression
                    : System.IO.Compression.CompressionLevel.Optimal);
                entry.LastWriteTime = new DateTimeOffset(2024, 3, 14, 9, 26, 52, TimeSpan.Zero);
                using var stream = entry.Open();
                stream.Write(Encoding.UTF8.GetBytes(content));
            }
        }
        return buffer.ToArray();
    }

    /// <summary>A tenth of a second of 8 kHz mono 16-bit PCM as an AIFF with no tags: a form
    /// header, the common chunk with its 80-bit sample rate, and the sound.</summary>
    public static byte[] Aiff()
    {
        var samples = new byte[1600];
        for (var i = 1; i < samples.Length; i += 2) samples[i] = (byte)(i % 200);

        byte[] common =
        [
            0, 1,                                    // one channel
            .. BigEndian(samples.Length / 2, 4),     // sample frames
            0, 16,                                   // bits per sample
            0x40, 0x0B, 0xFA, 0, 0, 0, 0, 0, 0, 0,   // 8000.0 as an 80-bit extended float
        ];
        byte[] sound = [0, 0, 0, 0, 0, 0, 0, 0, .. samples];

        return
        [
            .. "FORM"u8, .. BigEndian(4 + 8 + common.Length + 8 + sound.Length, 4), .. "AIFF"u8,
            .. "COMM"u8, .. BigEndian(common.Length, 4), .. common,
            .. "SSND"u8, .. BigEndian(sound.Length, 4), .. sound,
        ];
    }

    /// <summary>
    /// A fifth of a second of silence as Opus in Ogg: the identification packet, an empty comment
    /// packet, and ten 20 ms frames, on three pages with true checksums. Laid out by hand from
    /// RFC 3533 and RFC 7845; <c>F8 FF FE</c> is the codec's own encoding of a silent frame.
    /// </summary>
    public static byte[] Opus()
    {
        byte[] head = [.. "OpusHead"u8, 1, 1, 0x38, 0x01, 0x80, 0xBB, 0, 0, 0, 0, 0];
        byte[] tags = [.. "OpusTags"u8, 4, 0, 0, 0, .. "hand"u8, 0, 0, 0, 0];
        byte[] frame = [0xF8, 0xFF, 0xFE];

        return
        [
            .. OggPage(0x02, 0, 0, [head]),
            .. OggPage(0x00, 0, 1, [tags]),
            .. OggPage(0x04, 960 * 10 + 312, 2, [.. Enumerable.Repeat(frame, 10)]),
        ];
    }

    private static byte[] OggPage(byte flags, long granule, int sequence, byte[][] packets)
    {
        var table = new List<byte>();
        foreach (var packet in packets)
        {
            for (var left = packet.Length; left >= 255; left -= 255) table.Add(255);
            table.Add((byte)(packet.Length % 255));
        }

        byte[] page =
        [
            .. "OggS"u8, 0, flags, .. BitConverter.GetBytes(granule), .. BitConverter.GetBytes(0x1234ABCD),
            .. BitConverter.GetBytes(sequence), 0, 0, 0, 0, (byte)table.Count, .. table,
            .. packets.SelectMany(p => p),
        ];

        // Ogg's checksum: the CRC-32 polynomial run the other way round from zip's, from zero.
        uint crc = 0;
        foreach (var b in page)
        {
            crc ^= (uint)b << 24;
            for (var bit = 0; bit < 8; bit++) crc = (crc & 0x8000_0000) != 0 ? (crc << 1) ^ 0x04C1_1DB7 : crc << 1;
        }
        BitConverter.GetBytes(crc).CopyTo(page, 22);
        return page;
    }

    /// <summary>
    /// An 8×8 uncompressed TIFF as a scanner would leave it: a title ("Old tiff title"), a maker
    /// ("ScanCo"), a date taken (2020-01-02 03:04:05) and a GPS directory, with the one strip of pixels
    /// sitting <em>before</em> the directory that points at it.
    /// <code>TiffBitmapEncoder { Compression = None } with BitmapMetadata("tiff"): /ifd/{ushort=270,271}, /ifd/exif/{ushort=36867}, /ifd/gps/{ushort=1}</code>
    /// </summary>
    public const string Tiff =
        "SUkqAMgAAAAOBwAjHBU4MSpNRj9iW1R3cGmMhX6hmpO2r6jLxL3g2dL17ucKA/wfGBE0LSZJQjteV1BzbGWIgXqdlo+yq6THwLnc" +
        "1c7x6uMG//gbFA0wKSJFPjdaU0xvaGGEfXaZkouup6DDvLXY0crt5t8C+/QXEAksJR5BOjNWT0hrZF2AeXKVjoeqo5y/uLHUzcbp" +
        "4tv+9/ATDAUoIRo9Ni9SS0RnYFl8dW6RioOmn5i7tK3QycLl3tf68+wPCAEkHRY5MisSAP4ABAABAAAAAAAAAAABBAABAAAACAAA" +
        "AAEBBAABAAAACAAAAAIBAwADAAAApgEAAAMBAwABAAAAAQAAAAYBAwABAAAAAgAAAA4BAgAPAAAArAEAAA8BAgAHAAAAvAEAABEB" +
        "BAABAAAACAAAABUBAwABAAAAAwAAABYBBAABAAAACAAAABcBBAABAAAAwAAAABoBBQABAAAAxAEAABsBBQABAAAAzAEAABwBAwAB" +
        "AAAAAQAAACgBAwABAAAAAgAAAGmHBAABAAAA1AEAACWIBAABAAAA/AEAAAAAAAAIAAgACABPbGQgdGlmZiB0aXRsZQAAU2NhbkNv" +
        "AAAAdwEA6AMAAAB3AQDoAwAAAQADkAIAFAAAAOYBAAAAAAAAMjAyMDowMTowMiAwMzowNDowNQAAAAEAAQACAAIAAABOAAAAAAAA" +
        "AAAA";

    private static byte[] Syncsafe(int value) =>
        [(byte)((value >> 21) & 0x7F), (byte)((value >> 14) & 0x7F), (byte)((value >> 7) & 0x7F), (byte)(value & 0x7F)];

    private static byte[] BigEndian(int value, int width)
    {
        var bytes = new byte[width];
        for (var i = 0; i < width; i++) bytes[width - 1 - i] = (byte)(value >> (8 * i));
        return bytes;
    }
}
