// Port of tests/BertBrowser.Core.Tests/MetadataCodecTests.cs. The codecs against files they did not
// write: pictures from Windows' own encoder and MP3 tags laid out by hand from the specification.
use std::io::Cursor;

use bb_core::cancel::CancelToken;
use bb_core::metadata::{
    AudioTagCodec, ExifBlock, ExifFields, JpegCodec, MetadataCodec, MetadataCodecs,
    MetadataDocument, MetadataEdit, MetadataError, MetadataField, MetadataFields, MetadataResult,
};
use bb_test_support::metadata_fixtures as fx;

fn edit(changes: &[(MetadataField, &str)]) -> MetadataEdit {
    MetadataEdit::of(changes)
}

fn try_write(
    codec: &dyn MetadataCodec,
    file: &[u8],
    edit: &MetadataEdit,
) -> MetadataResult<Vec<u8>> {
    let mut source = Cursor::new(file.to_vec());
    let mut destination = Cursor::new(Vec::new());
    codec.write(&mut source, &mut destination, edit, &CancelToken::none())?;
    Ok(destination.into_inner())
}

fn write(codec: &dyn MetadataCodec, file: &[u8], edit: &MetadataEdit) -> Vec<u8> {
    try_write(codec, file, edit).expect("write")
}

fn try_read(codec: &dyn MetadataCodec, file: &[u8]) -> MetadataResult<MetadataDocument> {
    codec.read(&mut Cursor::new(file.to_vec()))
}

fn read(codec: &dyn MetadataCodec, file: &[u8]) -> MetadataDocument {
    try_read(codec, file).expect("read")
}

fn digest(codec: &dyn MetadataCodec, file: &[u8]) -> Vec<u8> {
    codec
        .payload_digest(&mut Cursor::new(file.to_vec()))
        .expect("digest")
}

fn index_of(haystack: &[u8], needle: &[u8]) -> i64 {
    if needle.is_empty() || haystack.len() < needle.len() {
        return -1;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p as i64)
        .unwrap_or(-1)
}

fn is_format<T>(result: &MetadataResult<T>) -> bool {
    matches!(result, Err(e) if e.is_format())
}

fn camera() -> Vec<u8> {
    fx::jpeg(fx::CAMERA_JPEG)
}

fn plain_jpeg() -> Vec<u8> {
    fx::jpeg(fx::PLAIN_JPEG)
}

fn mp3() -> AudioTagCodec {
    AudioTagCodec::new(".mp3")
}

// ---- fields -----------------------------------------------------------------------------------

#[test]
fn a_typed_value_becomes_its_canonical_form() {
    let rows = [
        (MetadataField::TrackNumber, "07", "7"),
        (MetadataField::Title, "  Spaced  ", "Spaced"),
        (MetadataField::Keywords, "a;b ; a;;c", "a; b; c"),
        (MetadataField::Rating, "0", ""),
        (MetadataField::Rating, "5", "5"),
        (
            MetadataField::DateTaken,
            "2024-03-14",
            "2024-03-14 00:00:00",
        ),
        (
            MetadataField::DateTaken,
            "2024:03:14 09:26:53",
            "2024-03-14 09:26:53",
        ),
        (MetadataField::Year, "", ""),
    ];
    for (field, typed, expected) in rows {
        let value = MetadataFields::try_normalize(field, typed);
        assert_eq!(Some(expected.to_string()), value, "{field:?} {typed:?}");
    }
}

#[test]
fn a_value_of_the_wrong_kind_is_refused() {
    let rows = [
        (MetadataField::TrackNumber, "seven"),
        (MetadataField::TrackNumber, "-1"),
        (MetadataField::Rating, "6"),
        (MetadataField::DateTaken, "last tuesday"),
        (MetadataField::Title, "nul\0inside"),
    ];
    for (field, typed) in rows {
        assert!(
            MetadataFields::try_normalize(field, typed).is_none(),
            "{field:?} {typed:?}"
        );
    }
}

