//! Which EXIF tags each field is read from and written to. Spec: Core/Services/Metadata/ExifFields.cs.
//!
//! **A text field is written twice, on purpose.** The standard tags (`ImageDescription`, `Artist`)
//! are typed ASCII and are what cameras and most software read; the `XP*` tags are UTF-16 and are
//! what Windows reads first. Writing one and not the other leaves two programs disagreeing about a
//! picture's title, so both are kept in step and the UTF-16 one wins on read.

use std::collections::BTreeMap;

use super::codec::{MetadataError, MetadataResult};
use super::exif::{
    ExifBlock, ExifDirectory, TYPE_ASCII, TYPE_BYTE, TYPE_RATIONAL, TYPE_SHORT, TYPE_UNDEFINED,
};
use super::fields::{MetadataField, MetadataFields, round_half_even};
use super::models::MetadataEdit;

const IMAGE_DESCRIPTION: u16 = 0x010E;
const MAKE: u16 = 0x010F;
const MODEL: u16 = 0x0110;
const ORIENTATION: u16 = 0x0112;
const ARTIST: u16 = 0x013B;
const RATING: u16 = 0x4746;
const RATING_PERCENT: u16 = 0x4749;
const COPYRIGHT: u16 = 0x8298;
const XP_TITLE: u16 = 0x9C9B;
const XP_COMMENT: u16 = 0x9C9C;
const XP_AUTHOR: u16 = 0x9C9D;
const XP_KEYWORDS: u16 = 0x9C9E;
const XP_SUBJECT: u16 = 0x9C9F;
const DATE_TIME_ORIGINAL: u16 = 0x9003;
const USER_COMMENT: u16 = 0x9286;
const EXPOSURE_TIME: u16 = 0x829A;
const F_NUMBER: u16 = 0x829D;
const ISO_SPEED: u16 = 0x8827;
const FOCAL_LENGTH: u16 = 0x920A;
const LENS_MODEL: u16 = 0xA434;

/// Every field EXIF has a tag for — the same set whichever picture format carries it.
pub const FIELDS: [MetadataField; 15] = [
    MetadataField::Title,
    MetadataField::Author,
    MetadataField::Subject,
    MetadataField::Comment,
    MetadataField::Copyright,
    MetadataField::CameraMake,
    MetadataField::CameraModel,
    MetadataField::Keywords,
    MetadataField::Rating,
    MetadataField::DateTaken,
    MetadataField::LensModel,
    MetadataField::Iso,
    MetadataField::Aperture,
    MetadataField::ExposureTime,
    MetadataField::FocalLength,
];

pub struct ExifFields;

impl ExifFields {
    pub fn read(block: &ExifBlock) -> BTreeMap<MetadataField, String> {
        let mut values = BTreeMap::new();
        let mut put = |field: MetadataField, value: Option<String>| {
            if let Some(v) = value {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    values.insert(field, trimmed.to_string());
                }
            }
        };

        put(
            MetadataField::Title,
            xp(block, XP_TITLE).or_else(|| ascii(block, ExifDirectory::Image, IMAGE_DESCRIPTION)),
        );
        put(
            MetadataField::Author,
            xp(block, XP_AUTHOR).or_else(|| ascii(block, ExifDirectory::Image, ARTIST)),
        );
        put(MetadataField::Subject, xp(block, XP_SUBJECT));
        put(
            MetadataField::Comment,
            read_user_comment(block).or_else(|| xp(block, XP_COMMENT)),
        );
        put(
            MetadataField::Copyright,
            ascii(block, ExifDirectory::Image, COPYRIGHT),
        );
        put(
            MetadataField::CameraMake,
            ascii(block, ExifDirectory::Image, MAKE),
        );
        put(
            MetadataField::CameraModel,
            ascii(block, ExifDirectory::Image, MODEL),
        );
        put(
            MetadataField::LensModel,
            ascii(block, ExifDirectory::Photo, LENS_MODEL),
        );

        if let Some(iso) = block.get(ExifDirectory::Photo, ISO_SPEED)
            && iso.len() >= 2
            && block.type_of(ExifDirectory::Photo, ISO_SPEED) == Some(TYPE_SHORT)
            && block.read_u16(&iso) > 0
        {
            put(MetadataField::Iso, Some(block.read_u16(&iso).to_string()));
        }

        if let Some((top, bottom)) = rational(block, F_NUMBER) {
            put(MetadataField::Aperture, Some(tenths(top, bottom)));
        }
        if let Some((top, bottom)) = rational(block, FOCAL_LENGTH) {
            put(MetadataField::FocalLength, Some(tenths(top, bottom)));
        }
        if let Some((top, bottom)) = rational(block, EXPOSURE_TIME) {
            put(
                MetadataField::ExposureTime,
                Some(MetadataFields::format_exposure(top, bottom)),
            );
        }

        if let Some(keywords) = xp(block, XP_KEYWORDS) {
            put(
                MetadataField::Keywords,
                Some(MetadataFields::split_list(&keywords).join(MetadataFields::LIST_SEPARATOR)),
            );
        }

