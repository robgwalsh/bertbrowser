// Port of tests/BertBrowser.Core.Tests/MetadataFormatsTests.cs. The formats beyond JPEG and MP3: PNG
// and WebP pictures, Office documents and EPUB books, the camera-setting fields, and the audio
// fields and cover pictures every container is asked to keep.
use std::collections::BTreeMap;
use std::io::{Cursor, Read as _, Write as _};

use bb_core::cancel::CancelToken;
use bb_core::metadata::{
    AudioTagCodec, EpubCodec, JpegCodec, MetadataCodec, MetadataCodecs, MetadataDocument,
    MetadataEdit, MetadataField, MetadataFieldKind, MetadataFields, MetadataResult, OpenXmlCodec,
    PngCodec, TiffCodec, WebPCodec,
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

fn try_digest(codec: &dyn MetadataCodec, file: &[u8]) -> MetadataResult<Vec<u8>> {
    codec.payload_digest(&mut Cursor::new(file.to_vec()))
}

fn digest(codec: &dyn MetadataCodec, file: &[u8]) -> Vec<u8> {
    try_digest(codec, file).expect("digest")
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

fn last_index_of(haystack: &[u8], needle: &[u8]) -> i64 {
    if needle.is_empty() || haystack.len() < needle.len() {
        return -1;
    }
    haystack
        .windows(needle.len())
        .rposition(|w| w == needle)
        .map(|p| p as i64)
        .unwrap_or(-1)
}

fn is_format<T>(result: &MetadataResult<T>) -> bool {
    matches!(result, Err(e) if e.is_format())
}

/// A byte string as the text Latin-1 would make of it.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// A value of the right kind for each field, with text no single-byte code page holds.
fn sample(field: MetadataField) -> String {
    match MetadataFields::get(field).kind {
        MetadataFieldKind::Number => if field == MetadataField::Year {
            "1997"
        } else {
            "7"
        }
        .to_string(),
        MetadataFieldKind::Date => "2024-03-14 09:26:53".to_string(),
        MetadataFieldKind::List => "sea; boats; 日本".to_string(),
        MetadataFieldKind::Rating => "4".to_string(),
        MetadataFieldKind::Decimal => "2.8".to_string(),
        MetadataFieldKind::Exposure => "1/250".to_string(),
        MetadataFieldKind::Text => {
            if field == MetadataField::Lyrics {
                "First line\nSecond line".to_string()
            } else {
                format!("Värde {field:?} 日本")
            }
        }
    }
}

fn everything(codec: &dyn MetadataCodec) -> MetadataEdit {
    let changes: BTreeMap<MetadataField, String> =
        codec.fields().iter().map(|f| (*f, sample(*f))).collect();
    MetadataEdit::none().with_changes(changes)
}

fn remove_all() -> MetadataEdit {
    MetadataEdit {
        remove_all: true,
        ..MetadataEdit::none()
    }
}

fn remove_location() -> MetadataEdit {
    MetadataEdit {
        remove_location: true,
        ..MetadataEdit::none()
    }
}

// ---- values -----------------------------------------------------------------------------------

#[test]
fn a_shutter_time_is_written_the_way_a_camera_shows_it() {
    let rows = [
        ("1/250", "1/250"),
        ("0.004", "1/250"),
        ("0.5", "1/2"),
        ("2/4", "1/2"),
        ("0.3", "0.3"),
        ("2.5", "2.5"),
        ("30", "30"),
        ("1", "1"),
    ];
    for (typed, expected) in rows {
        let value = MetadataFields::try_normalize(MetadataField::ExposureTime, typed)
            .unwrap_or_else(|| panic!("{typed} refused"));
        assert_eq!(expected, value, "{typed}");

        // And what is stored reads back as exactly what was typed, or every edit would fail its check.
        let (top, bottom) = MetadataFields::try_parse_exposure(&value).expect(typed);
        assert_eq!(
            value,
            MetadataFields::format_exposure(top, bottom),
            "{typed}"
        );
    }
}

#[test]
fn a_measurement_that_is_not_one_is_refused() {
    let rows = [
        (MetadataField::ExposureTime, "fast"),
        (MetadataField::ExposureTime, "1/0"),
        (MetadataField::ExposureTime, "0"),
        (MetadataField::Aperture, "-2.8"),
        (MetadataField::Aperture, "f/2.8"),
        (MetadataField::FocalLength, "0"),
    ];
    for (field, typed) in rows {
        assert!(
            MetadataFields::try_normalize(field, typed).is_none(),
            "{field:?} {typed:?}"
        );
    }
}

#[test]
fn an_aperture_keeps_one_decimal_place() {
    assert_eq!(
        Some("2.8".to_string()),
        MetadataFields::try_normalize(MetadataField::Aperture, "2.80")
    );
    assert_eq!(
        Some("50".to_string()),
        MetadataFields::try_normalize(MetadataField::FocalLength, "50")
    );
}

// ---- pictures ---------------------------------------------------------------------------------

const PICTURES: [&str; 3] = [".jpg", ".png", ".webp"];

fn picture(extension: &str) -> (Box<dyn MetadataCodec>, Vec<u8>) {
    match extension {
        ".jpg" => (Box::new(JpegCodec), fx::jpeg(fx::PLAIN_JPEG)),
        ".png" => (Box::new(PngCodec), fx::bytes(fx::PNG)),
        ".webp" => (Box::new(WebPCodec), fx::bytes(fx::WEBP)),
        other => panic!("no picture for {other}"),
    }
}

#[test]
fn every_picture_format_takes_every_exif_field_and_keeps_its_pixels() {
    for extension in PICTURES {
        let (codec, original) = picture(extension);
        let codec = codec.as_ref();
        assert!(read(codec, &original).values.is_empty(), "{extension}");

        let e = everything(codec);
        let written = write(codec, &original, &e);
        let doc = read(codec, &written);

        for (field, value) in &e.changes {
            assert_eq!(value, doc.get(*field), "{extension} {field:?}");
        }
        assert_eq!(
            digest(codec, &original),
            digest(codec, &written),
            "{extension}"
        );

        // Edited again in place, then emptied.
        let again = write(
            codec,
            &written,
            &edit(&[(MetadataField::Title, "Shorter"), (MetadataField::Iso, "")]),
        );
        assert_eq!(
            "Shorter",
            read(codec, &again).get(MetadataField::Title),
            "{extension}"
        );
        assert_eq!(
            "",
            read(codec, &again).get(MetadataField::Iso),
            "{extension}"
        );
        assert_eq!(
            "1/250",
            read(codec, &again).get(MetadataField::ExposureTime),
            "{extension}"
        );
        assert_eq!(
            digest(codec, &original),
            digest(codec, &again),
            "{extension}"
        );

        let stripped = write(codec, &again, &remove_all());
        assert!(read(codec, &stripped).values.is_empty(), "{extension}");
        assert_eq!(original, stripped, "{extension}");
    }
}

#[test]
fn a_picture_named_wrongly_is_refused() {
    for extension in PICTURES {
        let (codec, original) = picture(extension);
        let codec = codec.as_ref();

        assert!(is_format(&try_read(codec, &[])), "{extension}");
        assert!(
            is_format(&try_read(codec, b"not a picture of any kind at all")),
            "{extension}"
        );
        assert!(
            is_format(&try_read(codec, &original[..original.len() / 2])),
            "{extension}"
        );
    }
}

#[test]
fn a_png_chunk_is_written_before_the_picture_data_with_a_true_checksum() {
    let written = write(
        &PngCodec,
        &fx::bytes(fx::PNG),
        &edit(&[(MetadataField::Title, "Hello")]),
    );

    let exif = index_of(&written, b"eXIf");
    let data = index_of(&written, b"IDAT");
    assert!(exif > 0 && exif < data);
    let exif = exif as usize;

    // Checked with an independent CRC-32, not the codec's own table.
    let length = u32::from_be_bytes(written[exif - 4..exif].try_into().unwrap()) as usize;
    let expected = crc32fast::hash(&written[exif..exif + 4 + length]);
    let stored = u32::from_be_bytes(
        written[exif + 4 + length..exif + 8 + length]
            .try_into()
            .unwrap(),
    );
    assert_eq!(expected, stored);
}

#[test]
fn a_simple_webp_gains_the_header_metadata_needs_and_loses_it_again() {
    let codec = WebPCodec;
    let original = fx::bytes(fx::WEBP);
    assert_eq!(-1, index_of(&original, b"VP8X"));

    let written = write(&codec, &original, &edit(&[(MetadataField::Title, "Hello")]));

    // The extended header comes first, says there is EXIF, and states the one-pixel canvas.
    assert_eq!(b"VP8X", &written[12..16]);
    assert_eq!(0x08, written[20] & 0x0C);
    assert_eq!(&[0u8; 6], &written[24..30]);
    assert_eq!(
        written.len() as i32 - 8,
        i32::from_le_bytes(written[4..8].try_into().unwrap())
    );
    assert!(index_of(&written, b"VP8L") < index_of(&written, b"EXIF"));
    assert_eq!(0, written.len() % 2);
}

#[test]
fn camera_settings_already_in_a_picture_are_read_and_changed_in_place() {
    let codec = JpegCodec;
    let camera = fx::jpeg(fx::CAMERA_JPEG);

    let written = write(
        &codec,
        &camera,
        &edit(&[
            (MetadataField::Iso, "400"),
            (MetadataField::Aperture, "5.6"),
            (MetadataField::ExposureTime, "1/60"),
            (MetadataField::FocalLength, "35"),
            (MetadataField::LensModel, "35mm F1.4"),
        ]),
    );
    let doc = read(&codec, &written);

    assert_eq!("400", doc.get(MetadataField::Iso));
    assert_eq!("5.6", doc.get(MetadataField::Aperture));
    assert_eq!("1/60", doc.get(MetadataField::ExposureTime));
    assert_eq!("35", doc.get(MetadataField::FocalLength));
    assert_eq!("35mm F1.4", doc.get(MetadataField::LensModel));

    // The note that must not move has not, with five tags added to the directory it lives in.
    let note = fx::maker_note();
    assert_eq!(index_of(&camera, &note), index_of(&written, &note));
    assert_eq!("TestCam", doc.get(MetadataField::CameraMake));
}

fn dataset(number: u8, text: &str) -> Vec<u8> {
    let mut out = vec![0x1C, 2, number, 0, text.len() as u8];
    out.extend(text.chars().map(|c| c as u8));
    out
}

#[test]
fn an_iptc_record_that_states_a_field_changes_with_it_or_falls_silent() {
    // Laid out by hand from the IIM and Photoshop resource specifications: a title, two
    // keywords, a caption, a city this app has no field for, and Photoshop's checksum of the lot.
    let mut record = Vec::new();
    record.extend(dataset(5, "Stale title"));
    record.extend(dataset(25, "old"));
    record.extend(dataset(25, "older"));
    record.extend(dataset(120, "Stale caption"));
    record.extend(dataset(90, "Lisbon"));

    let mut resources = Vec::new();
    resources.extend_from_slice(b"Photoshop 3.0\0");
    resources.extend_from_slice(b"8BIM");
    resources.extend_from_slice(&[0x04, 0x04, 0, 0, 0, 0, 0, record.len() as u8]);
    resources.extend_from_slice(&record);
    if record.len() % 2 != 0 {
        resources.push(0);
    }
    resources.extend_from_slice(b"8BIM");
    resources.extend_from_slice(&[0x04, 0x25, 0, 0, 0, 0, 0, 16]);
    resources.extend_from_slice(&[0u8; 16]);

    let jpeg = fx::jpeg(fx::CAMERA_JPEG);
    let length = resources.len() + 2;
    let mut original = jpeg[..20].to_vec();
    original.extend_from_slice(&[0xFF, 0xED, (length >> 8) as u8, length as u8]);
    original.extend_from_slice(&resources);
    original.extend_from_slice(&jpeg[20..]);

    let codec = JpegCodec;
    let written = write(
        &codec,
        &original,
        &edit(&[
            (MetadataField::Title, "Fresh title"),
            (MetadataField::Keywords, "sea; boats"),
        ]),
    );
    let text = latin1(&written);

    assert!(!text.contains("Stale title"));
    assert!(!text.contains("Stale caption"));
    assert!(!text.contains("older"));
    assert!(text.contains("\u{1C}\u{2}\u{5}\0\u{B}Fresh title"));
    assert!(text.contains("\u{1C}\u{2}x\0\u{B}Fresh title"));
    assert!(text.contains("\u{1C}\u{2}\u{19}\0\u{3}sea"));
    assert!(text.contains("\u{1C}\u{2}\u{19}\0\u{5}boats"));
    assert!(text.contains("Lisbon"));

    // The checksum of the old record is gone rather than left saying the record is out of date.
    assert_eq!(
        -1,
        index_of(&written, &[0x38, 0x42, 0x49, 0x4D, 0x04, 0x25])
    );
    assert_eq!(digest(&codec, &original), digest(&codec, &written));

    // A title this record's character set cannot spell is taken out of it, not misspelt.
    let unspellable = write(&codec, &original, &edit(&[(MetadataField::Title, "日本")]));
    assert_eq!("日本", read(&codec, &unspellable).get(MetadataField::Title));
    let after = latin1(&unspellable);
    assert!(!after.contains("Stale title"));
    assert!(!after.contains("Stale caption"));
    assert!(!after.contains("??"));
    assert!(after.contains("\u{1C}\u{2}\u{19}\0\u{5}older"));
    assert!(after.contains("Lisbon"));
}

#[test]
fn a_tiff_is_edited_without_moving_a_pixel() {
    let codec = TiffCodec;
    let original = fx::bytes(fx::TIFF);
    let doc = read(&codec, &original);

    assert_eq!("Old tiff title", doc.get(MetadataField::Title));
    assert_eq!("ScanCo", doc.get(MetadataField::CameraMake));
    assert_eq!("2020-01-02 03:04:05", doc.get(MetadataField::DateTaken));
    assert!(doc.has_location);

    let e = everything(&codec);
    let written = write(&codec, &original, &e);

    for (field, value) in &e.changes {
        assert_eq!(value, read(&codec, &written).get(*field), "{field:?}");
    }
    assert_eq!(digest(&codec, &original), digest(&codec, &written));

    // The strip of pixels is the first thing in the file after its header, and still is:
    // everything new went on the end.
    assert_eq!(&original[8..200], &written[8..200]);
    assert!(written.len() > original.len());

    let unlocated = write(&codec, &written, &remove_location());
    assert!(!read(&codec, &unlocated).has_location);
    assert_eq!(
        e.changes[&MetadataField::Title],
        read(&codec, &unlocated).get(MetadataField::Title)
    );

    let stripped = write(&codec, &written, &remove_all());
    assert!(read(&codec, &stripped).values.is_empty());
    assert!(!read(&codec, &stripped).has_location);
    assert_eq!(-1, index_of(&stripped, b"ScanCo"));
    assert_eq!(-1, index_of(&stripped, "2024:03:14".as_bytes()));
    assert_eq!(digest(&codec, &original), digest(&codec, &stripped));
}

#[test]
fn a_tiff_whose_pixels_or_shape_changed_does_not_pass_for_untouched() {
    let codec = TiffCodec;
    let original = fx::bytes(fx::TIFF);

    let mut pixel = original.clone();
    pixel[40] ^= 0xFF;
    assert_ne!(digest(&codec, &original), digest(&codec, &pixel));

    // The width, in the directory: same pixels, a different picture.
    let width = index_of(
        &original,
        &[0x00, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08],
    );
    assert!(width > 0);
    let mut reshaped = original.clone();
    reshaped[width as usize + 8] = 4;
    assert_ne!(digest(&codec, &original), digest(&codec, &reshaped));

    assert!(is_format(&try_read(&codec, &original[..6])));
    assert!(is_format(&try_read(
        &codec,
        &[b'I', b'I', 43, 0, 8, 0, 0, 0, 0, 0, 0, 0]
    )));
}

// ---- audio ------------------------------------------------------------------------------------

const CONTAINERS: [&str; 8] = [
    ".mp3", ".flac", ".wav", ".m4a", ".mp4", ".wma", ".aiff", ".opus",
];

fn audio(extension: &str) -> Vec<u8> {
    match extension {
        ".mp3" => fx::audio(),
        ".flac" => fx::bytes(fx::FLAC),
        ".wav" => fx::wav(),
        ".m4a" => fx::bytes(fx::M4A),
        ".mp4" => fx::bytes(fx::MP4_VIDEO),
        ".wma" => fx::bytes(fx::WMA),
        ".aiff" => fx::aiff(),
        ".opus" => fx::opus(),
        other => panic!("no audio for {other}"),
    }
}

#[test]
fn an_ogg_stream_is_judged_by_its_packets_and_its_timing_not_its_pages() {
    let codec = AudioTagCodec::new(".opus");
    let original = fx::opus();

    // A tag edit re-cuts and renumbers the pages; the digest has to see through that...
    let long_title = "X".repeat(3000);
    let tagged = write(
        &codec,
        &original,
        &edit(&[(MetadataField::Title, &long_title)]),
    );
    assert_eq!(digest(&codec, &original), digest(&codec, &tagged));

    // ...and still notice one changed byte of sound, or a page that ends at the wrong time.
    let sound = last_index_of(&original, &[0xF8, 0xFF, 0xFE]);
    assert!(sound >= 0);
    let mut deaf = original.clone();
    deaf[sound as usize + 2] ^= 0x01;
    assert_ne!(digest(&codec, &original), digest(&codec, &deaf));

    let last_page = last_index_of(&original, b"OggS") as usize;
    let mut late = original.clone();
    late[last_page + 6] ^= 0x10;
    assert_ne!(digest(&codec, &original), digest(&codec, &late));

    // Two streams in one file is something this does not claim to understand.
    let mut second = original.clone();
    second[last_page + 14] ^= 0xFF;
    assert!(is_format(&try_digest(&codec, &second)));
}

#[test]
fn every_field_a_container_offers_is_one_it_keeps() {
    // The pin behind each codec's field set: a field offered here that a format silently
    // drops would fail the executor's read-back on every file of that kind.
    for extension in CONTAINERS {
        let codec = AudioTagCodec::new(extension);
        let original = audio(extension);

        let e = everything(&codec);
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
    }
}

#[test]
fn a_cover_picture_goes_in_comes_back_byte_for_byte_and_comes_out() {
    for extension in CONTAINERS {
        let codec = AudioTagCodec::new(extension);
        let original = audio(extension);
        let cover = fx::jpeg(fx::PLAIN_JPEG);
        assert!(codec.holds_picture(), "{extension}");
        assert!(read(&codec, &original).picture.is_none(), "{extension}");

        let set = |bytes: &[u8]| MetadataEdit {
            set_picture: Some(bytes.to_vec()),
            ..MetadataEdit::none()
        };
        let covered = write(&codec, &original, &set(&cover));
        assert_eq!(
            Some(cover.clone()),
            read(&codec, &covered).picture,
            "{extension}"
        );
        assert_eq!(
            digest(&codec, &original),
            digest(&codec, &covered),
            "{extension}"
        );

        // Replaced, not added to.
        let other = fx::bytes(fx::PNG);
        let replaced = write(&codec, &covered, &set(&other));
        assert_eq!(Some(other), read(&codec, &replaced).picture, "{extension}");

        let bare = write(
            &codec,
            &replaced,
            &MetadataEdit {
                remove_picture: true,
                ..MetadataEdit::none()
            },
        );
        assert!(read(&codec, &bare).picture.is_none(), "{extension}");
        assert_eq!(
            digest(&codec, &original),
            digest(&codec, &bare),
            "{extension}"
        );
    }
}

#[test]
fn a_field_one_container_has_no_room_for_is_not_offered_for_it() {
    assert!(
        !AudioTagCodec::new(".wma")
            .fields()
            .contains(&MetadataField::Publisher)
    );
    assert!(
        AudioTagCodec::new(".mp3")
            .fields()
            .contains(&MetadataField::Publisher)
    );

    // And an edit is cut down to what the file can take before it is ever planned.
    let e = edit(&[
        (MetadataField::Publisher, "P"),
        (MetadataField::Title, "T"),
        (MetadataField::Iso, "100"),
    ]);
    let wma: Vec<MetadataField> = e
        .for_codec(&AudioTagCodec::new(".wma"))
        .changes
        .keys()
        .copied()
        .collect();
    assert_eq!(vec![MetadataField::Title], wma);
    let mut jpeg: Vec<MetadataField> = e.for_codec(&JpegCodec).changes.keys().copied().collect();
    jpeg.sort();
    assert_eq!(vec![MetadataField::Title, MetadataField::Iso], jpeg);
}

// ---- documents --------------------------------------------------------------------------------

fn entry(zip: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip.to_vec())).expect("zip");
    let mut file = archive.by_name(name).expect(name);
    let mut text = String::new();
    file.read_to_string(&mut text).unwrap();
    text
}