#[test]
fn field_ids_are_unique_and_every_field_has_a_row() {
    let all = MetadataFields::all();
    let mut ids: Vec<&str> = all.iter().map(|s| s.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(all.len(), ids.len());

    use MetadataField as F;
    let every = [
        F::Title,
        F::Artist,
        F::AlbumArtist,
        F::Album,
        F::TrackNumber,
        F::TrackTotal,
        F::DiscNumber,
        F::DiscTotal,
        F::Year,
        F::Genre,
        F::Composer,
        F::Comment,
        F::Copyright,
        F::Author,
        F::Subject,
        F::Keywords,
        F::Rating,
        F::DateTaken,
        F::CameraMake,
        F::CameraModel,
        F::LensModel,
        F::Iso,
        F::Aperture,
        F::ExposureTime,
        F::FocalLength,
        F::Publisher,
        F::Conductor,
        F::Bpm,
        F::Lyrics,
        F::Category,
    ];
    assert_eq!(every.len(), all.len());
    for field in every {
        assert_eq!(field, MetadataFields::get(field).field);
    }
}

// ---- JPEG -------------------------------------------------------------------------------------

#[test]
fn a_camera_picture_reads_what_the_camera_wrote() {
    let doc = read(&JpegCodec, &camera());

    assert_eq!("Old title", doc.get(MetadataField::Title));
    assert_eq!("TestCam", doc.get(MetadataField::CameraMake));
    assert_eq!("Model One", doc.get(MetadataField::CameraModel));
    assert_eq!("2021-06-05 04:03:02", doc.get(MetadataField::DateTaken));
    assert_eq!("", doc.get(MetadataField::Author));
}

#[test]
fn a_picture_with_no_exif_reads_as_empty_and_can_be_given_some() {
    let codec = JpegCodec;
    let plain = plain_jpeg();
    assert!(read(&codec, &plain).values.is_empty());

    let written = write(
        &codec,
        &plain,
        &edit(&[
            (MetadataField::Title, "Hello"),
            (MetadataField::DateTaken, "2024-03-14 09:26:53"),
        ]),
    );
    let doc = read(&codec, &written);

    assert_eq!("Hello", doc.get(MetadataField::Title));
    assert_eq!("2024-03-14 09:26:53", doc.get(MetadataField::DateTaken));
    assert_eq!(digest(&codec, &plain), digest(&codec, &written));

    // After the JFIF header, where readers expect it, and the scan untouched behind it.
    assert_eq!(&[0xFF, 0xD8, 0xFF, 0xE0], &written[..4]);
    assert_eq!(&[0xFF, 0xE1], &written[20..22]);
    assert_eq!(&plain[20..], &written[written.len() - (plain.len() - 20)..]);
}

#[test]
fn every_picture_field_round_trips_including_text_that_is_not_ascii() {
    let codec = JpegCodec;
    let e = edit(&[
        (MetadataField::Title, "Café au lait — 日本"),
        (MetadataField::Author, "Renée"),
        (MetadataField::Subject, "Breakfast"),
        (MetadataField::Keywords, "food; coffee; paris"),
        (MetadataField::Rating, "4"),
        (MetadataField::Comment, "Taken before the rain"),
        (MetadataField::Copyright, "© 2024 Renée"),
        (MetadataField::CameraMake, "Another"),
        (MetadataField::CameraModel, "Thing"),
        (MetadataField::DateTaken, "2024-03-14 09:26:53"),
    ]);

    for fixture in [fx::CAMERA_JPEG, fx::PLAIN_JPEG] {
        let original = fx::jpeg(fixture);
        let written = write(&codec, &original, &e);
        let doc = read(&codec, &written);

        for (field, value) in &e.changes {
            assert_eq!(value, doc.get(*field), "{field:?}");
        }
        assert_eq!(digest(&codec, &original), digest(&codec, &written));
    }
}

#[test]
fn an_edit_never_moves_the_makernote() {
    let codec = JpegCodec;
    let original = camera();
    let written = write(
        &codec,
        &original,
        &edit(&[
            (
                MetadataField::Title,
                "A much longer title than the one that was there before",
            ),
            (MetadataField::Author, "Somebody"),
            (
                MetadataField::Comment,
                "And a comment, which adds a tag to the directory the note lives in",
            ),
        ]),
    );

    // Its offsets are measured from the start of the EXIF block, which starts at the same
    // place in both files - so the same index in the file is the same offset in the block.
    let note = fx::maker_note();
    let before = index_of(&original, &note);
    assert!(before > 0);
    assert_eq!(before, index_of(&written, &note));

    // And what it did not touch is still read: the camera's own fields.
    let doc = read(&codec, &written);
    assert_eq!("TestCam", doc.get(MetadataField::CameraMake));
    assert_eq!("2021-06-05 04:03:02", doc.get(MetadataField::DateTaken));
}

#[test]
fn a_cleared_field_is_gone_from_the_bytes_not_just_from_the_directory() {
    let codec = JpegCodec;
    let original = camera();
    assert!(index_of(&original, b"Old title") > 0);

    let written = write(&codec, &original, &edit(&[(MetadataField::Title, "")]));

    assert_eq!("", read(&codec, &written).get(MetadataField::Title));
    assert_eq!(-1, index_of(&written, b"Old title"));
    assert_eq!(
        "TestCam",
        read(&codec, &written).get(MetadataField::CameraMake)
    );
}

#[test]
fn editing_the_same_field_again_reuses_its_room() {
    let codec = JpegCodec;
    let first = write(
        &codec,
        &camera(),
        &edit(&[(MetadataField::Title, "First title")]),
    );
    let second = write(
        &codec,
        &first,
        &edit(&[(MetadataField::Title, "Other title")]),
    );
    let third = write(&codec, &second, &edit(&[(MetadataField::Title, "Third")]));

    assert_eq!(first.len(), second.len());
    assert_eq!(first.len(), third.len());
    assert_eq!("Third", read(&codec, &third).get(MetadataField::Title));
}

#[test]
fn a_title_too_long_for_the_segment_is_refused() {
    let huge = "x".repeat(70_000);
    let result = try_write(
        &JpegCodec,
        &plain_jpeg(),
        &edit(&[(MetadataField::Title, &huge)]),
    );
    assert!(is_format(&result));
}

#[test]
fn anything_that_is_not_a_jpeg_is_refused_rather_than_guessed_at() {
    let codec = JpegCodec;
    let real = camera();

    assert!(is_format(&try_read(&codec, &[])));
    assert!(is_format(&try_read(&codec, b"not a picture")));
    assert!(is_format(&try_read(&codec, &real[..40])));
    assert!(is_format(&try_read(
        &codec,
        &[0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01]
    )));
}

#[test]
fn a_damaged_or_hostile_exif_block_never_hangs_or_escapes() {
    let codec = JpegCodec;
    let real = camera();
    let exif_start = index_of(&real, b"Exif\0\0") as usize + 6;

    // Every single-byte corruption of the block's first two hundred bytes: each either reads
    // or is refused by name. Offsets pointing at themselves, counts of 65,535, types that do
    // not exist - all of them are in here somewhere.
    for at in 0..200 {
        for poison in [0x00u8, 0xFF, 0x08] {
            let mut damaged = real.clone();
            damaged[exif_start + at] = poison;

            match try_read(&codec, &damaged) {
                Ok(_) => {}
                Err(e) => {
                    assert!(e.is_format(), "read at {at}: {e}");
                    continue;
                }
            }
            if let Err(e) = try_write(&codec, &damaged, &edit(&[(MetadataField::Title, "x")])) {
                assert!(e.is_format(), "write at {at}: {e}");
            }
        }
    }
}

// ---- removing ---------------------------------------------------------------------------------

fn remove_location() -> MetadataEdit {
    MetadataEdit {
        remove_location: true,
        ..MetadataEdit::none()
    }
}

fn remove_all() -> MetadataEdit {
    MetadataEdit {
        remove_all: true,
        ..MetadataEdit::none()
    }
}

/// A JPEG with one more APP1 segment spliced in after its JFIF header.
fn with_segment(jpeg: &[u8], label: &[u8], body: &[u8]) -> Vec<u8> {
    let length = label.len() + body.len() + 2;
    let mut out = jpeg[..20].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1, (length >> 8) as u8, length as u8]);
    out.extend_from_slice(label);
    out.extend_from_slice(body);
    out.extend_from_slice(&jpeg[20..]);
    out
}