        if let Some(rating) = block.get(ExifDirectory::Image, RATING)
            && rating.len() >= 2
            && block.type_of(ExifDirectory::Image, RATING) == Some(TYPE_SHORT)
        {
            let stars = block.read_u16(&rating);
            if (1..=5).contains(&stars) {
                put(MetadataField::Rating, Some(stars.to_string()));
            }
        }

        if let Some(taken) = ascii(block, ExifDirectory::Photo, DATE_TIME_ORIGINAL)
            && let Some(date) = parse_exif_date(&taken)
        {
            put(
                MetadataField::DateTaken,
                Some(MetadataFields::format_date(date)),
            );
        }

        values
    }

    pub fn apply(block: &mut ExifBlock, edit: &MetadataEdit) -> MetadataResult<()> {
        for (field, value) in &edit.changes {
            match field {
                MetadataField::Title => {
                    set_ascii(block, ExifDirectory::Image, IMAGE_DESCRIPTION, value);
                    set_xp(block, XP_TITLE, value);
                }
                MetadataField::Author => {
                    set_ascii(block, ExifDirectory::Image, ARTIST, value);
                    set_xp(block, XP_AUTHOR, value);
                }
                MetadataField::Subject => set_xp(block, XP_SUBJECT, value),
                MetadataField::Comment => {
                    set_xp(block, XP_COMMENT, value);
                    set_user_comment(block, value);
                }
                MetadataField::Copyright => {
                    set_ascii(block, ExifDirectory::Image, COPYRIGHT, value)
                }
                MetadataField::CameraMake => set_ascii(block, ExifDirectory::Image, MAKE, value),
                MetadataField::CameraModel => set_ascii(block, ExifDirectory::Image, MODEL, value),
                MetadataField::LensModel => {
                    set_ascii(block, ExifDirectory::Photo, LENS_MODEL, value);
                }
                MetadataField::Iso => {
                    let speed = value
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                        .then(|| value.parse::<u64>().ok())
                        .flatten();
                    match speed {
                        Some(speed) if speed > 0 && speed <= u16::MAX as u64 => {
                            let bytes = block.write_u16(speed as u16);
                            block.set(ExifDirectory::Photo, ISO_SPEED, TYPE_SHORT, bytes);
                        }
                        _ if value.is_empty() => block.remove(ExifDirectory::Photo, ISO_SPEED),
                        _ => {
                            return Err(MetadataError::format(
                                "An ISO above 65,535 cannot be stored in this tag.",
                            ));
                        }
                    }
                }
                MetadataField::Aperture => set_tenths(block, F_NUMBER, value),
                MetadataField::FocalLength => set_tenths(block, FOCAL_LENGTH, value),
                MetadataField::ExposureTime => match MetadataFields::try_parse_exposure(value) {
                    Some((top, bottom)) => set_rational(block, EXPOSURE_TIME, top, bottom),
                    None => block.remove(ExifDirectory::Photo, EXPOSURE_TIME),
                },
                MetadataField::Keywords => {
                    // Windows separates these with a bare semicolon, and reads nothing else back.
                    set_xp(
                        block,
                        XP_KEYWORDS,
                        &MetadataFields::split_list(value).join(";"),
                    );
                }
                MetadataField::Rating => set_rating(block, value),
                MetadataField::DateTaken => {
                    let text = match MetadataFields::try_parse_date(value) {
                        Some(date) => format_exif_date(date),
                        None => String::new(),
                    };
                    set_ascii(block, ExifDirectory::Photo, DATE_TIME_ORIGINAL, &text);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Which way up the picture is drawn. Not metadata in the sense the rest is: take it away
    /// and a portrait photograph lies on its side.
    pub fn orientation(block: &ExifBlock) -> Option<u16> {
        let raw = block.get(ExifDirectory::Image, ORIENTATION)?;
        if raw.len() >= 2 && block.type_of(ExifDirectory::Image, ORIENTATION) == Some(TYPE_SHORT) {
            Some(block.read_u16(&raw))
        } else {
            None
        }
    }

    pub fn set_orientation(block: &mut ExifBlock, orientation: u16) {
        let bytes = block.write_u16(orientation);
        block.set(ExifDirectory::Image, ORIENTATION, TYPE_SHORT, bytes);
    }
}

/// `yyyy:MM:dd HH:mm:ss` with a year above one, as wall-clock ticks.
fn parse_exif_date(text: &str) -> Option<crate::FileTime> {
    let b = text.as_bytes();
    if b.len() != 19 || b[4] != b':' || b[7] != b':' || b[10] != b' ' {
        return None;
    }
    let date = MetadataFields::try_parse_date(text)?;
    (date.civil().year > 1).then_some(date)
}

fn format_exif_date(date: crate::FileTime) -> String {
    let c = date.civil();
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}

fn tenths(top: u32, bottom: u32) -> String {
    MetadataFields::format_decimal(round_half_even((top as f64 / bottom as f64) * 10.0) / 10.0)
}

/// A fraction: two unsigned numbers, the second never zero for a value worth reading.
fn rational(block: &ExifBlock, tag: u16) -> Option<(u32, u32)> {
    let raw = block.get(ExifDirectory::Photo, tag)?;
    if raw.len() < 8 || block.type_of(ExifDirectory::Photo, tag) != Some(TYPE_RATIONAL) {
        return None;
    }
    let (top, bottom) = (block.read_u32(&raw[..4]), block.read_u32(&raw[4..8]));
    if top == 0 || bottom == 0 {
        None
    } else {
        Some((top, bottom))
    }
}

fn set_rational(block: &mut ExifBlock, tag: u16, top: u32, bottom: u32) {
    let mut bytes = block.write_u32(top);
    bytes.extend(block.write_u32(bottom));
    block.set(ExifDirectory::Photo, tag, TYPE_RATIONAL, bytes);
}

/// A measurement to one decimal place, stored as so-many tenths.
fn set_tenths(block: &mut ExifBlock, tag: u16, value: &str) {
    match MetadataFields::try_parse_decimal(value) {
        Some(measure) => set_rational(block, tag, round_half_even(measure * 10.0) as u32, 10),
        None => block.remove(ExifDirectory::Photo, tag),
    }
}

fn ascii(block: &ExifBlock, directory: ExifDirectory, tag: u16) -> Option<String> {
    let raw = block.get(directory, tag)?;
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let bytes = &raw[..end];

    // Typed ASCII, but UTF-8 is what everything actually writes into it. Anything that is not
    // valid UTF-8 is from an older tool and a single-byte code page, where Latin-1 at least never
    // fails and gets Western text right.
    Some(match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => latin1(bytes),
    })
}

pub(crate) fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn set_ascii(block: &mut ExifBlock, directory: ExifDirectory, tag: u16, value: &str) {
    if value.is_empty() {
        block.remove(directory, tag);
        return;
    }
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0);
    block.set(directory, tag, TYPE_ASCII, bytes);
}

fn xp(block: &ExifBlock, tag: u16) -> Option<String> {
    let raw = block.get(ExifDirectory::Image, tag)?;
    if raw.len() < 2 {
        return None;
    }
    let text = utf16(&raw[..raw.len() & !1], true);
    Some(match text.find('\0') {
        Some(end) => text[..end].to_string(),
        None => text,
    })
}

fn set_xp(block: &mut ExifBlock, tag: u16, value: &str) {
    if value.is_empty() {
        block.remove(ExifDirectory::Image, tag);
        return;
    }
    let mut bytes: Vec<u8> = value.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    bytes.extend_from_slice(&[0, 0]);
    block.set(ExifDirectory::Image, tag, TYPE_BYTE, bytes);
}

fn set_rating(block: &mut ExifBlock, value: &str) {
    let stars = value
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| value.parse::<u64>().ok())
        .flatten();
    let Some(stars) = stars.filter(|s| (1..=5).contains(s)) else {
        block.remove(ExifDirectory::Image, RATING);
        block.remove(ExifDirectory::Image, RATING_PERCENT);
        return;
    };

    // The percentages Windows itself writes for one to five stars.
    let percent: u16 = match stars {
        1 => 1,
        2 => 25,
        3 => 50,
        4 => 75,
        _ => 99,
    };
    let bytes = block.write_u16(stars as u16);
    block.set(ExifDirectory::Image, RATING, TYPE_SHORT, bytes);
    let bytes = block.write_u16(percent);
    block.set(ExifDirectory::Image, RATING_PERCENT, TYPE_SHORT, bytes);
}