#[test]
fn a_word_document_reads_and_takes_its_properties_and_nothing_else_changes() {
    let codec = OpenXmlCodec;
    let original = fx::docx();
    let doc = read(&codec, &original);

    assert_eq!("Quarterly report", doc.get(MetadataField::Title));
    assert_eq!("A. Writer", doc.get(MetadataField::Author));
    assert_eq!("finance; q3", doc.get(MetadataField::Keywords));

    let e = everything(&codec);
    let written = write(&codec, &original, &e);

    for (field, value) in &e.changes {
        assert_eq!(value, read(&codec, &written).get(*field), "{field:?}");
    }
    assert_eq!(digest(&codec, &original), digest(&codec, &written));
    assert_eq!(
        entry(&original, "word/document.xml"),
        entry(&written, "word/document.xml")
    );

    // What this app has no field for is still in the part.
    let core = entry(&written, "docProps/core.xml");
    assert!(core.contains("<cp:revision>4</cp:revision>"), "{core}");
    assert!(core.contains("2024-03-14T09:26:53Z"), "{core}");
}

#[test]
fn removing_everything_from_a_document_also_forgets_who_saved_it() {
    let codec = OpenXmlCodec;
    let written = write(&codec, &fx::docx(), &remove_all());

    assert!(read(&codec, &written).values.is_empty());
    assert!(!entry(&written, "docProps/core.xml").contains("A. Writer"));
    assert!(entry(&written, "word/document.xml").contains("The body of the document"));
}