fn with_xmp(jpeg: &[u8], packet: &str) -> Vec<u8> {
    with_segment(jpeg, b"http://ns.adobe.com/xap/1.0/\0", packet.as_bytes())
}

const XMP_WITH_LOCATION: &str = concat!(
    "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>",
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">",
    "<rdf:Description rdf:about=\"\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" ",
    "exif:GPSLatitude=\"51,30.4N\" exif:GPSLongitude=\"0,7.5W\" exif:ExposureTime=\"1/60\">",
    "<exif:GPSAltitude>35/1</exif:GPSAltitude><dc:creator><rdf:Seq><rdf:li>Renée</rdf:li></rdf:Seq></dc:creator>",
    "</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
);

#[test]
fn a_location_is_reported_and_removing_it_takes_only_the_location() {
    let codec = JpegCodec;
    let original = camera();
    assert!(read(&codec, &original).has_location);
    assert!(!read(&codec, &plain_jpeg()).has_location);

    let written = write(&codec, &original, &remove_location());
    let doc = read(&codec, &written);

    assert!(!doc.has_location);
    assert_eq!("Old title", doc.get(MetadataField::Title));
    assert_eq!("2021-06-05 04:03:02", doc.get(MetadataField::DateTaken));
    let note = fx::maker_note();
    assert_eq!(index_of(&original, &note), index_of(&written, &note));
    assert_eq!(digest(&codec, &original), digest(&codec, &written));

    // Same length: the directory was zeroed where it stood, not cut out, so nothing moved.
    assert_eq!(original.len(), written.len());
}