/// `UserComment` carries its own eight-byte label saying how the rest is encoded.
fn read_user_comment(block: &ExifBlock) -> Option<String> {
    let raw = block.get(ExifDirectory::Photo, USER_COMMENT)?;
    if raw.len() <= 8 {
        return None;
    }
    let label = &raw[..8];
    let body = &raw[8..];

    let text = if label.starts_with(b"UNICODE") {
        utf16(&body[..body.len() & !1], block.is_little_endian())
    } else if label.starts_with(b"ASCII") || label.iter().all(|&b| b == 0) {
        String::from_utf8_lossy(body).into_owned()
    } else {
        // JIS, or something private. Not worth guessing at; the XP tag may still answer.
        return None;
    };

    let text = match text.find('\0') {
        Some(end) => &text[..end],
        None => &text,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn set_user_comment(block: &mut ExifBlock, value: &str) {
    if value.is_empty() {
        block.remove(ExifDirectory::Photo, USER_COMMENT);
        return;
    }
    let little = block.is_little_endian();
    let mut bytes = b"UNICODE\0".to_vec();
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&if little {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        });
    }
    block.set(ExifDirectory::Photo, USER_COMMENT, TYPE_UNDEFINED, bytes);
}

/// UTF-16 of either byte order, lossy as `Encoding.Unicode.GetString` is.
#[allow(clippy::chunks_exact_to_as_chunks)]
pub(crate) fn utf16(bytes: &[u8], little: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            if little {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}