#[test]
fn a_book_reads_and_takes_its_metadata_and_stays_a_book() {
    let codec = EpubCodec;
    let original = fx::epub();
    let doc = read(&codec, &original);

    assert_eq!("A Short Book", doc.get(MetadataField::Title));
    assert_eq!("Some Author", doc.get(MetadataField::Author));
    assert_eq!("Fiction; Short stories", doc.get(MetadataField::Keywords));

    let e = everything(&codec);
    let written = write(&codec, &original, &e);

    for (field, value) in &e.changes {
        assert_eq!(value, read(&codec, &written).get(*field), "{field:?}");
    }
    assert_eq!(digest(&codec, &original), digest(&codec, &written));
    assert_eq!(
        entry(&original, "book/chapter1.xhtml"),
        entry(&written, "book/chapter1.xhtml")
    );

    // The type marker is still the first entry and still stored, which is how a reader that
    // sniffs the first bytes of the file knows what it is holding.
    assert_eq!("mimetypeapplication/epub+zip", latin1(&written[30..58]));
    let mut archive = zip::ZipArchive::new(Cursor::new(written.clone())).unwrap();
    let first = archive.by_index(0).unwrap();
    assert_eq!("mimetype", first.name().unwrap());
    assert_eq!(first.size(), first.compressed_size());
    drop(first);

    let package = entry(&written, "book/package.opf");
    assert!(
        package.contains("<dc:language>en</dc:language>"),
        "{package}"
    );
    assert!(package.contains("<itemref idref=\"c1\""), "{package}");
}

