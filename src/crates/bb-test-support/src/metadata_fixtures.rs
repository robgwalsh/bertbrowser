//! Small real files for the metadata tests to edit. Port of
//! src/BertBrowser.Core/Services/Metadata/MetadataFixtures.cs and MetadataFixtures.Media.cs.
//!
//! **None of them is written by the code under test.** A codec that reads back its own output
//! proves only that it agrees with itself, so the pictures come from Windows' own encoder, the
//! media containers from Windows' media pipeline, and the MP3, WAV, AIFF and Ogg files are laid out
//! here byte by byte from their specifications. The Base64 constants live in
//! `metadata_fixtures_data.rs`, copied verbatim from the C# so every shipped byte stays visible.

use std::io::Write;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use zip::write::SimpleFileOptions;

pub use crate::metadata_fixtures_data::*;

/// The MakerNote `CAMERA_JPEG` carries, to look for after an edit.
pub fn maker_note() -> Vec<u8> {
    (0..48u8).map(|i| 0xA0 + i % 16).collect()
}

pub fn jpeg(base64: &str) -> Vec<u8> {
    bytes(base64)
}

pub fn bytes(base64: &str) -> Vec<u8> {
    STANDARD.decode(base64).expect("fixture is valid Base64")
}

/// Twenty silent MPEG-1 Layer III frames (128 kbit/s, 44.1 kHz) behind an ID3v2.3 tag.
/// `frames` are frame id and text, e.g. `("TIT2", "Song")`; text that is not Latin-1 is written as
/// UTF-16 with a byte-order mark, as the specification asks. `with_v1` also appends an ID3v1 tag,
/// so there is one at each end.
pub fn mp3(frames: &[(&str, &str)], with_v1: bool) -> Vec<u8> {
    let mut body = Vec::new();
    for (id, text) in frames {
        let data: Vec<u8> = if text.chars().all(|c| (c as u32) <= 0xFF) {
            let mut d = vec![0u8];
            d.extend(text.chars().map(|c| c as u32 as u8));
            d
        } else {
            let mut d = vec![1u8, 0xFF, 0xFE];
            for unit in text.encode_utf16() {
                d.extend_from_slice(&unit.to_le_bytes());
            }
            d
        };
        body.extend_from_slice(id.as_bytes());
        body.extend_from_slice(&big_endian(data.len(), 4));
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&data);
    }

    // Padding, which every real tagger leaves and which the tag's size includes.
    body.extend_from_slice(&[0u8; 64]);

    let mut file = vec![b'I', b'D', b'3', 3, 0, 0];
    file.extend_from_slice(&syncsafe(body.len()));
    file.extend_from_slice(&body);
    file.extend_from_slice(&audio());

    if with_v1 {
        let mut v1 = [0u8; 128];
        v1[..3].copy_from_slice(b"TAG");
        v1[3..3 + 12].copy_from_slice(b"Old v1 title");
        v1[127] = 255;
        file.extend_from_slice(&v1);
    }

    file
}

/// The same sound behind an ID3v2.2 tag: three-letter frames, the oldest layout still found in
/// the wild, which a writer has to read and may not write.
pub fn mp3_v22(title: &str, artist: &str) -> Vec<u8> {
    let mut body = Vec::new();
    for (id, text) in [("TT2", title), ("TP1", artist)] {
        let mut data = vec![0u8];
        data.extend(text.chars().map(|c| c as u32 as u8));
        body.extend_from_slice(id.as_bytes());
        body.extend_from_slice(&big_endian(data.len(), 3));
        body.extend_from_slice(&data);
    }

    let mut file = vec![b'I', b'D', b'3', 2, 0, 0];
    file.extend_from_slice(&syncsafe(body.len()));
    file.extend_from_slice(&body);
    file.extend_from_slice(&audio());
    file
}

/// The frames alone, with no tag at either end.
pub fn audio() -> Vec<u8> {
    // 144 * 128000 / 44100, unpadded. A header and then zeros is a valid frame of silence.
    const FRAME_LENGTH: usize = 417;
    let mut audio = vec![0u8; FRAME_LENGTH * 20];
    for i in 0..20 {
        audio[i * FRAME_LENGTH] = 0xFF;
        audio[i * FRAME_LENGTH + 1] = 0xFB;
        audio[i * FRAME_LENGTH + 2] = 0x90;
        audio[i * FRAME_LENGTH + 3] = 0x64;
    }
    audio
}