#[test]
fn a_location_written_in_xmp_as_well_is_removed_from_both() {
    let codec = JpegCodec;
    let original = with_xmp(&camera(), XMP_WITH_LOCATION);
    assert!(read(&codec, &original).has_location);

    let written = write(&codec, &original, &remove_location());
    let text = String::from_utf8_lossy(&written).into_owned();

    assert!(!read(&codec, &written).has_location);
    assert!(!text.contains("GPS"));
    assert!(!text.contains("51,30.4N"));

    // What was beside it in the packet is still there.
    assert!(text.contains("exif:ExposureTime=\"1/60\""));
    assert!(text.contains("Renée"));
    assert!(text.contains("<?xpacket end=\"w\"?>"));
    assert_eq!(digest(&codec, &original), digest(&codec, &written));
}

#[test]
fn a_packet_that_states_a_field_changes_with_it_and_one_is_never_invented() {
    let codec = JpegCodec;
    const PACKET: &str = concat!(
        "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>",
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">",
        "<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" ",
        "xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\" xmp:Rating=\"2\" lr:private=\"kept\">",
        "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Stale title</rdf:li></rdf:Alt></dc:title>",
        "<dc:subject><rdf:Bag><rdf:li>old</rdf:li></rdf:Bag></dc:subject>",
        "<dc:creator><rdf:Seq><rdf:li>Somebody</rdf:li></rdf:Seq></dc:creator>",
        "</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    );

    let original = with_xmp(&camera(), PACKET);
    let written = write(
        &codec,
        &original,
        &edit(&[
            (MetadataField::Title, "Fresh title"),
            (MetadataField::Keywords, "sea; boats"),
            (MetadataField::Rating, ""),
        ]),
    );
    let text = String::from_utf8_lossy(&written).into_owned();

    assert!(!text.contains("Stale title"));
    assert!(text.contains("<rdf:li xml:lang=\"x-default\">Fresh title</rdf:li>"));
    assert!(text.contains("<rdf:li>sea</rdf:li><rdf:li>boats</rdf:li>"));
    assert!(!text.contains("<rdf:li>old</rdf:li>"));
    assert!(!text.contains("xmp:Rating"));

    // What was not edited, and what this app has never heard of, is exactly as it was.
    assert!(text.contains("<rdf:li>Somebody</rdf:li>"));
    assert!(text.contains("lr:private=\"kept\""));
    assert!(!text.contains("dc:description"));
    assert_eq!(digest(&codec, &original), digest(&codec, &written));

    // And a picture with no packet does not get one.
    let bare = write(
        &codec,
        &camera(),
        &edit(&[(MetadataField::Title, "Fresh title")]),
    );
    assert_eq!(-1, index_of(&bare, b"xmpmeta"));
}

#[test]
fn a_location_that_cannot_be_shown_to_be_gone_is_refused_not_claimed() {
    let codec = JpegCodec;
    let plain = plain_jpeg();

    // Not XML at all, but it says GPS: the safe reading is that a location is in there.
    let broken = with_xmp(&plain, "<x:xmpmeta exif:GPSLatitude=\"51,30.4N\" <<<");
    assert!(read(&codec, &broken).has_location);
    assert!(is_format(&try_write(&codec, &broken, &remove_location())));

    // The overflow segments of a large packet are fragments, never parsed.
    let extended = with_segment(
        &plain,
        b"http://ns.adobe.com/xmp/extension/\0",
        "…exif:GPSLatitude…".as_bytes(),
    );
    assert!(read(&codec, &extended).has_location);
    assert!(is_format(&try_write(&codec, &extended, &remove_location())));

    // A DTD is how hostile XML reaches outside itself. It is never processed.
    let hostile = with_xmp(
        &plain,
        "<!DOCTYPE x [<!ENTITY e SYSTEM \"file:///c:/windows/win.ini\">]><x>GPS &e;</x>",
    );
    assert!(read(&codec, &hostile).has_location);
    assert!(is_format(&try_write(&codec, &hostile, &remove_location())));
}

#[test]
fn removing_everything_leaves_the_picture_and_how_it_is_drawn() {
    let codec = JpegCodec;
    let camera = camera();

    // Give it an orientation, an XMP packet and a comment segment to lose or keep.
    let sideways = write(&codec, &camera, &MetadataEdit::none());
    let start = index_of(&sideways, b"Exif\0\0") as usize + 6;
    let mut block = ExifBlock::parse(&sideways[start..start + 0xFE - 6]).unwrap();
    ExifFields::set_orientation(&mut block, 6);
    let exif = block.to_bytes(65_000).unwrap();

    let mut built = camera[..20].to_vec();
    built.extend_from_slice(&[
        0xFF,
        0xE1,
        ((exif.len() + 8) >> 8) as u8,
        (exif.len() + 8) as u8,
    ]);
    built.extend_from_slice(b"Exif\0\0");
    built.extend_from_slice(&exif);
    built.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x09]);
    built.extend_from_slice(b"private");
    built.extend_from_slice(&camera[20 + 2 + 0xFE..]);
    let original = with_xmp(&built, XMP_WITH_LOCATION);

    assert_eq!(
        "TestCam",
        read(&codec, &original).get(MetadataField::CameraMake)
    );

    let written = write(&codec, &original, &remove_all());
    let doc = read(&codec, &written);

    assert!(doc.values.is_empty());
    assert!(!doc.has_location);
    assert_eq!(-1, index_of(&written, &fx::maker_note()));
    assert_eq!(-1, index_of(&written, b"TestCam"));
    assert_eq!(-1, index_of(&written, b"xmpmeta"));
    assert_eq!(-1, index_of(&written, b"private"));
    assert_eq!(digest(&codec, &original), digest(&codec, &written));

    // Still the right way up: orientation is how the picture is drawn, not what it says.
    let at = index_of(&written, b"Exif\0\0") as usize + 6;
    let kept = ExifBlock::parse(&written[at..at + 26]).unwrap();
    assert_eq!(Some(6), ExifFields::orientation(&kept));
}

