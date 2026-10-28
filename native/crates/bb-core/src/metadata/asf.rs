//! The tags of an ASF container (WMA): the Content Description and Extended Content Description
//! objects of the header, read and rewritten here because the audio tag library has no ASF
//! support. Port of the ASF half of Core/Services/Metadata/AudioTagCodec.cs (where the library
//! did the work) over the object walk of AudioPayload.cs.
//!
//! The header object is rebuilt with those two objects replaced and every other sub-object kept as
//! it was; the data object and everything after it are copied through untouched, which is exactly
//! the range `AudioPayload` hashes. The file-size field of the File Properties object is brought
//! up to date, since a header that grew moves everything behind it.

use std::collections::BTreeMap;
use std::io::SeekFrom;

use super::audio_payload::{ASF_HEADER_OBJECT, asf_data_offset};
use super::codec::{
    Destination, MetadataError, MetadataResult, Source, copy_to_end, read_at, read_fully,
};
use super::exif_fields::utf16;
use super::fields::MetadataField;
use crate::cancel::CancelToken;

const CONTENT_DESCRIPTION: [u8; 16] = [
    0x33, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const EXTENDED_CONTENT_DESCRIPTION: [u8; 16] = [
    0x40, 0xA4, 0xD0, 0xD2, 0x07, 0xE3, 0xD2, 0x11, 0x97, 0xF0, 0x00, 0x0F, 0x8C, 0x2C, 0xB4, 0xD5,
];
const FILE_PROPERTIES: [u8; 16] = [
    0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];

/// The most a header is allowed to be before it is refused: a tag block of this size is not a
/// song's.
const MAX_HEADER: u64 = 64 << 20;

/// The descriptor each field is kept in. The Content Description object holds the first five;
/// the rest are named descriptors of the extended object, as every Windows writer names them.
fn descriptor(field: MetadataField) -> Option<&'static str> {
    Some(match field {
        MetadataField::Album => "WM/AlbumTitle",
        MetadataField::AlbumArtist => "WM/AlbumArtist",
        MetadataField::TrackNumber => "WM/TrackNumber",
        MetadataField::DiscNumber => "WM/PartOfSet",
        MetadataField::Year => "WM/Year",
        MetadataField::Genre => "WM/Genre",
        MetadataField::Composer => "WM/Composer",
        MetadataField::Conductor => "WM/Conductor",
        MetadataField::Bpm => "WM/BeatsPerMinute",
        MetadataField::Lyrics => "WM/Lyrics",
        MetadataField::Publisher => "WM/Publisher",
        _ => return None,
    })
}

/// What an ASF file's header says about the song.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AsfTags {
    /// Text values by descriptor name (`WM/AlbumTitle`), plus the five content-description
    /// fields under `Title`, `Author`, `Copyright`, `Description` and `Rating`.
    pub values: BTreeMap<String, String>,
    /// The bytes of the first `WM/Picture`, as the image file they are.
    pub picture: Option<Vec<u8>>,
}

impl AsfTags {
    fn get(&self, name: &str) -> &str {
        self.values.get(name).map(String::as_str).unwrap_or("")
    }

    fn set(&mut self, name: &str, value: &str) {
        if value.is_empty() {
            self.values.remove(name);
        } else {
            self.values.insert(name.to_string(), value.to_string());
        }
    }

