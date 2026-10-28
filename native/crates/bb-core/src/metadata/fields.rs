//! The field catalogue, and the one place a typed value becomes the form every codec stores and
//! compares. Spec: Core/Services/Metadata/MetadataFields.cs.
//!
//! **Values are strings in a canonical form**, and the executor's read-back compares them as such,
//! so normalising happens here, once, before a plan exists. Two spellings of one value reaching a
//! codec would make a correct write look like a failed one. Which of these a given file can hold
//! is not decided here: each codec states its own set (`MetadataCodec::fields`), because a WAV and
//! an MP3 are both audio and do not have room for the same things.

use crate::FileTime;

/// What kind of file a codec describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MetadataFamily {
    Image,
    /// Sound, and video in the containers that tag the same way.
    Audio,
    Document,
}

/// The fields this app can write. One vocabulary for every codec, the editor and the rename tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    /// A shutter time in seconds, written the way a camera shows it: `1/250`, or `2.5` for a
    /// long one.
    Exposure,
}

/// One row of the field catalogue. `id` is the stable lowercase name — what a script, a rename
/// token or a saved setting says.
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

    /// What separates the values of a `List`.
    pub const LIST_SEPARATOR: &'static str = "; ";

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

    /// Turns what somebody typed into the canonical value, or says it is not one. None when the
    /// text cannot be this kind of field; an empty result is a valid value and means *clear the
    /// field*.
    pub fn try_normalize(field: MetadataField, text: &str) -> Option<String> {
        Self::try_normalize_kind(Self::get(field).kind, text)
    }

    pub fn try_normalize_kind(kind: MetadataFieldKind, text: &str) -> Option<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Some(String::new());
        }

        // A control character cannot be typed and no tag format has a place for one; refusing it
        // here keeps a pasted NUL from truncating the value inside a codec.
        if trimmed
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
        {
            return None;
        }

        match kind {
            MetadataFieldKind::Number => {
                let number = parse_digits(trimmed)?;
                if number == 0 || number > 9_999_999 {
                    return None;
                }
                Some(number.to_string())
            }
            MetadataFieldKind::Rating => {
                let stars = parse_digits(trimmed)?;
                if stars > 5 {
                    return None;
                }
                Some(if stars == 0 {
                    String::new()
                } else {
                    stars.to_string()
                })
            }
            MetadataFieldKind::Date => {
                let date = Self::try_parse_date(trimmed)?;
                Some(Self::format_date(date))
            }
            MetadataFieldKind::List => Some(Self::split_list(trimmed).join(Self::LIST_SEPARATOR)),
            MetadataFieldKind::Decimal => {
                let measure = Self::try_parse_decimal(trimmed)?;
                Some(Self::format_decimal(measure))
            }
            MetadataFieldKind::Exposure => {
                let (numerator, denominator) = Self::try_parse_exposure(trimmed)?;
                Some(Self::format_exposure(numerator, denominator))
            }
            MetadataFieldKind::Text => {
                // One line ending, whichever the text arrived with: tag formats disagree about
                // them, and a value that differs only there must not read back as a different one.
                Some(trimmed.replace("\r\n", "\n").replace('\r', "\n"))
            }
        }
    }

    /// The values of a list field, trimmed, with blanks and repeats dropped.
    pub fn split_list(value: &str) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        let mut out = Vec::new();
        for part in value.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let key = part.to_uppercase();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            out.push(part.to_string());
        }
        out
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

    /// Wall-clock ticks as `DATE_FORMAT`.
    pub fn format_date(date: FileTime) -> String {
        let c = date.civil();
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            c.year, c.month, c.day, c.hour, c.minute, c.second
        )
    }

    /// A measurement to one decimal place, above zero and below a million.
    pub fn try_parse_decimal(text: &str) -> Option<f64> {
        let value = parse_plain_double(text)?;
        let rounded = round_half_even(value * 10.0) / 10.0;
        if rounded > 0.0 && rounded < 1_000_000.0 {
            Some(rounded)
        } else {
            None
        }
    }

    /// `value.ToString("0.#")`.
    pub fn format_decimal(value: f64) -> String {
        let text = format!("{:.1}", value);
        match text.strip_suffix(".0") {
            Some(whole) => whole.to_string(),
            None => text,
        }
    }

    /// A shutter time as the fraction a file stores: `1/250` is 1 over 250, `2.5` is 25 over 10.
    /// A time below a second that is one over a whole number is kept as that fraction, because
    /// that is how every camera and every photographer writes it.
    pub fn try_parse_exposure(text: &str) -> Option<(u32, u32)> {
        if let Some(slash) = text.find('/').filter(|&s| s > 0) {
            let top = parse_digits(text[..slash].trim())?;
            let bottom = parse_digits(text[slash + 1..].trim())?;
            if top == 0 || bottom == 0 || bottom > 1_000_000 || top > 1_000_000 {
                return None;
            }

            // Anything that is not one-over-something is the same time written as a decimal.
            if top == 1 {
                return Some((1, bottom as u32));
            }

            return from_seconds(top as f64 / bottom as f64);
        }

        from_seconds(parse_plain_double(text)?)
    }

    pub fn format_exposure(numerator: u32, denominator: u32) -> String {
        if numerator == 0 || denominator == 0 {
            return String::new();
        }
        if numerator == 1 && denominator > 1 {
            return format!("1/{denominator}");
        }

        match from_seconds(numerator as f64 / denominator as f64) {
            Some((1, bottom)) if bottom > 1 => format!("1/{bottom}"),
            _ => Self::format_decimal(
                round_half_even((numerator as f64 / denominator as f64) * 10.0) / 10.0,
            ),
        }
    }
}

fn from_seconds(seconds: f64) -> Option<(u32, u32)> {
    if !(seconds > 0.0 && seconds <= 86_400.0) {
        return None;
    }

    let inverse = 1.0 / seconds;
    if seconds < 1.0 && (inverse - round_half_even(inverse)).abs() < 1e-6 && inverse <= 1_000_000.0
    {
        return Some((1, round_half_even(inverse) as u32));
    }

    let tenths = round_half_even(seconds * 10.0);
    if tenths < 1.0 {
        return None;
    }
    let (mut numerator, mut denominator) = (tenths as u32, 10u32);

    // 0.5 is tidier, and more usual, as 1/2.
    if numerator < 10 && 10 % numerator == 0 {
        (numerator, denominator) = (1, 10 / numerator);
    }
    Some((numerator, denominator))
}

/// `Math.Round(double)`: to the nearest whole number, ties to even.
pub(crate) fn round_half_even(value: f64) -> f64 {
    let floor = value.floor();
    let fraction = value - floor;
    if fraction < 0.5 {
        floor
    } else if fraction > 0.5 {
        floor + 1.0
    } else if floor % 2.0 == 0.0 {
        floor
    } else {
        floor + 1.0
    }
}

/// `int.TryParse(text, NumberStyles.None)`: digits only, no sign, no spaces; None on overflow.
fn parse_digits(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok().filter(|&n| n <= u32::MAX as u64)
}

/// `double.TryParse(text, NumberStyles.AllowDecimalPoint)`: digits and at most one point.
fn parse_plain_double(text: &str) -> Option<f64> {
    let digits = text.bytes().filter(|b| b.is_ascii_digit()).count();
    let points = text.bytes().filter(|&b| b == b'.').count();
    if digits == 0 || points > 1 || text.bytes().any(|b| !b.is_ascii_digit() && b != b'.') {
        return None;
    }
    text.parse::<f64>().ok()
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