#[test]
fn removing_everything_from_a_picture_with_nothing_adds_nothing() {
    let plain = plain_jpeg();
    assert_eq!(plain, write(&JpegCodec, &plain, &remove_all()));
}

#[test]
fn removing_everything_from_a_song_takes_every_tag_at_both_ends() {
    let codec = mp3();
    let original = fx::mp3(&[("TIT2", "Song"), ("TPE1", "Artist")], true);
    let written = write(&codec, &original, &remove_all());

    assert!(read(&codec, &written).values.is_empty());
    assert_eq!(-1, index_of(&written, b"Old v1 title"));
    assert_eq!(-1, index_of(&written, b"Artist"));
    assert_eq!(digest(&codec, &original), digest(&codec, &written));

    // And a field set in the same edit is the only thing left.
    let only = MetadataEdit {
        remove_all: true,
        ..edit(&[(MetadataField::Title, "Only this")])
    };
    let retitled = write(&codec, &original, &only);
    let keys: Vec<MetadataField> = read(&codec, &retitled).values.keys().copied().collect();
    assert_eq!(vec![MetadataField::Title], keys);
}

// ---- MP3 --------------------------------------------------------------------------------------

#[test]
fn an_mp3_reads_the_tag_somebody_else_wrote() {
    let file = fx::mp3(
        &[
            ("TIT2", "Song"),
            ("TPE1", "Artist"),
            ("TALB", "Album"),
            ("TRCK", "3/12"),
            ("TYER", "1999"),
        ],
        false,
    );
    let doc = read(&mp3(), &file);

    assert_eq!("Song", doc.get(MetadataField::Title));
    assert_eq!("Artist", doc.get(MetadataField::Artist));
    assert_eq!("Album", doc.get(MetadataField::Album));
    assert_eq!("3", doc.get(MetadataField::TrackNumber));
    assert_eq!("12", doc.get(MetadataField::TrackTotal));
    assert_eq!("1999", doc.get(MetadataField::Year));
}