    /// The fields, as the codec's vocabulary. A number is reported only above zero; a total only
    /// beside its number, as `3/12`.
    pub fn fields(&self) -> BTreeMap<MetadataField, String> {
        let mut out = BTreeMap::new();
        let mut text = |field: MetadataField, value: &str| {
            let value = value.trim();
            if !value.is_empty() {
                out.insert(field, value.replace("\r\n", "\n").replace('\r', "\n"));
            }
        };
        text(MetadataField::Title, self.get("Title"));
        text(MetadataField::Artist, self.get("Author"));
        text(MetadataField::Copyright, self.get("Copyright"));
        text(MetadataField::Comment, self.get("Description"));
        for field in [
            MetadataField::Album,
            MetadataField::AlbumArtist,
            MetadataField::Genre,
            MetadataField::Composer,
            MetadataField::Conductor,
            MetadataField::Lyrics,
            MetadataField::Publisher,
        ] {
            text(field, self.get(descriptor(field).expect("mapped")));
        }
        for (field, total) in [
            (MetadataField::TrackNumber, MetadataField::TrackTotal),
            (MetadataField::DiscNumber, MetadataField::DiscTotal),
        ] {
            let raw = self.get(descriptor(field).expect("mapped"));
            let (number, rest) = raw.split_once('/').unwrap_or((raw, ""));
            if let Some(n) = positive(number) {
                out.insert(field, n.to_string());
                if let Some(t) = positive(rest) {
                    out.insert(total, t.to_string());
                }
            }
        }
        for field in [MetadataField::Year, MetadataField::Bpm] {
            if let Some(n) = positive(self.get(descriptor(field).expect("mapped"))) {
                out.insert(field, n.to_string());
            }
        }
        out
    }

    /// Applies one field's canonical value: empty clears. A total is folded into its number's
    /// `n/total` form, and `Format` names a total with no number to go with it.
    pub fn apply(&mut self, field: MetadataField, value: &str) -> MetadataResult<()> {
        match field {
            MetadataField::Title => self.set("Title", value),
            MetadataField::Artist => self.set("Author", value),
            MetadataField::Copyright => self.set("Copyright", value),
            MetadataField::Comment => self.set("Description", value),
            MetadataField::TrackNumber | MetadataField::TrackTotal => {
                self.pair("WM/TrackNumber", field == MetadataField::TrackTotal, value)
            }
            MetadataField::DiscNumber | MetadataField::DiscTotal => {
                self.pair("WM/PartOfSet", field == MetadataField::DiscTotal, value)
            }
            _ => {
                if let Some(name) = descriptor(field) {
                    self.set(name, value);
                }
            }
        }
        Ok(())
    }

    fn pair(&mut self, name: &str, total: bool, value: &str) {
        let raw = self.get(name).to_string();
        let (number, rest) = raw.split_once('/').unwrap_or((raw.as_str(), ""));
        let (mut number, mut rest) = (
            positive(number).map(|n| n.to_string()).unwrap_or_default(),
            positive(rest).map(|n| n.to_string()).unwrap_or_default(),
        );
        if total {
            rest = value.to_string();
        } else {
            number = value.to_string();
        }
        let joined = if rest.is_empty() {
            number
        } else {
            format!("{number}/{rest}")
        };
        self.set(name, &joined);
    }

    /// Whether a total is stated with no number to carry it.
    pub fn orphaned_total(&self, name: &str) -> bool {
        let raw = self.get(name);
        match raw.split_once('/') {
            Some((number, rest)) => positive(rest).is_some() && positive(number).is_none(),
            None => false,
        }
    }
}

fn positive(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok().filter(|&n| n > 0)
}

fn not_audio() -> MetadataError {
    MetadataError::format("This is not a readable audio file, so it was left as it is.")
}

/// A header sub-object: its GUID, and its content after the 24-byte object header.
struct Object {
    guid: [u8; 16],
    data: Vec<u8>,
}

struct Header {
    objects: Vec<Object>,
    data_offset: u64,
}

fn read_header(source: &mut dyn Source) -> MetadataResult<Header> {
    let data_offset = asf_data_offset(source)?;
    if data_offset > MAX_HEADER {
        return Err(not_audio());
    }
    let mut header = vec![0u8; data_offset as usize];
    source.seek(SeekFrom::Start(0))?;
    if read_fully(source, &mut header)? != header.len() || header.len() < 30 {
        return Err(not_audio());
    }
    if header[..16] != ASF_HEADER_OBJECT {
        return Err(not_audio());
    }
    let size = u64::from_le_bytes(header[16..24].try_into().expect("8 bytes"));
    if size < 30 || size > data_offset {
        return Err(not_audio());
    }

    let mut objects = Vec::new();
    let mut at = 30usize;
    let end = size as usize;
    while at + 24 <= end {
        let guid: [u8; 16] = header[at..at + 16].try_into().expect("16 bytes");
        let length = u64::from_le_bytes(header[at + 16..at + 24].try_into().expect("8 bytes"));
        if length < 24 || at as u64 + length > end as u64 {
            return Err(not_audio());
        }
        objects.push(Object {
            guid,
            data: header[at + 24..at + length as usize].to_vec(),
        });
        at += length as usize;
    }

    Ok(Header {
        objects,
        data_offset,
    })
}

