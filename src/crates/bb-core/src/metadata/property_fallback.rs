//! Windows' own property handlers, for the files no codec here writes — what Explorer's Details
//! tab edits through. Spec: Core/Services/Metadata/PropertyFallback.cs.
//!
//! **This is the one write whose payload nobody checks.** A codec's rewrite is only swapped in
//! once everything that is not metadata hashes the same as the original; a property handler is
//! somebody else's code writing a format this app cannot read, so there is nothing to hash. What
//! is kept: the handler is only ever pointed at a *copy* beside the file, the fields are read
//! back from that copy, and it is swapped in the same way — so a handler that fails or refuses
//! costs a temporary file, and one that succeeds is one undo from never having happened. What is
//! lost is the proof that the rest of the file is untouched, and the pane says so on every file
//! edited this way. Never offered where a codec exists: the extension decides, and a codec always
//! wins.
//!
//! The store itself (`IPropertyStore` opened `GPS_READWRITE` to ask `IsPropertyWritable`, values read
//! through a shared store) is the C++ side's, in `property_host.cpp`; this module holds the trait it
//! implements and the pure half that can be wrong without Windows being involved.

use std::collections::BTreeMap;

use super::codec::{MetadataError, MetadataResult};
use super::fields::{MetadataField, MetadataFieldKind, MetadataFields};
use super::models::MetadataDocument;
use crate::FileTime;

pub trait PropertyFallback {
    /// What the file holds and which fields Windows will take, or None when its handler writes
    /// none of them (or it has no handler at all).
    fn read(&self, path: &str) -> Option<PropertyFallbackReading>;

    /// Writes into the file at `path`, in place. `Format` when the handler refused, in words fit
    /// to show.
    fn write(&self, path: &str, changes: &BTreeMap<MetadataField, String>) -> MetadataResult<()>;
}

/// `fields` are the fields the handler said it would write — per file, not per type: the answer
/// is the handler's, and the same extension can get a different one on another PC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyFallbackReading {
    pub document: MetadataDocument,
    pub fields: Vec<MetadataField>,
}

/// A property value on its way to Windows: exactly one part set, or none to clear it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowsPropertyValue {
    pub text: Option<String>,
    pub number: Option<u32>,
    pub utc: Option<FileTime>,
}

impl WindowsPropertyValue {
    pub fn is_clear(&self) -> bool {
        self.text.is_none() && self.number.is_none() && self.utc.is_none()
    }
}

/// Canonical property names, which are the same in every language of Windows. Only fields whose
/// value is text, a whole number, a date or stars: nothing here guesses at how a handler wants a
/// fraction.
const CANONICAL: [(MetadataField, &str); 20] = [
    (MetadataField::Title, "System.Title"),
    (MetadataField::Artist, "System.Music.Artist"),
    (MetadataField::AlbumArtist, "System.Music.AlbumArtist"),
    (MetadataField::Album, "System.Music.AlbumTitle"),
    (MetadataField::TrackNumber, "System.Music.TrackNumber"),
    (MetadataField::Year, "System.Media.Year"),
    (MetadataField::Genre, "System.Music.Genre"),
    (MetadataField::Composer, "System.Music.Composer"),
    (MetadataField::Conductor, "System.Music.Conductor"),
    (MetadataField::Publisher, "System.Media.Publisher"),
    (MetadataField::Author, "System.Author"),
    (MetadataField::DateTaken, "System.Photo.DateTaken"),
    (MetadataField::Subject, "System.Subject"),
    (MetadataField::Category, "System.Category"),
    (MetadataField::Keywords, "System.Keywords"),
    (MetadataField::Rating, "System.Rating"),
    (MetadataField::Comment, "System.Comment"),
    (MetadataField::Copyright, "System.Copyright"),
    (MetadataField::CameraMake, "System.Photo.CameraManufacturer"),
    (MetadataField::CameraModel, "System.Photo.CameraModel"),
];

/// Read-only here — nothing writes a fraction through a handler — but still the same fact as a
/// field a codec offers.
const READ_ONLY: [(MetadataField, &str); 5] = [
    (MetadataField::LensModel, "System.Photo.LensModel"),
    (MetadataField::Aperture, "System.Photo.FNumber"),
    (MetadataField::ExposureTime, "System.Photo.ExposureTime"),
    (MetadataField::Iso, "System.Photo.ISOSpeed"),
    (MetadataField::FocalLength, "System.Photo.FocalLength"),
];

/// Which Windows property each field is, and how a value crosses in each direction. Pure, so the
/// part of the fallback that can be wrong without Windows being involved is tested without it.
pub struct WindowsPropertyFields;