#[test]
fn a_book_whose_reading_order_changed_does_not_pass_for_untouched() {
    let codec = EpubCodec;
    let original = fx::epub();

    let mut source = zip::ZipArchive::new(Cursor::new(original.clone())).unwrap();
    let mut target = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut file = source.by_index(i).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        let text = text.replace("<itemref idref=\"c1\"/>", "");
        let name = file.name().unwrap().into_owned();
        target
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        target.write_all(text.as_bytes()).unwrap();
    }
    let rewritten = target.finish().unwrap().into_inner();

    assert_ne!(digest(&codec, &original), digest(&codec, &rewritten));
}

#[test]
fn something_that_is_not_the_document_its_name_says_is_refused() {
    assert!(is_format(&try_read(
        &OpenXmlCodec,
        b"an old binary .doc, or a protected file"
    )));
    assert!(is_format(&try_read(&OpenXmlCodec, &fx::epub())));
    assert!(is_format(&try_read(&EpubCodec, &fx::docx())));
    assert!(is_format(&try_read(&EpubCodec, &[])));
}

#[test]
fn every_extension_has_a_codec_and_every_codec_offers_only_real_fields() {
    for extension in MetadataCodecs::extensions() {
        let codec = MetadataCodecs::for_path(&format!("x{extension}")).expect(extension);
        assert!(!codec.fields().is_empty(), "{extension}");
        for field in codec.fields() {
            assert_eq!(*field, MetadataFields::get(*field).field, "{extension}");
        }
    }

    let id = |path: &str| MetadataCodecs::for_path(path).map(|c| c.id());
    assert_eq!(Some("png"), id(r"C:\x\a.PNG"));
    assert_eq!(Some("webp"), id(r"C:\x\a.webp"));
    assert_eq!(Some("openxml"), id(r"C:\x\a.docx"));
    assert_eq!(Some("openxml"), id(r"C:\x\a.xlsm"));
    assert_eq!(Some("epub"), id(r"C:\x\a.epub"));
}
