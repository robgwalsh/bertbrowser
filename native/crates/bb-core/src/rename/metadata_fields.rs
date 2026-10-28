//! The metadata field catalogue the rename tokens draw on: `{artist}`, `{track:00}`, `{taken}`.
//! Spec: Core/Services/Metadata/MetadataFields.cs (the catalogue and `TryParseDate` only).
//!
//! TODO(port): extended when Metadata is ported. Ids, labels and kinds are the C# rows verbatim so
//! the Metadata port can move this file whole and re-export from there; nothing here reads a file.

use crate::FileTime;

/// The fields this app can write. One vocabulary for every codec, the editor and the rename tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MetadataField {
    Title,
    Artist,
    AlbumArtist,
    Album,
    TrackNumber,
    TrackTotal,
    DiscNumber,
    DiscTotal,
    Year,
    Genre,
    Composer,
    Comment,
    Copyright,
    Author,
    Subject,
    Keywords,
    Rating,
    DateTaken,
    CameraMake,
    CameraModel,
    LensModel,
    Iso,
    Aperture,
    ExposureTime,
    FocalLength,
    Publisher,
    Conductor,
    Bpm,
    Lyrics,
    Category,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataFieldKind {
    Text,
    /// A whole number above zero.
    Number,
    /// A local date and time, with no zone — what a camera writes.
    Date,
    /// Several values, shown and typed separated by semicolons.
    List,
    /// Stars, one to five.
    Rating,
    /// A measurement above zero, to one decimal place: an f-number, a focal length.
    Decimal,
    /// A shutter time in seconds, written the way a camera shows it.
    Exposure,
}

/// One row of the field catalogue. `id` is the stable lowercase name — what a rename token says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetadataFieldSpec {
    pub field: MetadataField,
    pub id: &'static str,
    pub label: &'static str,
    pub kind: MetadataFieldKind,
}

const fn spec(
    field: MetadataField,
    id: &'static str,
    label: &'static str,
    kind: MetadataFieldKind,
) -> MetadataFieldSpec {
    MetadataFieldSpec {
        field,
        id,
        label,
        kind,
    }
}

use MetadataField as F;
use MetadataFieldKind as K;

/// Every field, in the order the editor lists them.
const ALL: [MetadataFieldSpec; 30] = [
    spec(F::Title, "title", "Title", K::Text),
    spec(F::Artist, "artist", "Artist", K::Text),
    spec(F::AlbumArtist, "albumartist", "Album artist", K::Text),
    spec(F::Album, "album", "Album", K::Text),
    spec(F::TrackNumber, "track", "Track", K::Number),
    spec(F::TrackTotal, "tracktotal", "Tracks in total", K::Number),
    spec(F::DiscNumber, "disc", "Disc", K::Number),
    spec(F::DiscTotal, "disctotal", "Discs in total", K::Number),
    spec(F::Year, "year", "Year", K::Number),
    spec(F::Genre, "genre", "Genre", K::Text),
    spec(F::Composer, "composer", "Composer", K::Text),
    spec(F::Conductor, "conductor", "Conductor", K::Text),
    spec(F::Publisher, "publisher", "Publisher", K::Text),
    spec(F::Bpm, "bpm", "Beats per minute", K::Number),
    spec(F::Author, "author", "Author", K::Text),
    spec(F::DateTaken, "taken", "Date taken", K::Date),
    spec(F::Subject, "subject", "Subject", K::Text),
    spec(F::Category, "category", "Category", K::Text),
    spec(F::Keywords, "keywords", "Tags", K::List),
    spec(F::Rating, "rating", "Rating", K::Rating),
    spec(F::Comment, "comment", "Comment", K::Text),
    spec(F::Lyrics, "lyrics", "Lyrics", K::Text),
    spec(F::Copyright, "copyright", "Copyright", K::Text),
    spec(F::CameraMake, "make", "Camera maker", K::Text),
    spec(F::CameraModel, "model", "Camera model", K::Text),
    spec(F::LensModel, "lens", "Lens", K::Text),
    spec(F::FocalLength, "focal", "Focal length (mm)", K::Decimal),
    spec(F::Aperture, "aperture", "Aperture (f/)", K::Decimal),
    spec(F::ExposureTime, "exposure", "Exposure (s)", K::Exposure),
    spec(F::Iso, "iso", "ISO", K::Number),
];

pub struct MetadataFields;

impl MetadataFields {
    /// How a `Date` field is held: sortable, zoneless.
    pub const DATE_FORMAT: &'static str = "yyyy-MM-dd HH:mm:ss";

    pub fn all() -> &'static [MetadataFieldSpec] {
        &ALL
    }

    pub fn get(field: MetadataField) -> &'static MetadataFieldSpec {
        ALL.iter()
            .find(|s| s.field == field)
            .expect("every field has a row")
    }

    /// The row for an id, matched without regard to case; None for an id nobody knows.
    pub fn find(id: &str) -> Option<&'static MetadataFieldSpec> {
        ALL.iter().find(|s| s.id.eq_ignore_ascii_case(id))
    }

    /// `DateTime.TryParseExact` over the forms a date field is written in: `yyyy-MM-dd HH:mm:ss`,
    /// `yyyy-MM-dd HH:mm`, `yyyy-MM-dd`, `yyyy-MM-ddTHH:mm:ss` and the EXIF `yyyy:MM:dd HH:mm:ss`.
    /// The answer is zoneless wall-clock ticks.
    pub fn try_parse_date(text: &str) -> Option<FileTime> {
        let b = text.as_bytes();
        let num = |from: usize, len: usize| -> Option<u32> {
            let s = b.get(from..from + len)?;
            if !s.iter().all(u8::is_ascii_digit) {
                return None;
            }
            std::str::from_utf8(s).ok()?.parse().ok()
        };
        let sep = |at: usize, wanted: &[u8]| b.get(at).is_some_and(|c| wanted.contains(c));

        let year = num(0, 4)?;
        let date_sep: &[u8] = if sep(4, b":") { b":" } else { b"-" };
        if !sep(4, date_sep) || !sep(7, date_sep) {
            return None;
        }
        let month = num(5, 2)?;
        let day = num(8, 2)?;
        let (hour, minute, second) = match b.len() {
            10 => (0, 0, 0),
            16 if date_sep == b"-" && sep(10, b" ") && sep(13, b":") => {
                (num(11, 2)?, num(14, 2)?, 0)
            }
            19 if sep(13, b":") && sep(16, b":") => {
                let time_sep: &[u8] = if date_sep == b":" { b" " } else { b" T" };
                if !sep(10, time_sep) {
                    return None;
                }
                (num(11, 2)?, num(14, 2)?, num(17, 2)?)
            }
            _ => return None,
        };
        if !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return None;
        }
        Some(FileTime::from_ymd_hms(
            i64::from(year),
            month,
            day,
            hour,
            minute,
            second,
        ))
    }
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) {
                29
            } else {
                28
            }
        }
    }
}
