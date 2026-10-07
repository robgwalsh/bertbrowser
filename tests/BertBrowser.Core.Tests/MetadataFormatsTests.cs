using System.IO.Compression;
using System.Text;
using BertBrowser.Core.Services.Metadata;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The formats beyond JPEG and MP3: PNG and WebP pictures, Office documents and EPUB books, the
/// camera-setting fields, and the audio fields and cover pictures every container is asked to keep.
/// </summary>
public class MetadataFormatsTests
{
    private static MetadataEdit Edit(params (MetadataField Field, string Value)[] changes) =>
        new(changes.ToDictionary(c => c.Field, c => c.Value));

    private static byte[] Write(IMetadataCodec codec, byte[] file, MetadataEdit edit)
    {
        using var source = new MemoryStream(file);
        using var destination = new MemoryStream();
        codec.Write(source, destination, edit, CancellationToken.None);
        return destination.ToArray();
    }

    private static MetadataDocument Read(IMetadataCodec codec, byte[] file) => codec.Read(new MemoryStream(file));

    private static byte[] Digest(IMetadataCodec codec, byte[] file) => codec.PayloadDigest(new MemoryStream(file));

    /// <summary>A value of the right kind for each field, with text no single-byte code page holds.</summary>
    private static string Sample(MetadataField field) => MetadataFields.Get(field).Kind switch
    {
        MetadataFieldKind.Number => field == MetadataField.Year ? "1997" : "7",
        MetadataFieldKind.Date => "2024-03-14 09:26:53",
        MetadataFieldKind.List => "sea; boats; 日本",
        MetadataFieldKind.Rating => "4",
        MetadataFieldKind.Decimal => "2.8",
        MetadataFieldKind.Exposure => "1/250",
        _ => field == MetadataField.Lyrics ? "First line\nSecond line" : $"Värde {field} 日本",
    };

    private static MetadataEdit Everything(IMetadataCodec codec) =>
        new(codec.Fields.ToDictionary(f => f, Sample));

    // ---- values ---------------------------------------------------------------------------------

    [Theory]
    [InlineData("1/250", "1/250")]
    [InlineData("0.004", "1/250")]
    [InlineData("0.5", "1/2")]
    [InlineData("2/4", "1/2")]
    [InlineData("0.3", "0.3")]
    [InlineData("2.5", "2.5")]
    [InlineData("30", "30")]
    [InlineData("1", "1")]
    public void A_shutter_time_is_written_the_way_a_camera_shows_it(string typed, string expected)
    {
        Assert.True(MetadataFields.TryNormalize(MetadataField.ExposureTime, typed, out var value));
        Assert.Equal(expected, value);

        // And what is stored reads back as exactly what was typed, or every edit would fail its check.
        Assert.True(MetadataFields.TryParseExposure(value, out var top, out var bottom));
        Assert.Equal(value, MetadataFields.FormatExposure(top, bottom));
    }

    [Theory]
    [InlineData(MetadataField.ExposureTime, "fast")]
    [InlineData(MetadataField.ExposureTime, "1/0")]
    [InlineData(MetadataField.ExposureTime, "0")]
    [InlineData(MetadataField.Aperture, "-2.8")]
    [InlineData(MetadataField.Aperture, "f/2.8")]
    [InlineData(MetadataField.FocalLength, "0")]
    public void A_measurement_that_is_not_one_is_refused(MetadataField field, string typed) =>
        Assert.False(MetadataFields.TryNormalize(field, typed, out _));

    [Fact]
    public void An_aperture_keeps_one_decimal_place()
    {
        Assert.True(MetadataFields.TryNormalize(MetadataField.Aperture, "2.80", out var value));
        Assert.Equal("2.8", value);
        Assert.True(MetadataFields.TryNormalize(MetadataField.FocalLength, "50", out value));
        Assert.Equal("50", value);
    }

    // ---- pictures -------------------------------------------------------------------------------

    public static TheoryData<string> Pictures => [".jpg", ".png", ".webp"];