/// A tenth of a second of 8 kHz mono 16-bit PCM in a WAV, with no tags: the format is forty-four
/// bytes of header and then the samples, so it is simply written out.
pub fn wav() -> Vec<u8> {
    let mut samples = vec![0u8; 1600];
    for i in (0..samples.len()).step_by(2) {
        samples[i] = (i % 200) as u8;
    }

    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + samples.len() as i32).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16i32.to_le_bytes());
    file.extend_from_slice(&1i16.to_le_bytes());
    file.extend_from_slice(&1i16.to_le_bytes());
    file.extend_from_slice(&8000i32.to_le_bytes());
    file.extend_from_slice(&16000i32.to_le_bytes());
    file.extend_from_slice(&2i16.to_le_bytes());
    file.extend_from_slice(&16i16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(samples.len() as i32).to_le_bytes());
    file.extend_from_slice(&samples);
    file
}

/// A Word document reduced to the parts a reader needs: content types, relationships, a
/// one-paragraph body, and core properties with a title ("Quarterly report"), an author
/// ("A. Writer"), keywords and who last saved it. Laid out by hand from the Open Packaging
/// Conventions; zipped by the zip crate, which the codec does not write with differently, so the
/// properties part is what is under test, not the container.
pub fn docx() -> Vec<u8> {
    zip_of(&[
        (
            "[Content_Types].xml",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
                "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>",
                "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
                "<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>"
            ),
        ),
        (
            "_rels/.rels",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
                "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>",
                "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>"
            ),
        ),
        (
            "word/document.xml",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
                "<w:body><w:p><w:r><w:t>The body of the document, which no edit may touch.</w:t></w:r></w:p></w:body></w:document>"
            ),
        ),
        (
            "docProps/core.xml",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" ",
                "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
                "<dc:title>Quarterly report</dc:title><dc:creator>A. Writer</dc:creator><cp:keywords>finance; q3</cp:keywords>",
                "<cp:lastModifiedBy>A. Writer</cp:lastModifiedBy><cp:revision>4</cp:revision>",
                "<dcterms:created xsi:type=\"dcterms:W3CDTF\">2024-03-14T09:26:53Z</dcterms:created></cp:coreProperties>"
            ),
        ),
    ])
}

/// A one-chapter EPUB: the stored `mimetype` entry first, a container file pointing at a package
/// document that is *not* at a fixed path, and a title, an author and two subjects in its
/// metadata. Laid out by hand from the EPUB specification.
pub fn epub() -> Vec<u8> {
    zip_of(&[
        ("mimetype", "application/epub+zip"),
        (
            "META-INF/container.xml",
            concat!(
                "<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">",
                "<rootfiles><rootfile full-path=\"book/package.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>"
            ),
        ),
        (
            "book/package.opf",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"id\">",
                "<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"id\">urn:uuid:0</dc:identifier>",
                "<dc:title>A Short Book</dc:title><dc:creator>Some Author</dc:creator><dc:language>en</dc:language>",
                "<dc:subject>Fiction</dc:subject><dc:subject>Short stories</dc:subject></metadata>",
                "<manifest><item id=\"c1\" href=\"chapter1.xhtml\" media-type=\"application/xhtml+xml\"/></manifest>",
                "<spine><itemref idref=\"c1\"/></spine></package>"
            ),
        ),
        (
            "book/chapter1.xhtml",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>One</title></head>",
                "<body><p>The only chapter, which no edit may touch.</p></body></html>"
            ),
        ),
    ])
}

/// A zip of text entries, `mimetype` stored and the rest deflated, every entry dated
/// 2024-03-14 09:26:52.
pub fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
    let binary: Vec<(&str, &[u8])> = entries.iter().map(|(n, c)| (*n, c.as_bytes())).collect();
    zip_of_bytes(&binary)
}

/// As `zip_of`, for entries that are not text (a picture, another zip).
pub fn zip_of_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in entries {
        let method = if *name == "mimetype" {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        };
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .last_modified_time(
                zip::DateTime::from_date_and_time(2024, 3, 14, 9, 26, 52).expect("a valid date"),
            );
        writer.start_file(*name, options).expect("start entry");
        writer.write_all(content).expect("write entry");
    }
    writer.finish().expect("finish zip").into_inner()
}