fn wide_string(bytes: &[u8]) -> String {
    let text = utf16(&bytes[..bytes.len() & !1], true);
    text.trim_end_matches('\0').to_string()
}

fn to_wide(text: &str) -> Vec<u8> {
    let mut out: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

fn parse_tags(header: &Header) -> AsfTags {
    let mut tags = AsfTags::default();

    for object in &header.objects {
        if object.guid == CONTENT_DESCRIPTION {
            let d = &object.data;
            if d.len() < 10 {
                continue;
            }
            let lengths: Vec<usize> = (0..5)
                .map(|i| u16::from_le_bytes([d[i * 2], d[i * 2 + 1]]) as usize)
                .collect();
            let mut at = 10;
            for (name, length) in ["Title", "Author", "Copyright", "Description", "Rating"]
                .iter()
                .zip(lengths)
            {
                if at + length > d.len() {
                    break;
                }
                tags.set(name, &wide_string(&d[at..at + length]));
                at += length;
            }
        } else if object.guid == EXTENDED_CONTENT_DESCRIPTION {
            let d = &object.data;
            if d.len() < 2 {
                continue;
            }
            let count = u16::from_le_bytes([d[0], d[1]]) as usize;
            let mut at = 2;
            for _ in 0..count {
                if at + 2 > d.len() {
                    break;
                }
                let name_length = u16::from_le_bytes([d[at], d[at + 1]]) as usize;
                at += 2;
                if at + name_length + 4 > d.len() {
                    break;
                }
                let name = wide_string(&d[at..at + name_length]);
                at += name_length;
                let kind = u16::from_le_bytes([d[at], d[at + 1]]);
                let length = u16::from_le_bytes([d[at + 2], d[at + 3]]) as usize;
                at += 4;
                if at + length > d.len() {
                    break;
                }
                let value = &d[at..at + length];
                at += length;

                match (name.as_str(), kind) {
                    ("WM/Picture", 1) => {
                        if tags.picture.is_none() {
                            tags.picture = picture_data(value);
                        }
                    }
                    (_, 0) => tags.set(&name, &wide_string(value)),
                    (_, 3) if value.len() >= 4 => {
                        let n = u32::from_le_bytes(value[..4].try_into().expect("4 bytes"));
                        tags.set(&name, &n.to_string());
                    }
                    (_, 4) if value.len() >= 8 => {
                        let n = u64::from_le_bytes(value[..8].try_into().expect("8 bytes"));
                        tags.set(&name, &n.to_string());
                    }
                    (_, 5) if value.len() >= 2 => {
                        let n = u16::from_le_bytes([value[0], value[1]]);
                        tags.set(&name, &n.to_string());
                    }
                    _ => {}
                }
            }
        }
    }

    tags
}

/// The image inside a `WM/Picture` value: a type byte, a length, a MIME type and a description
/// as NUL-terminated wide strings, then the bytes.
fn picture_data(value: &[u8]) -> Option<Vec<u8>> {
    if value.len() < 5 {
        return None;
    }
    let length = u32::from_le_bytes([value[1], value[2], value[3], value[4]]) as usize;
    let mut at = 5;
    for _ in 0..2 {
        loop {
            if at + 2 > value.len() {
                return None;
            }
            at += 2;
            if value[at - 2] == 0 && value[at - 1] == 0 {
                break;
            }
        }
    }
    if at + length > value.len() || length == 0 {
        return None;
    }
    Some(value[at..at + length].to_vec())
}

fn picture_value(picture: &[u8]) -> Vec<u8> {
    let mime = if picture.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    let mut out = vec![3u8]; // front cover
    out.extend_from_slice(&(picture.len() as u32).to_le_bytes());
    out.extend_from_slice(&to_wide(mime));
    out.extend_from_slice(&to_wide(""));
    out.extend_from_slice(picture);
    out
}

fn content_description(tags: &AsfTags) -> Option<Object> {
    let names = ["Title", "Author", "Copyright", "Description", "Rating"];
    if names.iter().all(|n| tags.get(n).is_empty()) {
        return None;
    }
    let values: Vec<Vec<u8>> = names.iter().map(|n| to_wide(tags.get(n))).collect();
    let mut data = Vec::new();
    for v in &values {
        data.extend_from_slice(&(v.len() as u16).to_le_bytes());
    }
    for v in &values {
        data.extend_from_slice(v);
    }
    Some(Object {
        guid: CONTENT_DESCRIPTION,
        data,
    })
}

fn extended_content_description(tags: &AsfTags) -> MetadataResult<Option<Object>> {
    let fixed = ["Title", "Author", "Copyright", "Description", "Rating"];
    let mut descriptors: Vec<(String, u16, Vec<u8>)> = tags
        .values
        .iter()
        .filter(|(name, _)| !fixed.contains(&name.as_str()))
        .map(|(name, value)| (name.clone(), 0u16, to_wide(value)))
        .collect();
    if let Some(picture) = &tags.picture {
        descriptors.push(("WM/Picture".to_string(), 1, picture_value(picture)));
    }
    if descriptors.is_empty() {
        return Ok(None);
    }

    let mut data = Vec::new();
    data.extend_from_slice(&(descriptors.len() as u16).to_le_bytes());
    for (name, kind, value) in descriptors {
        let name = to_wide(&name);
        if value.len() > u16::MAX as usize {
            return Err(MetadataError::format(
                "That is too large to be stored in this file's tags.",
            ));
        }
        data.extend_from_slice(&(name.len() as u16).to_le_bytes());
        data.extend_from_slice(&name);
        data.extend_from_slice(&kind.to_le_bytes());
        data.extend_from_slice(&(value.len() as u16).to_le_bytes());
        data.extend_from_slice(&value);
    }
    Ok(Some(Object {
        guid: EXTENDED_CONTENT_DESCRIPTION,
        data,
    }))
}

/// What the file's header holds.
pub fn read(source: &mut dyn Source) -> MetadataResult<AsfTags> {
    Ok(parse_tags(&read_header(source)?))
}

/// `source` written to `destination` with its tags replaced by `tags`; the data object and
/// everything after it are copied through as they are.
pub fn rewrite(
    source: &mut dyn Source,
    destination: &mut dyn Destination,
    tags: &AsfTags,
    cancel: &CancelToken,
) -> MetadataResult<()> {
    let header = read_header(source)?;
    let total = {
        let mut probe = [0u8; 1];
        let end = super::codec::stream_len(source)?;
        read_at(source, end.saturating_sub(1), &mut probe)?;
        end
    };

    let mut objects: Vec<Object> = header
        .objects
        .into_iter()
        .filter(|o| o.guid != CONTENT_DESCRIPTION && o.guid != EXTENDED_CONTENT_DESCRIPTION)
        .collect();
    if let Some(cd) = content_description(tags) {
        objects.push(cd);
    }
    if let Some(ecd) = extended_content_description(tags)? {
        objects.push(ecd);
    }

    let header_size: u64 = 30
        + objects
            .iter()
            .map(|o| 24 + o.data.len() as u64)
            .sum::<u64>();
    let new_total = header_size + (total - header.data_offset);

    // The File Properties object states the file's size, which has just changed.
    for object in &mut objects {
        if object.guid == FILE_PROPERTIES && object.data.len() >= 24 {
            object.data[16..24].copy_from_slice(&new_total.to_le_bytes());
        }
    }

    cancel.check()?;
    destination.write_all(&ASF_HEADER_OBJECT)?;
    destination.write_all(&header_size.to_le_bytes())?;
    destination.write_all(&(objects.len() as u32).to_le_bytes())?;
    destination.write_all(&[0x01, 0x02])?;
    for object in &objects {
        destination.write_all(&object.guid)?;
        destination.write_all(&(24 + object.data.len() as u64).to_le_bytes())?;
        destination.write_all(&object.data)?;
    }

    copy_to_end(source, destination, header.data_offset, cancel)
}