impl WindowsPropertyFields {
    pub fn canonical() -> &'static [(MetadataField, &'static str)] {
        &CANONICAL
    }

    /// The canonical name a field is written as, or None for one nothing writes through a handler.
    pub fn canonical_of(field: MetadataField) -> Option<&'static str> {
        CANONICAL.iter().find(|(f, _)| *f == field).map(|(_, n)| *n)
    }

    /// The field a Windows property is the same fact as, if any — so a read-only list of
    /// properties can leave out what already has a box.
    pub fn field_of(canonical: &str) -> Option<MetadataField> {
        CANONICAL
            .iter()
            .chain(READ_ONLY.iter())
            .find(|(_, n)| n.eq_ignore_ascii_case(canonical))
            .map(|(f, _)| *f)
    }

    /// What Windows returned, as this app's canonical value; empty for nothing, or for something
    /// that is not a value of the field's kind. `text` is the value as Windows renders it, a list
    /// joined with semicolons.
    pub fn from_windows(
        field: MetadataField,
        text: Option<&str>,
        number: Option<f64>,
        utc: Option<FileTime>,
    ) -> String {
        let raw = match MetadataFields::get(field).kind {
            MetadataFieldKind::Rating => number.map(stars).unwrap_or_default(),
            MetadataFieldKind::Date => utc
                .map(|d| MetadataFields::format_date(d.to_local()))
                .unwrap_or_default(),
            MetadataFieldKind::Number
                if number.is_some_and(|n| n == n.floor() && n > 0.0 && n < u32::MAX as f64) =>
            {
                (number.expect("checked") as u32).to_string()
            }
            _ => text.unwrap_or("").to_string(),
        };

        MetadataFields::try_normalize(field, &raw).unwrap_or_default()
    }

    /// A canonical value as Windows wants it; an empty one clears the property. `Format` when
    /// not a value of the field's kind.
    pub fn to_windows(field: MetadataField, value: &str) -> MetadataResult<WindowsPropertyValue> {
        if value.is_empty() {
            return Ok(WindowsPropertyValue::default());
        }

        let spec = MetadataFields::get(field);
        let whole = || {
            value
                .bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| value.parse::<u32>().ok())
                .flatten()
        };
        match spec.kind {
            MetadataFieldKind::Rating if whole().is_some_and(|s| s <= 5) => {
                let stars = whole().expect("checked");
                Ok(if stars == 0 {
                    WindowsPropertyValue::default()
                } else {
                    WindowsPropertyValue {
                        number: Some(rating(stars)),
                        ..Default::default()
                    }
                })
            }
            MetadataFieldKind::Number if whole().is_some() => Ok(WindowsPropertyValue {
                number: whole(),
                ..Default::default()
            }),
            MetadataFieldKind::Date if MetadataFields::try_parse_date(value).is_some() => {
                let date = MetadataFields::try_parse_date(value).expect("checked");
                Ok(WindowsPropertyValue {
                    utc: Some(date.local_to_utc()),
                    ..Default::default()
                })
            }
            MetadataFieldKind::Text | MetadataFieldKind::List => Ok(WindowsPropertyValue {
                text: Some(value.to_string()),
                ..Default::default()
            }),
            _ => Err(MetadataError::format(format!(
                "'{value}' is not a value {} can take.",
                spec.label
            ))),
        }
    }
}

// Windows keeps stars as 1 to 99, and these are the five numbers Explorer itself writes; the
// bands are the ones it reads them back with.
fn rating(stars: u32) -> u32 {
    match stars {
        1 => 1,
        2 => 25,
        3 => 50,
        4 => 75,
        _ => 99,
    }
}

fn stars(rating: f64) -> String {
    if rating < 1.0 {
        ""
    } else if rating < 13.0 {
        "1"
    } else if rating < 38.0 {
        "2"
    } else if rating < 63.0 {
        "3"
    } else if rating < 88.0 {
        "4"
    } else {
        "5"
    }
    .to_string()
}

/// Which fields a handler will take, remembered per extension. `IsPropertyWritable` lies on a
/// read-only store, so the question has to be asked of a store opened for writing — and that
/// open is exclusive, so it is asked once per extension rather than once per file, and a store
/// that would not open is not remembered, since that can be one file in use.
#[derive(Clone, Debug, Default)]
pub struct WritabilityMemory {
    by_extension: BTreeMap<String, Vec<MetadataField>>,
}

impl WritabilityMemory {
    pub fn new() -> WritabilityMemory {
        WritabilityMemory::default()
    }

    /// The fields remembered for a path's extension, or None when nothing has been learnt yet.
    pub fn recall(&self, path: &str) -> Option<&[MetadataField]> {
        self.by_extension.get(&key(path)).map(Vec::as_slice)
    }

    /// What the handler answered for one file of this extension, when the store opened. A store
    /// that did not open teaches nothing.
    pub fn remember(&mut self, path: &str, offered: Option<Vec<MetadataField>>) {
        if let Some(fields) = offered {
            self.by_extension.insert(key(path), fields);
        }
    }
}

fn key(path: &str) -> String {
    super::codecs::extension_of(path).to_ascii_lowercase()
}