#[test]
fn the_oldest_tag_layout_is_read_too() {
    let doc = read(&mp3(), &fx::mp3_v22("Old song", "Old artist"));

    assert_eq!("Old song", doc.get(MetadataField::Title));
    assert_eq!("Old artist", doc.get(MetadataField::Artist));
}

fn everything_audio() -> MetadataEdit {
    edit(&[
        (MetadataField::Title, "Наша песня"),
        (MetadataField::Artist, "Björk"),
        (MetadataField::AlbumArtist, "Various"),
        (MetadataField::Album, "Homogenic"),
        (MetadataField::TrackNumber, "7"),
        (MetadataField::TrackTotal, "10"),
        (MetadataField::DiscNumber, "1"),
        (MetadataField::DiscTotal, "2"),
        (MetadataField::Year, "1997"),
        (MetadataField::Genre, "Electronic"),
        (MetadataField::Composer, "Somebody"),
        (MetadataField::Comment, "A comment"),
        (MetadataField::Copyright, "One Little Indian"),
    ])
}

#[test]
fn every_audio_field_round_trips_and_the_sound_is_untouched() {
    let codec = mp3();
    let e = everything_audio();
    let originals = [
        fx::mp3(&[("TIT2", "Song"), ("TPE1", "Artist")], false),
        fx::mp3(&[("TIT2", "Song")], true),
        fx::mp3_v22("Old song", "Old artist"),
        fx::audio(),
    ];

    for (n, original) in originals.iter().enumerate() {
        let written = write(&codec, original, &e);
        let doc = read(&codec, &written);

        for (field, value) in &e.changes {
            assert_eq!(value, doc.get(*field), "original {n}, {field:?}");
        }
        assert_eq!(
            digest(&codec, original),
            digest(&codec, &written),
            "original {n}"
        );
        assert!(index_of(&written, &fx::audio()) > 0, "original {n}");
    }
}