/// A tenth of a second of 8 kHz mono 16-bit PCM as an AIFF with no tags: a form header, the
/// common chunk with its 80-bit sample rate, and the sound.
pub fn aiff() -> Vec<u8> {
    let mut samples = vec![0u8; 1600];
    for i in (1..samples.len()).step_by(2) {
        samples[i] = (i % 200) as u8;
    }

    let mut common = vec![0u8, 1]; // one channel
    common.extend_from_slice(&big_endian(samples.len() / 2, 4)); // sample frames
    common.extend_from_slice(&[0, 16]); // bits per sample
    common.extend_from_slice(&[0x40, 0x0B, 0xFA, 0, 0, 0, 0, 0, 0, 0]); // 8000.0 as an 80-bit float
    let mut sound = vec![0u8; 8];
    sound.extend_from_slice(&samples);

    let mut file = Vec::new();
    file.extend_from_slice(b"FORM");
    file.extend_from_slice(&big_endian(4 + 8 + common.len() + 8 + sound.len(), 4));
    file.extend_from_slice(b"AIFF");
    file.extend_from_slice(b"COMM");
    file.extend_from_slice(&big_endian(common.len(), 4));
    file.extend_from_slice(&common);
    file.extend_from_slice(b"SSND");
    file.extend_from_slice(&big_endian(sound.len(), 4));
    file.extend_from_slice(&sound);
    file
}

/// A fifth of a second of silence as Opus in Ogg: the identification packet, an empty comment
/// packet, and ten 20 ms frames, on three pages with true checksums. Laid out by hand from
/// RFC 3533 and RFC 7845; `F8 FF FE` is the codec's own encoding of a silent frame.
pub fn opus() -> Vec<u8> {
    let mut head = b"OpusHead".to_vec();
    head.extend_from_slice(&[1, 1, 0x38, 0x01, 0x80, 0xBB, 0, 0, 0, 0, 0]);
    let mut tags = b"OpusTags".to_vec();
    tags.extend_from_slice(&[4, 0, 0, 0]);
    tags.extend_from_slice(b"hand");
    tags.extend_from_slice(&[0, 0, 0, 0]);
    let frame: Vec<u8> = vec![0xF8, 0xFF, 0xFE];
    let frames: Vec<Vec<u8>> = (0..10).map(|_| frame.clone()).collect();

    let mut file = ogg_page(0x02, 0, 0, &[head]);
    file.extend(ogg_page(0x00, 0, 1, &[tags]));
    file.extend(ogg_page(0x04, 960 * 10 + 312, 2, &frames));
    file
}

fn ogg_page(flags: u8, granule: i64, sequence: i32, packets: &[Vec<u8>]) -> Vec<u8> {
    let mut table = Vec::new();
    for packet in packets {
        let mut left = packet.len();
        while left >= 255 {
            table.push(255u8);
            left -= 255;
        }
        table.push((packet.len() % 255) as u8);
    }

    let mut page = Vec::new();
    page.extend_from_slice(b"OggS");
    page.push(0);
    page.push(flags);
    page.extend_from_slice(&granule.to_le_bytes());
    page.extend_from_slice(&0x1234ABCDi32.to_le_bytes());
    page.extend_from_slice(&sequence.to_le_bytes());
    page.extend_from_slice(&[0, 0, 0, 0]);
    page.push(table.len() as u8);
    page.extend_from_slice(&table);
    for packet in packets {
        page.extend_from_slice(packet);
    }

    // Ogg's checksum: the CRC-32 polynomial run the other way round from zip's, from zero.
    let mut crc: u32 = 0;
    for &b in &page {
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
    }
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    page
}

fn syncsafe(value: usize) -> [u8; 4] {
    [
        ((value >> 21) & 0x7F) as u8,
        ((value >> 14) & 0x7F) as u8,
        ((value >> 7) & 0x7F) as u8,
        (value & 0x7F) as u8,
    ]
}

fn big_endian(value: usize, width: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; width];
    for i in 0..width {
        bytes[width - 1 - i] = (value >> (8 * i)) as u8;
    }
    bytes
}