    private static (IMetadataCodec Codec, byte[] File) Picture(string extension) => extension switch
    {
        ".jpg" => (new JpegCodec(), MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg)),
        ".png" => (new PngCodec(), Convert.FromBase64String(MetadataFixtures.Png)),
        ".webp" => (new WebPCodec(), Convert.FromBase64String(MetadataFixtures.WebP)),
        _ => throw new ArgumentOutOfRangeException(nameof(extension)),
    };

    [Theory]
    [MemberData(nameof(Pictures))]
    public void Every_picture_format_takes_every_exif_field_and_keeps_its_pixels(string extension)
    {
        var (codec, original) = Picture(extension);
        Assert.Empty(Read(codec, original).Values);

        var edit = Everything(codec);
        var written = Write(codec, original, edit);
        var doc = Read(codec, written);

        foreach (var (field, value) in edit.Changes) Assert.Equal(value, doc.Get(field));
        Assert.Equal(Digest(codec, original), Digest(codec, written));

        // Edited again in place, then emptied.
        var again = Write(codec, written, Edit((MetadataField.Title, "Shorter"), (MetadataField.Iso, "")));
        Assert.Equal("Shorter", Read(codec, again).Get(MetadataField.Title));
        Assert.Equal("", Read(codec, again).Get(MetadataField.Iso));
        Assert.Equal("1/250", Read(codec, again).Get(MetadataField.ExposureTime));
        Assert.Equal(Digest(codec, original), Digest(codec, again));

        var stripped = Write(codec, again, MetadataEdit.None with { RemoveAll = true });
        Assert.Empty(Read(codec, stripped).Values);
        Assert.Equal(original, stripped);
    }

    [Theory]
    [MemberData(nameof(Pictures))]
    public void A_picture_named_wrongly_is_refused(string extension)
    {
        var (codec, original) = Picture(extension);

        Assert.Throws<MetadataFormatException>(() => Read(codec, []));
        Assert.Throws<MetadataFormatException>(() => Read(codec, "not a picture of any kind at all"u8.ToArray()));
        Assert.Throws<MetadataFormatException>(() => Read(codec, original[..(original.Length / 2)]));
    }

    [Fact]
    public void A_png_chunk_is_written_before_the_picture_data_with_a_true_checksum()
    {
        var written = Write(new PngCodec(), Convert.FromBase64String(MetadataFixtures.Png), Edit((MetadataField.Title, "Hello")));

        var exif = written.AsSpan().IndexOf("eXIf"u8);
        var data = written.AsSpan().IndexOf("IDAT"u8);
        Assert.True(exif > 0 && exif < data);

        // Checked with the framework's CRC-32, not the codec's own table.
        var length = System.Buffers.Binary.BinaryPrimitives.ReadInt32BigEndian(written.AsSpan(exif - 4));
        var expected = System.IO.Hashing.Crc32.HashToUInt32(written.AsSpan(exif, 4 + length));
        Assert.Equal(expected, System.Buffers.Binary.BinaryPrimitives.ReadUInt32BigEndian(written.AsSpan(exif + 4 + length)));
    }

    [Fact]
    public void A_simple_webp_gains_the_header_metadata_needs_and_loses_it_again()
    {
        var codec = new WebPCodec();
        var original = Convert.FromBase64String(MetadataFixtures.WebP);
        Assert.Equal(-1, original.AsSpan().IndexOf("VP8X"u8));

        var written = Write(codec, original, Edit((MetadataField.Title, "Hello")));

        // The extended header comes first, says there is EXIF, and states the one-pixel canvas.
        Assert.Equal("VP8X"u8.ToArray(), written[12..16]);
        Assert.Equal(0x08, written[20] & 0x0C);
        Assert.Equal(new byte[] { 0, 0, 0, 0, 0, 0 }, written[24..30]);
        Assert.Equal(written.Length - 8, BitConverter.ToInt32(written, 4));
        Assert.True(written.AsSpan().IndexOf("VP8L"u8) < written.AsSpan().IndexOf("EXIF"u8));
        Assert.Equal(0, written.Length % 2);
    }

    [Fact]
    public void Camera_settings_already_in_a_picture_are_read_and_changed_in_place()
    {
        var codec = new JpegCodec();
        var camera = MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg);

        var written = Write(codec, camera, Edit(
            (MetadataField.Iso, "400"), (MetadataField.Aperture, "5.6"), (MetadataField.ExposureTime, "1/60"),
            (MetadataField.FocalLength, "35"), (MetadataField.LensModel, "35mm F1.4")));
        var doc = Read(codec, written);

        Assert.Equal("400", doc.Get(MetadataField.Iso));
        Assert.Equal("5.6", doc.Get(MetadataField.Aperture));
        Assert.Equal("1/60", doc.Get(MetadataField.ExposureTime));
        Assert.Equal("35", doc.Get(MetadataField.FocalLength));
        Assert.Equal("35mm F1.4", doc.Get(MetadataField.LensModel));

        // The note that must not move has not, with five tags added to the directory it lives in.
        Assert.Equal(camera.AsSpan().IndexOf(MetadataFixtures.MakerNote), written.AsSpan().IndexOf(MetadataFixtures.MakerNote));
        Assert.Equal("TestCam", doc.Get(MetadataField.CameraMake));
    }

    [Fact]
    public void An_iptc_record_that_states_a_field_changes_with_it_or_falls_silent()
    {
        static byte[] Dataset(byte number, string text) =>
            [0x1C, 2, number, 0, (byte)text.Length, .. Encoding.Latin1.GetBytes(text)];

        // Laid out by hand from the IIM and Photoshop resource specifications: a title, two
        // keywords, a caption, a city this app has no field for, and Photoshop's checksum of the lot.
        byte[] record = [.. Dataset(5, "Stale title"), .. Dataset(25, "old"), .. Dataset(25, "older"),
            .. Dataset(120, "Stale caption"), .. Dataset(90, "Lisbon")];
        byte[] resources =
        [
            .. "Photoshop 3.0\0"u8,
            .. "8BIM"u8, 0x04, 0x04, 0, 0, 0, 0, 0, (byte)record.Length, .. record, .. (record.Length % 2 == 0 ? Array.Empty<byte>() : [(byte)0]),
            .. "8BIM"u8, 0x04, 0x25, 0, 0, 0, 0, 0, 16, .. new byte[16],
        ];

        var jpeg = MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg);
        var length = resources.Length + 2;
        byte[] original = [.. jpeg[..20], 0xFF, 0xED, (byte)(length >> 8), (byte)length, .. resources, .. jpeg[20..]];

        var codec = new JpegCodec();
        var written = Write(codec, original, Edit((MetadataField.Title, "Fresh title"), (MetadataField.Keywords, "sea; boats")));
        var text = Encoding.Latin1.GetString(written);

        Assert.DoesNotContain("Stale title", text);
        Assert.DoesNotContain("Stale caption", text);
        Assert.DoesNotContain("older", text);
        Assert.Contains("\u001C\u0002\u0005\0\u000BFresh title", text);
        Assert.Contains("\u001C\u0002x\0\u000BFresh title", text);
        Assert.Contains("\u001C\u0002\u0019\0\u0003sea", text);
        Assert.Contains("\u001C\u0002\u0019\0\u0005boats", text);
        Assert.Contains("Lisbon", text);

        // The checksum of the old record is gone rather than left saying the record is out of date.
        Assert.Equal(-1, written.AsSpan().IndexOf((ReadOnlySpan<byte>)[0x38, 0x42, 0x49, 0x4D, 0x04, 0x25]));
        Assert.Equal(Digest(codec, original), Digest(codec, written));

        // A title this record's character set cannot spell is taken out of it, not misspelt.
        var unspellable = Write(codec, original, Edit((MetadataField.Title, "日本")));
        Assert.Equal("日本", Read(codec, unspellable).Get(MetadataField.Title));
        var after = Encoding.Latin1.GetString(unspellable);
        Assert.DoesNotContain("Stale title", after);
        Assert.DoesNotContain("Stale caption", after);
        Assert.DoesNotContain("??", after);
        Assert.Contains("\u001C\u0002\u0019\0\u0005older", after);
        Assert.Contains("Lisbon", after);
    }

    [Fact]
    public void A_tiff_is_edited_without_moving_a_pixel()
    {
        var codec = new TiffCodec();
        var original = Convert.FromBase64String(MetadataFixtures.Tiff);
        var doc = Read(codec, original);

        Assert.Equal("Old tiff title", doc.Get(MetadataField.Title));
        Assert.Equal("ScanCo", doc.Get(MetadataField.CameraMake));
        Assert.Equal("2020-01-02 03:04:05", doc.Get(MetadataField.DateTaken));
        Assert.True(doc.HasLocation);

        var edit = Everything(codec);
        var written = Write(codec, original, edit);

        foreach (var (field, value) in edit.Changes) Assert.Equal(value, Read(codec, written).Get(field));
        Assert.Equal(Digest(codec, original), Digest(codec, written));

        // The strip of pixels is the first thing in the file after its header, and still is:
        // everything new went on the end.
        Assert.Equal(original[8..200], written[8..200]);
        Assert.True(written.Length > original.Length);

        var unlocated = Write(codec, written, MetadataEdit.None with { RemoveLocation = true });
        Assert.False(Read(codec, unlocated).HasLocation);
        Assert.Equal(edit.Changes[MetadataField.Title], Read(codec, unlocated).Get(MetadataField.Title));

        var stripped = Write(codec, written, MetadataEdit.None with { RemoveAll = true });
        Assert.Empty(Read(codec, stripped).Values);
        Assert.False(Read(codec, stripped).HasLocation);
        Assert.Equal(-1, stripped.AsSpan().IndexOf("ScanCo"u8));
        Assert.Equal(-1, stripped.AsSpan().IndexOf(Encoding.UTF8.GetBytes("2024:03:14")));
        Assert.Equal(Digest(codec, original), Digest(codec, stripped));
    }

    [Fact]
    public void A_tiff_whose_pixels_or_shape_changed_does_not_pass_for_untouched()
    {
        var codec = new TiffCodec();
        var original = Convert.FromBase64String(MetadataFixtures.Tiff);

        var pixel = (byte[])original.Clone();
        pixel[40] ^= 0xFF;
        Assert.NotEqual(Digest(codec, original), Digest(codec, pixel));

        // The width, in the directory: same pixels, a different picture.
        var width = original.AsSpan().IndexOf((ReadOnlySpan<byte>)[0x00, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08]);
        Assert.True(width > 0);
        var reshaped = (byte[])original.Clone();
        reshaped[width + 8] = 4;
        Assert.NotEqual(Digest(codec, original), Digest(codec, reshaped));

        Assert.Throws<MetadataFormatException>(() => Read(codec, original[..6]));
        Assert.Throws<MetadataFormatException>(() => Read(codec, [(byte)'I', (byte)'I', 43, 0, 8, 0, 0, 0, 0, 0, 0, 0]));
    }

    // ---- audio ----------------------------------------------------------------------------------

    public static TheoryData<string> Containers => [".mp3", ".flac", ".wav", ".m4a", ".mp4", ".wma", ".aiff", ".opus"];

    private static byte[] Audio(string extension) => extension switch
    {
        ".mp3" => MetadataFixtures.Audio(),
        ".flac" => MetadataFixtures.Bytes(MetadataFixtures.Flac),
        ".wav" => MetadataFixtures.Wav(),
        ".m4a" => MetadataFixtures.Bytes(MetadataFixtures.M4a),
        ".mp4" => MetadataFixtures.Bytes(MetadataFixtures.Mp4Video),
        ".wma" => MetadataFixtures.Bytes(MetadataFixtures.Wma),
        ".aiff" => MetadataFixtures.Aiff(),
        ".opus" => MetadataFixtures.Opus(),
        _ => throw new ArgumentOutOfRangeException(nameof(extension)),
    };

    [Fact]
    public void An_ogg_stream_is_judged_by_its_packets_and_its_timing_not_its_pages()
    {
        var codec = new AudioTagCodec(".opus");
        var original = MetadataFixtures.Opus();

        // A tag edit re-cuts and renumbers the pages; the digest has to see through that...
        var tagged = Write(codec, original, Edit((MetadataField.Title, new string((char)0x58, 3000))));
        Assert.Equal(Digest(codec, original), Digest(codec, tagged));

        // ...and still notice one changed byte of sound, or a page that ends at the wrong time.
        var sound = original.AsSpan().LastIndexOf((ReadOnlySpan<byte>)[0xF8, 0xFF, 0xFE]);
        var deaf = (byte[])original.Clone();
        deaf[sound + 2] ^= 0x01;
        Assert.NotEqual(Digest(codec, original), Digest(codec, deaf));

        var lastPage = original.AsSpan().LastIndexOf("OggS"u8);
        var late = (byte[])original.Clone();
        late[lastPage + 6] ^= 0x10;
        Assert.NotEqual(Digest(codec, original), Digest(codec, late));

        // Two streams in one file is something this does not claim to understand.
        var second = (byte[])original.Clone();
        second[lastPage + 14] ^= 0xFF;
        Assert.Throws<MetadataFormatException>(() => Digest(codec, second));
    }

    [Theory]
    [MemberData(nameof(Containers))]
    public void Every_field_a_container_offers_is_one_it_keeps(string extension)
    {
        // The pin behind each codec's field set: a field offered here that a format silently
        // drops would fail the executor's read-back on every file of that kind.
        var codec = new AudioTagCodec(extension);
        var original = Audio(extension);

        var edit = Everything(codec);
        var doc = Read(codec, Write(codec, original, edit));

        foreach (var (field, value) in edit.Changes) Assert.Equal(value, doc.Get(field));
        Assert.Equal(Digest(codec, original), Digest(codec, Write(codec, original, edit)));
    }

    [Theory]
    [MemberData(nameof(Containers))]
    public void A_cover_picture_goes_in_comes_back_byte_for_byte_and_comes_out(string extension)
    {
        var codec = new AudioTagCodec(extension);
        var original = Audio(extension);
        var cover = MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg);
        Assert.True(codec.HoldsPicture);
        Assert.Null(Read(codec, original).Picture);

        var covered = Write(codec, original, MetadataEdit.None with { SetPicture = cover });
        Assert.Equal(cover, Read(codec, covered).Picture);
        Assert.Equal(Digest(codec, original), Digest(codec, covered));

        // Replaced, not added to.
        var other = Convert.FromBase64String(MetadataFixtures.Png);
        var replaced = Write(codec, covered, MetadataEdit.None with { SetPicture = other });
        Assert.Equal(other, Read(codec, replaced).Picture);

        var bare = Write(codec, replaced, MetadataEdit.None with { RemovePicture = true });
        Assert.Null(Read(codec, bare).Picture);
        Assert.Equal(Digest(codec, original), Digest(codec, bare));
    }

    [Fact]
    public void A_field_one_container_has_no_room_for_is_not_offered_for_it()
    {
        Assert.DoesNotContain(MetadataField.Publisher, new AudioTagCodec(".wma").Fields);
        Assert.Contains(MetadataField.Publisher, new AudioTagCodec(".mp3").Fields);

        // And an edit is cut down to what the file can take before it is ever planned.
        var edit = Edit((MetadataField.Publisher, "P"), (MetadataField.Title, "T"), (MetadataField.Iso, "100"));
        Assert.Equal([MetadataField.Title], edit.For(new AudioTagCodec(".wma")).Changes.Keys);
        Assert.Equal([MetadataField.Title, MetadataField.Iso], edit.For(new JpegCodec()).Changes.Keys.Order());
    }

    // ---- documents ------------------------------------------------------------------------------

    private static string Entry(byte[] zip, string name)
    {
        using var archive = new ZipArchive(new MemoryStream(zip), ZipArchiveMode.Read);
        using var reader = new StreamReader(archive.GetEntry(name)!.Open(), Encoding.UTF8);
        return reader.ReadToEnd();
    }

    [Fact]
    public void A_word_document_reads_and_takes_its_properties_and_nothing_else_changes()
    {
        var codec = new OpenXmlCodec();
        var original = MetadataFixtures.Docx();
        var doc = Read(codec, original);

        Assert.Equal("Quarterly report", doc.Get(MetadataField.Title));
        Assert.Equal("A. Writer", doc.Get(MetadataField.Author));
        Assert.Equal("finance; q3", doc.Get(MetadataField.Keywords));

        var edit = Everything(codec);
        var written = Write(codec, original, edit);

        foreach (var (field, value) in edit.Changes) Assert.Equal(value, Read(codec, written).Get(field));
        Assert.Equal(Digest(codec, original), Digest(codec, written));
        Assert.Equal(Entry(original, "word/document.xml"), Entry(written, "word/document.xml"));

        // What this app has no field for is still in the part.
        var core = Entry(written, "docProps/core.xml");
        Assert.Contains("<cp:revision>4</cp:revision>", core);
        Assert.Contains("2024-03-14T09:26:53Z", core);
    }

    [Fact]
    public void Removing_everything_from_a_document_also_forgets_who_saved_it()
    {
        var codec = new OpenXmlCodec();
        var written = Write(codec, MetadataFixtures.Docx(), MetadataEdit.None with { RemoveAll = true });

        Assert.Empty(Read(codec, written).Values);
        Assert.DoesNotContain("A. Writer", Entry(written, "docProps/core.xml"));
        Assert.Contains("The body of the document", Entry(written, "word/document.xml"));
    }

    [Fact]
    public void A_book_reads_and_takes_its_metadata_and_stays_a_book()
    {
        var codec = new EpubCodec();
        var original = MetadataFixtures.Epub();
        var doc = Read(codec, original);

        Assert.Equal("A Short Book", doc.Get(MetadataField.Title));
        Assert.Equal("Some Author", doc.Get(MetadataField.Author));
        Assert.Equal("Fiction; Short stories", doc.Get(MetadataField.Keywords));

        var edit = Everything(codec);
        var written = Write(codec, original, edit);

        foreach (var (field, value) in edit.Changes) Assert.Equal(value, Read(codec, written).Get(field));
        Assert.Equal(Digest(codec, original), Digest(codec, written));
        Assert.Equal(Entry(original, "book/chapter1.xhtml"), Entry(written, "book/chapter1.xhtml"));

        // The type marker is still the first entry and still stored, which is how a reader that
        // sniffs the first bytes of the file knows what it is holding.
        Assert.Equal("mimetypeapplication/epub+zip", Encoding.ASCII.GetString(written, 30, 28));
        using var archive = new ZipArchive(new MemoryStream(written), ZipArchiveMode.Read);
        Assert.Equal("mimetype", archive.Entries[0].FullName);
        Assert.Equal(archive.Entries[0].Length, archive.Entries[0].CompressedLength);

        var package = Entry(written, "book/package.opf");
        Assert.Contains("<dc:language>en</dc:language>", package);
        Assert.Contains("<itemref idref=\"c1\"", package);
    }

    [Fact]
    public void A_book_whose_reading_order_changed_does_not_pass_for_untouched()
    {
        var codec = new EpubCodec();
        var original = MetadataFixtures.Epub();

        using var buffer = new MemoryStream();
        using (var source = new ZipArchive(new MemoryStream(original), ZipArchiveMode.Read))
        using (var target = new ZipArchive(buffer, ZipArchiveMode.Create, leaveOpen: true))
        {
            foreach (var entry in source.Entries)
            {
                using var reader = new StreamReader(entry.Open(), Encoding.UTF8);
                var text = reader.ReadToEnd().Replace("<itemref idref=\"c1\"/>", "");
                using var writer = target.CreateEntry(entry.FullName).Open();
                writer.Write(Encoding.UTF8.GetBytes(text));
            }
        }

        Assert.NotEqual(Digest(codec, original), Digest(codec, buffer.ToArray()));
    }

    [Fact]
    public void Something_that_is_not_the_document_its_name_says_is_refused()
    {
        Assert.Throws<MetadataFormatException>(() => Read(new OpenXmlCodec(), "an old binary .doc, or a protected file"u8.ToArray()));
        Assert.Throws<MetadataFormatException>(() => Read(new OpenXmlCodec(), MetadataFixtures.Epub()));
        Assert.Throws<MetadataFormatException>(() => Read(new EpubCodec(), MetadataFixtures.Docx()));
        Assert.Throws<MetadataFormatException>(() => Read(new EpubCodec(), []));
    }

    [Fact]
    public void Every_extension_has_a_codec_and_every_codec_offers_only_real_fields()
    {
        foreach (var extension in MetadataCodecs.Extensions)
        {
            var codec = MetadataCodecs.For("x" + extension);
            Assert.NotNull(codec);
            Assert.NotEmpty(codec.Fields);
            Assert.All(codec.Fields, f => Assert.NotNull(MetadataFields.Get(f)));
        }

        Assert.IsType<PngCodec>(MetadataCodecs.For(@"C:\x\a.PNG"));
        Assert.IsType<WebPCodec>(MetadataCodecs.For(@"C:\x\a.webp"));
        Assert.IsType<OpenXmlCodec>(MetadataCodecs.For(@"C:\x\a.docx"));
        Assert.IsType<OpenXmlCodec>(MetadataCodecs.For(@"C:\x\a.xlsm"));
        Assert.IsType<EpubCodec>(MetadataCodecs.For(@"C:\x\a.epub"));
    }
}