#[test]
fn an_untouched_audio_field_is_left_alone_and_a_cleared_one_is_gone() {
    let codec = mp3();
    let original = fx::mp3(
        &[("TIT2", "Song"), ("TPE1", "Artist"), ("TRCK", "3")],
        false,
    );
    let written = write(
        &codec,
        &original,
        &edit(&[
            (MetadataField::Artist, ""),
            (MetadataField::TrackNumber, ""),
        ]),
    );
    let doc = read(&codec, &written);

    assert_eq!("Song", doc.get(MetadataField::Title));
    assert_eq!("", doc.get(MetadataField::Artist));
    assert_eq!("", doc.get(MetadataField::TrackNumber));
}

#[test]
fn something_named_mp3_that_is_not_one_is_refused() {
    let codec = mp3();
    assert!(is_format(&try_read(&codec, &[])));
    assert!(is_format(&try_read(
        &codec,
        b"this is a text file, not a song"
    )));
    assert!(is_format(&try_read(&codec, &plain_jpeg()[..300])));
}

#[test]
fn the_payload_digest_ignores_tags_and_sees_the_sound() {
    let codec = mp3();
    let bare = fx::audio();
    let tagged = fx::mp3(&[("TIT2", "Song")], true);
    assert_eq!(digest(&codec, &bare), digest(&codec, &tagged));

    let mut changed = tagged.clone();
    let at = index_of(&changed, &bare) as usize + 1000;
    changed[at] ^= 0xFF;
    assert_ne!(digest(&codec, &tagged), digest(&codec, &changed));
}

// ---- the other containers ---------------------------------------------------------------------

const CONTAINERS: [&str; 5] = [".flac", ".wav", ".m4a", ".mp4", ".wma"];

fn fixture(extension: &str) -> Vec<u8> {
    match extension {
        ".flac" => fx::bytes(fx::FLAC),
        ".wav" => fx::wav(),
        ".m4a" => fx::bytes(fx::M4A),
        ".mp4" => fx::bytes(fx::MP4_VIDEO),
        ".wma" => fx::bytes(fx::WMA),
        other => panic!("no fixture for {other}"),
    }
}

#[test]
fn every_container_takes_every_field_and_keeps_its_sound() {
    for extension in CONTAINERS {
        let codec = AudioTagCodec::new(extension);
        let original = fixture(extension);
        assert!(read(&codec, &original).values.is_empty(), "{extension}");

        let e = everything_audio();
        let written = write(&codec, &original, &e);
        let doc = read(&codec, &written);

        for (field, value) in &e.changes {
            assert_eq!(value, doc.get(*field), "{extension} {field:?}");
        }
        assert_eq!(
            digest(&codec, &original),
            digest(&codec, &written),
            "{extension}"
        );

        // Edited again, and then emptied: the same sound each time.
        let again = write(
            &codec,
            &written,
            &edit(&[
                (MetadataField::Title, "Shorter"),
                (MetadataField::Comment, ""),
            ]),
        );
        assert_eq!(
            "Shorter",
            read(&codec, &again).get(MetadataField::Title),
            "{extension}"
        );
        assert_eq!(
            "",
            read(&codec, &again).get(MetadataField::Comment),
            "{extension}"
        );
        assert_eq!(
            "Björk",
            read(&codec, &again).get(MetadataField::Artist),
            "{extension}"
        );
        assert_eq!(
            digest(&codec, &original),
            digest(&codec, &again),
            "{extension}"
        );

        let stripped = write(&codec, &again, &remove_all());
        assert!(read(&codec, &stripped).values.is_empty(), "{extension}");
        assert_eq!(
            digest(&codec, &original),
            digest(&codec, &stripped),
            "{extension}"
        );
    }
}

#[test]
fn a_container_named_wrongly_is_refused() {
    for extension in CONTAINERS {
        let codec = AudioTagCodec::new(extension);

        assert!(is_format(&try_read(&codec, &[])), "{extension}");
        assert!(
            is_format(&try_read(&codec, b"nothing like it at all, really")),
            "{extension}"
        );
        assert!(is_format(&try_read(&codec, &fx::audio())), "{extension}");
        assert!(
            is_format(&try_read(&codec, &fixture(extension)[..40])),
            "{extension}"
        );
    }
}

