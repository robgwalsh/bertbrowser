using BertBrowser.Core.Services.Metadata;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The codecs against files they did not write: pictures from Windows' own encoder and MP3 tags
/// laid out by hand from the specification.
/// </summary>
public class MetadataCodecTests
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

    private static int IndexOf(byte[] haystack, byte[] needle) => haystack.AsSpan().IndexOf(needle);

    // ---- fields ---------------------------------------------------------------------------------

    [Theory]
    [InlineData(MetadataField.TrackNumber, "07", "7")]
    [InlineData(MetadataField.Title, "  Spaced  ", "Spaced")]
    [InlineData(MetadataField.Keywords, "a;b ; a;;c", "a; b; c")]
    [InlineData(MetadataField.Rating, "0", "")]
    [InlineData(MetadataField.Rating, "5", "5")]
    [InlineData(MetadataField.DateTaken, "2024-03-14", "2024-03-14 00:00:00")]
    [InlineData(MetadataField.DateTaken, "2024:03:14 09:26:53", "2024-03-14 09:26:53")]
    [InlineData(MetadataField.Year, "", "")]
    public void A_typed_value_becomes_its_canonical_form(MetadataField field, string typed, string expected)
    {
        Assert.True(MetadataFields.TryNormalize(field, typed, out var value));
        Assert.Equal(expected, value);
    }

    [Theory]
    [InlineData(MetadataField.TrackNumber, "seven")]
    [InlineData(MetadataField.TrackNumber, "-1")]
    [InlineData(MetadataField.Rating, "6")]
    [InlineData(MetadataField.DateTaken, "last tuesday")]
    [InlineData(MetadataField.Title, "nul\0inside")]
    public void A_value_of_the_wrong_kind_is_refused(MetadataField field, string typed) =>
        Assert.False(MetadataFields.TryNormalize(field, typed, out _));

    [Fact]
    public void Field_ids_are_unique_and_every_field_has_a_row()
    {
        Assert.Equal(MetadataFields.All.Count, MetadataFields.All.Select(s => s.Id).Distinct().Count());
        foreach (var field in Enum.GetValues<MetadataField>()) Assert.NotNull(MetadataFields.Get(field));
    }

    // ---- JPEG -----------------------------------------------------------------------------------

    [Fact]
    public void A_camera_picture_reads_what_the_camera_wrote()
    {
        var doc = Read(new JpegCodec(), MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg));

        Assert.Equal("Old title", doc.Get(MetadataField.Title));
        Assert.Equal("TestCam", doc.Get(MetadataField.CameraMake));
        Assert.Equal("Model One", doc.Get(MetadataField.CameraModel));
        Assert.Equal("2021-06-05 04:03:02", doc.Get(MetadataField.DateTaken));
        Assert.Equal("", doc.Get(MetadataField.Author));
    }

    [Fact]
    public void A_picture_with_no_exif_reads_as_empty_and_can_be_given_some()
    {
        var codec = new JpegCodec();
        var plain = MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg);
        Assert.Empty(Read(codec, plain).Values);

        var written = Write(codec, plain, Edit((MetadataField.Title, "Hello"), (MetadataField.DateTaken, "2024-03-14 09:26:53")));
        var doc = Read(codec, written);

        Assert.Equal("Hello", doc.Get(MetadataField.Title));
        Assert.Equal("2024-03-14 09:26:53", doc.Get(MetadataField.DateTaken));
        Assert.Equal(Digest(codec, plain), Digest(codec, written));

        // After the JFIF header, where readers expect it, and the scan untouched behind it.
        Assert.Equal([0xFF, 0xD8, 0xFF, 0xE0], written[..4]);
        Assert.Equal([0xFF, 0xE1], written[20..22]);
        Assert.Equal(plain[20..], written[^(plain.Length - 20)..]);
    }

    [Fact]
    public void Every_picture_field_round_trips_including_text_that_is_not_ascii()
    {
        var codec = new JpegCodec();
        var edit = Edit(
            (MetadataField.Title, "Café au lait — 日本"),
            (MetadataField.Author, "Renée"),
            (MetadataField.Subject, "Breakfast"),
            (MetadataField.Keywords, "food; coffee; paris"),
            (MetadataField.Rating, "4"),
            (MetadataField.Comment, "Taken before the rain"),
            (MetadataField.Copyright, "© 2024 Renée"),
            (MetadataField.CameraMake, "Another"),
            (MetadataField.CameraModel, "Thing"),
            (MetadataField.DateTaken, "2024-03-14 09:26:53"));

        foreach (var fixture in new[] { MetadataFixtures.CameraJpeg, MetadataFixtures.PlainJpeg })
        {
            var original = MetadataFixtures.Jpeg(fixture);
            var written = Write(codec, original, edit);
            var doc = Read(codec, written);

            foreach (var (field, value) in edit.Changes) Assert.Equal(value, doc.Get(field));
            Assert.Equal(Digest(codec, original), Digest(codec, written));
        }
    }

    [Fact]
    public void An_edit_never_moves_the_makernote()
    {
        var codec = new JpegCodec();
        var original = MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg);
        var written = Write(codec, original, Edit(
            (MetadataField.Title, "A much longer title than the one that was there before"),
            (MetadataField.Author, "Somebody"),
            (MetadataField.Comment, "And a comment, which adds a tag to the directory the note lives in")));

        // Its offsets are measured from the start of the EXIF block, which starts at the same
        // place in both files — so the same index in the file is the same offset in the block.
        var before = IndexOf(original, MetadataFixtures.MakerNote);
        Assert.True(before > 0);
        Assert.Equal(before, IndexOf(written, MetadataFixtures.MakerNote));

        // And what it did not touch is still read: the camera's own fields.
        var doc = Read(codec, written);
        Assert.Equal("TestCam", doc.Get(MetadataField.CameraMake));
        Assert.Equal("2021-06-05 04:03:02", doc.Get(MetadataField.DateTaken));
    }

    [Fact]
    public void A_cleared_field_is_gone_from_the_bytes_not_just_from_the_directory()
    {
        var codec = new JpegCodec();
        var original = MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg);
        Assert.True(IndexOf(original, "Old title"u8.ToArray()) > 0);

        var written = Write(codec, original, Edit((MetadataField.Title, "")));

        Assert.Equal("", Read(codec, written).Get(MetadataField.Title));
        Assert.Equal(-1, IndexOf(written, "Old title"u8.ToArray()));
        Assert.Equal("TestCam", Read(codec, written).Get(MetadataField.CameraMake));
    }

    [Fact]
    public void Editing_the_same_field_again_reuses_its_room()
    {
        var codec = new JpegCodec();
        var first = Write(codec, MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg), Edit((MetadataField.Title, "First title")));
        var second = Write(codec, first, Edit((MetadataField.Title, "Other title")));
        var third = Write(codec, second, Edit((MetadataField.Title, "Third")));

        Assert.Equal(first.Length, second.Length);
        Assert.Equal(first.Length, third.Length);
        Assert.Equal("Third", Read(codec, third).Get(MetadataField.Title));
    }

    [Fact]
    public void A_title_too_long_for_the_segment_is_refused()
    {
        var huge = new string('x', 70_000);
        Assert.Throws<MetadataFormatException>(() =>
            Write(new JpegCodec(), MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg), Edit((MetadataField.Title, huge))));
    }

    [Fact]
    public void Anything_that_is_not_a_jpeg_is_refused_rather_than_guessed_at()
    {
        var codec = new JpegCodec();
        var real = MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg);

        Assert.Throws<MetadataFormatException>(() => Read(codec, []));
        Assert.Throws<MetadataFormatException>(() => Read(codec, "not a picture"u8.ToArray()));
        Assert.Throws<MetadataFormatException>(() => Read(codec, real[..40]));
        Assert.Throws<MetadataFormatException>(() => Read(codec, [0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01]));
    }

    [Fact]
    public void A_damaged_or_hostile_exif_block_never_hangs_or_escapes()
    {
        var codec = new JpegCodec();
        var real = MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg);
        var exifStart = IndexOf(real, "Exif\0\0"u8.ToArray()) + 6;

        // Every single-byte corruption of the block's first two hundred bytes: each either reads
        // or is refused by name. Offsets pointing at themselves, counts of 65,535, types that do
        // not exist — all of them are in here somewhere.
        for (var at = 0; at < 200; at++)
        {
            foreach (byte poison in new byte[] { 0x00, 0xFF, 0x08 })
            {
                var damaged = (byte[])real.Clone();
                damaged[exifStart + at] = poison;

                try
                {
                    Read(codec, damaged);
                    Write(codec, damaged, Edit((MetadataField.Title, "x")));
                }
                catch (MetadataFormatException)
                {
                }
            }
        }
    }

    // ---- removing ------------------------------------------------------------------------------

    private static MetadataEdit RemoveLocation => MetadataEdit.None with { RemoveLocation = true };

    private static MetadataEdit RemoveAll => MetadataEdit.None with { RemoveAll = true };

    /// <summary>A JPEG with one more APP1 segment spliced in after its JFIF header.</summary>
    private static byte[] WithSegment(byte[] jpeg, ReadOnlySpan<byte> label, ReadOnlySpan<byte> body)
    {
        var length = label.Length + body.Length + 2;
        return [.. jpeg[..20], 0xFF, 0xE1, (byte)(length >> 8), (byte)length, .. label, .. body, .. jpeg[20..]];
    }

    private static byte[] WithXmp(byte[] jpeg, string packet) =>
        WithSegment(jpeg, "http://ns.adobe.com/xap/1.0/\0"u8, System.Text.Encoding.UTF8.GetBytes(packet));

    private const string XmpWithLocation =
        "<?xpacket begin=\"\uFEFF\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>" +
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">" +
        "<rdf:Description rdf:about=\"\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" " +
        "exif:GPSLatitude=\"51,30.4N\" exif:GPSLongitude=\"0,7.5W\" exif:ExposureTime=\"1/60\">" +
        "<exif:GPSAltitude>35/1</exif:GPSAltitude><dc:creator><rdf:Seq><rdf:li>Renée</rdf:li></rdf:Seq></dc:creator>" +
        "</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>";

    [Fact]
    public void A_location_is_reported_and_removing_it_takes_only_the_location()
    {
        var codec = new JpegCodec();
        var original = MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg);
        Assert.True(Read(codec, original).HasLocation);
        Assert.False(Read(codec, MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg)).HasLocation);

        var written = Write(codec, original, RemoveLocation);
        var doc = Read(codec, written);

        Assert.False(doc.HasLocation);
        Assert.Equal("Old title", doc.Get(MetadataField.Title));
        Assert.Equal("2021-06-05 04:03:02", doc.Get(MetadataField.DateTaken));
        Assert.Equal(IndexOf(original, MetadataFixtures.MakerNote), IndexOf(written, MetadataFixtures.MakerNote));
        Assert.Equal(Digest(codec, original), Digest(codec, written));

        // Same length: the directory was zeroed where it stood, not cut out, so nothing moved.
        Assert.Equal(original.Length, written.Length);
    }

    [Fact]
    public void A_location_written_in_xmp_as_well_is_removed_from_both()
    {
        var codec = new JpegCodec();
        var original = WithXmp(MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg), XmpWithLocation);
        Assert.True(Read(codec, original).HasLocation);

        var written = Write(codec, original, RemoveLocation);
        var text = System.Text.Encoding.UTF8.GetString(written);

        Assert.False(Read(codec, written).HasLocation);
        Assert.DoesNotContain("GPS", text);
        Assert.DoesNotContain("51,30.4N", text);

        // What was beside it in the packet is still there.
        Assert.Contains("exif:ExposureTime=\"1/60\"", text);
        Assert.Contains("Renée", text);
        Assert.Contains("<?xpacket end=\"w\"?>", text);
        Assert.Equal(Digest(codec, original), Digest(codec, written));
    }

    [Fact]
    public void A_packet_that_states_a_field_changes_with_it_and_one_is_never_invented()
    {
        var codec = new JpegCodec();
        const string packet =
            "<?xpacket begin=\"﻿\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>" +
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">" +
            "<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" " +
            "xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\" xmp:Rating=\"2\" lr:private=\"kept\">" +
            "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Stale title</rdf:li></rdf:Alt></dc:title>" +
            "<dc:subject><rdf:Bag><rdf:li>old</rdf:li></rdf:Bag></dc:subject>" +
            "<dc:creator><rdf:Seq><rdf:li>Somebody</rdf:li></rdf:Seq></dc:creator>" +
            "</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>";

        var original = WithXmp(MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg), packet);
        var written = Write(codec, original, Edit(
            (MetadataField.Title, "Fresh title"), (MetadataField.Keywords, "sea; boats"), (MetadataField.Rating, "")));
        var text = System.Text.Encoding.UTF8.GetString(written);

        Assert.DoesNotContain("Stale title", text);
        Assert.Contains("<rdf:li xml:lang=\"x-default\">Fresh title</rdf:li>", text);
        Assert.Contains("<rdf:li>sea</rdf:li><rdf:li>boats</rdf:li>", text);
        Assert.DoesNotContain("<rdf:li>old</rdf:li>", text);
        Assert.DoesNotContain("xmp:Rating", text);

        // What was not edited, and what this app has never heard of, is exactly as it was.
        Assert.Contains("<rdf:li>Somebody</rdf:li>", text);
        Assert.Contains("lr:private=\"kept\"", text);
        Assert.DoesNotContain("dc:description", text);
        Assert.Equal(Digest(codec, original), Digest(codec, written));

        // And a picture with no packet does not get one.
        var bare = Write(codec, MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg), Edit((MetadataField.Title, "Fresh title")));
        Assert.Equal(-1, IndexOf(bare, "xmpmeta"u8.ToArray()));
    }

    [Fact]
    public void A_location_that_cannot_be_shown_to_be_gone_is_refused_not_claimed()
    {
        var codec = new JpegCodec();
        var plain = MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg);

        // Not XML at all, but it says GPS: the safe reading is that a location is in there.
        var broken = WithXmp(plain, "<x:xmpmeta exif:GPSLatitude=\"51,30.4N\" <<<");
        Assert.True(Read(codec, broken).HasLocation);
        Assert.Throws<MetadataFormatException>(() => Write(codec, broken, RemoveLocation));

        // The overflow segments of a large packet are fragments, never parsed.
        var extended = WithSegment(plain, "http://ns.adobe.com/xmp/extension/\0"u8, "…exif:GPSLatitude…"u8);
        Assert.True(Read(codec, extended).HasLocation);
        Assert.Throws<MetadataFormatException>(() => Write(codec, extended, RemoveLocation));

        // A DTD is how hostile XML reaches outside itself. It is never processed.
        var hostile = WithXmp(plain, "<!DOCTYPE x [<!ENTITY e SYSTEM \"file:///c:/windows/win.ini\">]><x>GPS &e;</x>");
        Assert.True(Read(codec, hostile).HasLocation);
        Assert.Throws<MetadataFormatException>(() => Write(codec, hostile, RemoveLocation));
    }

    [Fact]
    public void Removing_everything_leaves_the_picture_and_how_it_is_drawn()
    {
        var codec = new JpegCodec();
        var camera = MetadataFixtures.Jpeg(MetadataFixtures.CameraJpeg);

        // Give it an orientation, an XMP packet and a comment segment to lose or keep.
        var sideways = Write(codec, camera, MetadataEdit.None);
        var block = ExifBlock.Parse(sideways.AsSpan(IndexOf(sideways, "Exif\0\0"u8.ToArray()) + 6, 0xFE - 6));
        ExifFields.SetOrientation(block, 6);
        var exif = block.ToArray();
        var original = WithXmp(
            [.. camera[..20], 0xFF, 0xE1, (byte)((exif.Length + 8) >> 8), (byte)(exif.Length + 8), .. "Exif\0\0"u8, .. exif,
                0xFF, 0xFE, 0x00, 0x09, .. "private"u8, .. camera[(20 + 2 + 0xFE)..]],
            XmpWithLocation);

        Assert.Equal("TestCam", Read(codec, original).Get(MetadataField.CameraMake));

        var written = Write(codec, original, RemoveAll);
        var doc = Read(codec, written);

        Assert.Empty(doc.Values);
        Assert.False(doc.HasLocation);
        Assert.Equal(-1, IndexOf(written, MetadataFixtures.MakerNote));
        Assert.Equal(-1, IndexOf(written, "TestCam"u8.ToArray()));
        Assert.Equal(-1, IndexOf(written, "xmpmeta"u8.ToArray()));
        Assert.Equal(-1, IndexOf(written, "private"u8.ToArray()));
        Assert.Equal(Digest(codec, original), Digest(codec, written));

        // Still the right way up: orientation is how the picture is drawn, not what it says.
        var kept = ExifBlock.Parse(written.AsSpan(IndexOf(written, "Exif\0\0"u8.ToArray()) + 6, 26));
        Assert.Equal((ushort)6, ExifFields.Orientation(kept));
    }

    [Fact]
    public void Removing_everything_from_a_picture_with_nothing_adds_nothing()
    {
        var codec = new JpegCodec();
        var plain = MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg);

        Assert.Equal(plain, Write(codec, plain, RemoveAll));
    }

    [Fact]
    public void Removing_everything_from_a_song_takes_every_tag_at_both_ends()
    {
        var original = MetadataFixtures.Mp3([("TIT2", "Song"), ("TPE1", "Artist")], withV1: true);
        var written = Write(Mp3, original, RemoveAll);

        Assert.Empty(Read(Mp3, written).Values);
        Assert.Equal(-1, IndexOf(written, "Old v1 title"u8.ToArray()));
        Assert.Equal(-1, IndexOf(written, "Artist"u8.ToArray()));
        Assert.Equal(Digest(Mp3, original), Digest(Mp3, written));

        // And a field set in the same edit is the only thing left.
        var retitled = Write(Mp3, original, RemoveAll with { Changes = Edit((MetadataField.Title, "Only this")).Changes });
        Assert.Equal([MetadataField.Title], Read(Mp3, retitled).Values.Keys);
    }

    // ---- MP3 ------------------------------------------------------------------------------------

    private static readonly AudioTagCodec Mp3 = new(".mp3");

    [Fact]
    public void An_mp3_reads_the_tag_somebody_else_wrote()
    {
        var file = MetadataFixtures.Mp3([("TIT2", "Song"), ("TPE1", "Artist"), ("TALB", "Album"), ("TRCK", "3/12"), ("TYER", "1999")]);
        var doc = Read(Mp3, file);

        Assert.Equal("Song", doc.Get(MetadataField.Title));
        Assert.Equal("Artist", doc.Get(MetadataField.Artist));
        Assert.Equal("Album", doc.Get(MetadataField.Album));
        Assert.Equal("3", doc.Get(MetadataField.TrackNumber));
        Assert.Equal("12", doc.Get(MetadataField.TrackTotal));
        Assert.Equal("1999", doc.Get(MetadataField.Year));
    }

    [Fact]
    public void The_oldest_tag_layout_is_read_too()
    {
        var doc = Read(Mp3, MetadataFixtures.Mp3V22("Old song", "Old artist"));

        Assert.Equal("Old song", doc.Get(MetadataField.Title));
        Assert.Equal("Old artist", doc.Get(MetadataField.Artist));
    }

    [Fact]
    public void Every_audio_field_round_trips_and_the_sound_is_untouched()
    {
        var edit = Edit(
            (MetadataField.Title, "Наша песня"),
            (MetadataField.Artist, "Björk"),
            (MetadataField.AlbumArtist, "Various"),
            (MetadataField.Album, "Homogenic"),
            (MetadataField.TrackNumber, "7"),
            (MetadataField.TrackTotal, "10"),
            (MetadataField.DiscNumber, "1"),
            (MetadataField.DiscTotal, "2"),
            (MetadataField.Year, "1997"),
            (MetadataField.Genre, "Electronic"),
            (MetadataField.Composer, "Somebody"),
            (MetadataField.Comment, "A comment"),
            (MetadataField.Copyright, "One Little Indian"));

        byte[][] originals =
        [
            MetadataFixtures.Mp3([("TIT2", "Song"), ("TPE1", "Artist")]),
            MetadataFixtures.Mp3([("TIT2", "Song")], withV1: true),
            MetadataFixtures.Mp3V22("Old song", "Old artist"),
            MetadataFixtures.Audio(),
        ];

        foreach (var original in originals)
        {
            var written = Write(Mp3, original, edit);
            var doc = Read(Mp3, written);

            foreach (var (field, value) in edit.Changes) Assert.Equal(value, doc.Get(field));
            Assert.Equal(Digest(Mp3, original), Digest(Mp3, written));
            Assert.True(IndexOf(written, MetadataFixtures.Audio()) > 0);
        }
    }

    [Fact]
    public void An_untouched_audio_field_is_left_alone_and_a_cleared_one_is_gone()
    {
        var original = MetadataFixtures.Mp3([("TIT2", "Song"), ("TPE1", "Artist"), ("TRCK", "3")]);
        var written = Write(Mp3, original, Edit((MetadataField.Artist, ""), (MetadataField.TrackNumber, "")));
        var doc = Read(Mp3, written);

        Assert.Equal("Song", doc.Get(MetadataField.Title));
        Assert.Equal("", doc.Get(MetadataField.Artist));
        Assert.Equal("", doc.Get(MetadataField.TrackNumber));
    }

    [Fact]
    public void Something_named_mp3_that_is_not_one_is_refused()
    {
        Assert.Throws<MetadataFormatException>(() => Read(Mp3, []));
        Assert.Throws<MetadataFormatException>(() => Read(Mp3, "this is a text file, not a song"u8.ToArray()));
        Assert.Throws<MetadataFormatException>(() => Read(Mp3, MetadataFixtures.Jpeg(MetadataFixtures.PlainJpeg)[..300]));
    }

    [Fact]
    public void The_payload_digest_ignores_tags_and_sees_the_sound()
    {
        var bare = MetadataFixtures.Audio();
        var tagged = MetadataFixtures.Mp3([("TIT2", "Song")], withV1: true);
        Assert.Equal(Digest(Mp3, bare), Digest(Mp3, tagged));

        var changed = (byte[])tagged.Clone();
        changed[IndexOf(changed, bare) + 1000] ^= 0xFF;
        Assert.NotEqual(Digest(Mp3, tagged), Digest(Mp3, changed));
    }

    // ---- the other containers ---------------------------------------------------------------

    public static TheoryData<string> Containers => [".flac", ".wav", ".m4a", ".mp4", ".wma"];

    private static byte[] Fixture(string extension) => extension switch
    {
        ".flac" => MetadataFixtures.Bytes(MetadataFixtures.Flac),
        ".wav" => MetadataFixtures.Wav(),
        ".m4a" => MetadataFixtures.Bytes(MetadataFixtures.M4a),
        ".mp4" => MetadataFixtures.Bytes(MetadataFixtures.Mp4Video),
        ".wma" => MetadataFixtures.Bytes(MetadataFixtures.Wma),
        _ => throw new ArgumentOutOfRangeException(nameof(extension)),
    };

    [Theory]
    [MemberData(nameof(Containers))]
    public void Every_container_takes_every_field_and_keeps_its_sound(string extension)
    {
        var codec = new AudioTagCodec(extension);
        var original = Fixture(extension);
        Assert.Empty(Read(codec, original).Values);

        var edit = Edit(
            (MetadataField.Title, "Наша песня"),
            (MetadataField.Artist, "Björk"),
            (MetadataField.AlbumArtist, "Various"),
            (MetadataField.Album, "Homogenic"),
            (MetadataField.TrackNumber, "7"),
            (MetadataField.TrackTotal, "10"),
            (MetadataField.DiscNumber, "1"),
            (MetadataField.DiscTotal, "2"),
            (MetadataField.Year, "1997"),
            (MetadataField.Genre, "Electronic"),
            (MetadataField.Composer, "Somebody"),
            (MetadataField.Comment, "A comment"),
            (MetadataField.Copyright, "One Little Indian"));

        var written = Write(codec, original, edit);
        var doc = Read(codec, written);

        foreach (var (field, value) in edit.Changes) Assert.Equal(value, doc.Get(field));
        Assert.Equal(Digest(codec, original), Digest(codec, written));

        // Edited again, and then emptied: the same sound each time.
        var again = Write(codec, written, Edit((MetadataField.Title, "Shorter"), (MetadataField.Comment, "")));
        Assert.Equal("Shorter", Read(codec, again).Get(MetadataField.Title));
        Assert.Equal("", Read(codec, again).Get(MetadataField.Comment));
        Assert.Equal("Björk", Read(codec, again).Get(MetadataField.Artist));
        Assert.Equal(Digest(codec, original), Digest(codec, again));

        var stripped = Write(codec, again, RemoveAll);
        Assert.Empty(Read(codec, stripped).Values);
        Assert.Equal(Digest(codec, original), Digest(codec, stripped));
    }

    [Theory]
    [MemberData(nameof(Containers))]
    public void A_container_named_wrongly_is_refused(string extension)
    {
        var codec = new AudioTagCodec(extension);

        Assert.Throws<MetadataFormatException>(() => Read(codec, []));
        Assert.Throws<MetadataFormatException>(() => Read(codec, "nothing like it at all, really"u8.ToArray()));
        Assert.Throws<MetadataFormatException>(() => Read(codec, MetadataFixtures.Audio()));
        Assert.Throws<MetadataFormatException>(() => Read(codec, Fixture(extension)[..40]));
    }

    [Theory]
    [InlineData(".m4a")]
    [InlineData(".mp4")]
    public void An_mpeg4_file_whose_tables_were_left_behind_does_not_pass_for_untouched(string extension)
    {
        // What a careless tag writer does: makes room near the top, pushes the media data down the
        // file, and forgets to move the offsets that say where each piece of it starts. Every byte
        // of sound and picture is still there and still the same — and nothing can play it.
        var codec = new AudioTagCodec(extension);
        var original = Fixture(extension);

        var media = IndexOf(original, "mdat"u8.ToArray()) - 4;
        Assert.True(media > 0);

        byte[] pushed = [.. original[..media], 0, 0, 0, 16, .. "free"u8, .. new byte[8], .. original[media..]];

        Assert.NotEqual(Digest(codec, original), Digest(codec, pushed));
    }

    [Fact]
    public void A_total_with_no_number_to_go_with_it_is_refused_by_name()
    {
        var ex = Assert.Throws<MetadataFormatException>(() =>
            Write(Mp3, MetadataFixtures.Audio(), Edit((MetadataField.TrackTotal, "12"))));
        Assert.Contains("track number", ex.Message);

        // With one already there, a total alone is fine.
        var numbered = MetadataFixtures.Mp3([("TRCK", "3")]);
        Assert.Equal("12", Read(Mp3, Write(Mp3, numbered, Edit((MetadataField.TrackTotal, "12")))).Get(MetadataField.TrackTotal));
    }

    [Fact]
    public void The_extension_decides_which_codec()
    {
        Assert.IsType<JpegCodec>(MetadataCodecs.For(@"C:\x\a.JPG"));
        Assert.IsType<AudioTagCodec>(MetadataCodecs.For(@"C:\x\a.mp3"));
        Assert.Null(MetadataCodecs.For(@"C:\x\a.txt"));
        Assert.Null(MetadataCodecs.For(@"C:\x\noextension"));
    }
}