#[test]
fn an_mpeg4_file_whose_tables_were_left_behind_does_not_pass_for_untouched() {
    // What a careless tag writer does: makes room near the top, pushes the media data down the
    // file, and forgets to move the offsets that say where each piece of it starts. Every byte
    // of sound and picture is still there and still the same - and nothing can play it.
    for extension in [".m4a", ".mp4"] {
        let codec = AudioTagCodec::new(extension);
        let original = fixture(extension);

        let media = index_of(&original, b"mdat") - 4;
        assert!(media > 0, "{extension}");
        let media = media as usize;

        let mut pushed = original[..media].to_vec();
        pushed.extend_from_slice(&[0, 0, 0, 16]);
        pushed.extend_from_slice(b"free");
        pushed.extend_from_slice(&[0u8; 8]);
        pushed.extend_from_slice(&original[media..]);

        assert_ne!(
            digest(&codec, &original),
            digest(&codec, &pushed),
            "{extension}"
        );
    }
}

#[test]
fn a_total_with_no_number_to_go_with_it_is_refused_by_name() {
    let codec = mp3();
    let result = try_write(
        &codec,
        &fx::audio(),
        &edit(&[(MetadataField::TrackTotal, "12")]),
    );
    match result {
        Err(MetadataError::Format(message)) => {
            assert!(message.contains("track number"), "{message}")
        }
        other => panic!("expected a format error, got {other:?}"),
    }

    // With one already there, a total alone is fine.
    let numbered = fx::mp3(&[("TRCK", "3")], false);
    let written = write(
        &codec,
        &numbered,
        &edit(&[(MetadataField::TrackTotal, "12")]),
    );
    assert_eq!("12", read(&codec, &written).get(MetadataField::TrackTotal));
}

#[test]
fn the_extension_decides_which_codec() {
    let id = |path: &str| MetadataCodecs::for_path(path).map(|c| c.id());
    assert_eq!(Some("jpeg"), id(r"C:\x\a.JPG"));
    assert_eq!(Some("audio"), id(r"C:\x\a.mp3"));
    assert_eq!(None, id(r"C:\x\a.txt"));
    assert_eq!(None, id(r"C:\x\noextension"));
}

// ---- an ASF header Windows itself reads ---------------------------------------------------------

/// The Extended Content Description Object's GUID, D2D0A440-E307-11D2-97F0-00A0C95EA850, in the byte
/// order a header stores it. The port once carried a different last half, which no reader of the file
/// (Windows' own property handler included) would look for: the album was written and read back by the
/// codec alone and never seen by anything else.
const ECD_GUID: [u8; 16] = [
    0x40, 0xA4, 0xD0, 0xD2, 0x07, 0xE3, 0xD2, 0x11, 0x97, 0xF0, 0x00, 0xA0, 0xC9, 0x5E, 0xA8, 0x50,
];

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[test]
fn a_wma_header_carries_its_descriptions_under_the_specifications_guid() {
    let codec = AudioTagCodec::new(".wma");
    let original = fixture(".wma");
    assert!(
        find(&original, &ECD_GUID).is_some(),
        "the Windows-encoded fixture has an extended content description"
    );

    let written = write(
        &codec,
        &original,
        &edit(&[(MetadataField::Album, "Five containers")]),
    );
    let header = &written[..written.len().min(4096)];
    assert!(
        find(header, &ECD_GUID).is_some(),
        "the rewritten header names it the way the spec does"
    );
    assert_eq!(
        "Five containers",
        read(&codec, &written).get(MetadataField::Album)
    );

    // The fixture's own description (written by Windows, so under the right GUID) is read, not skipped.
    let again = write(&codec, &written, &edit(&[(MetadataField::Year, "2024")]));
    assert_eq!(
        "Five containers",
        read(&codec, &again).get(MetadataField::Album)
    );
    assert_eq!("2024", read(&codec, &again).get(MetadataField::Year));
}
