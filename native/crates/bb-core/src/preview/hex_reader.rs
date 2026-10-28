//! Formats the front of a file as a hex dump. Pure over a `Read`: it never opens anything and
//! never fails, which is what lets every rule in it be tested against a `Cursor`.
//!
//! Rows carry their own spans rather than the whole dump arriving as one string with one span
//! list. The pane's line splitter could rebase a global list, but a cover is much easier to get
//! wrong across sixty-five thousand rows than within one, and here the cover is a property a
//! single row can be asserted to hold.
//!
//! The colouring reuses `SyntaxClass` rather than minting hex-specific theme tokens: the five
//! syntax colours are already contrast-checked against every built-in palette, and a parallel
//! family would be a second set to keep in step for no gain.
//! Spec: Core/Services/Preview/HexPreviewReader.cs.

use std::fmt::Write as _;
use std::io::Read;

use super::text_reader::read_at_most;
use super::{SyntaxClass, SyntaxSpan};

/// One row of a hex dump, already formatted and already coloured. `spans` is a complete, gap-free
/// cover of `text`: complete because the view builds one run per span and renders nothing else,
/// so a gap does not show as plain text, it deletes the characters under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HexRow {
    /// Where in the file this row starts.
    pub offset: i64,
    /// The whole row: offset, the hex column, then the ASCII column.
    pub text: String,
    pub spans: Vec<SyntaxSpan>,
}

/// What a hex read turned out to be. The footer says all of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HexPreview {
    /// The formatted rows, in order. Empty for an empty file.
    pub rows: Vec<HexRow>,
    /// How many bytes the rows account for.
    pub bytes_shown: i64,
    /// The file has more than this.
    pub truncated: bool,
}

pub struct HexPreviewReader;

impl HexPreviewReader {
    /// Bytes per row. Sixteen, split eight and eight, because that is what every other dump does
    /// and the offsets then line up with a person's mental arithmetic.
    pub const BYTES_PER_ROW: usize = 16;

    /// Rows beyond this are not shown: the analogue of `TextPreviewReader::DEFAULT_MAX_LINES`,
    /// and needed for the same reason. Hex costs about four characters per byte, so the pane's
    /// megabyte budget would otherwise be 65,536 paragraphs.
    pub const DEFAULT_MAX_ROWS: usize = 5_000;

    /// The most a dump ever reads, whatever budget it is handed.
    pub fn max_bytes(max_rows: usize) -> i64 {
        max_rows as i64 * Self::BYTES_PER_ROW as i64
    }

    /// `origin` is the file offset the stream is already positioned at, so the offsets down the
    /// left read as where the bytes really are. Zero for a preview, which always starts at the
    /// beginning; a file comparison seeks to the neighbourhood of the first difference and needs
    /// the true numbers, and a second dump formatter written to provide them would be the thing
    /// worth avoiding.
    pub fn read(
        stream: &mut impl Read,
        byte_budget: i64,
        max_rows: usize,
        origin: i64,
    ) -> HexPreview {
        if max_rows == 0 {
            return HexPreview {
                rows: Vec::new(),
                bytes_shown: 0,
                truncated: false,
            };
        }

        let budget = byte_budget
            .min(Self::max_bytes(max_rows))
            .clamp(0, i64::from(i32::MAX) - 1) as usize;
        let (bytes, more_remains) = read_at_most(stream, budget);

        let rows = bytes
            .chunks(Self::BYTES_PER_ROW)
            .enumerate()
            .map(|(i, chunk)| format_row(chunk, origin + (i * Self::BYTES_PER_ROW) as i64))
            .collect();

        HexPreview {
            rows,
            bytes_shown: bytes.len() as i64,
            truncated: more_remains,
        }
    }
}

/// One row. A short final row pads both columns rather than ending early, so the ASCII gutter
/// stays where the rows above put it instead of sliding left on the last line.
fn format_row(bytes: &[u8], offset: i64) -> HexRow {
    let mut row = String::with_capacity(80);

    let _ = write!(row, "{offset:08X}");
    let offset_length = row.len();

    row.push_str("  ");
    for i in 0..HexPreviewReader::BYTES_PER_ROW {
        // The extra gap between the two halves of eight; without it counting to the eleventh
        // byte of a row means counting to eleven.
        if i == HexPreviewReader::BYTES_PER_ROW / 2 {
            row.push(' ');
        }
        match bytes.get(i) {
            Some(b) => {
                let _ = write!(row, "{b:02X}");
            }
            None => row.push_str("  "),
        }
        row.push(' ');
    }

    row.push(' ');
    let ascii_start = row.len();
    for i in 0..HexPreviewReader::BYTES_PER_ROW {
        row.push(match bytes.get(i) {
            None => ' ',
            Some(&b) if (0x20..=0x7E).contains(&b) => b as char,
            Some(_) => '.',
        });
    }

    // The bytes are the content, so they take the pane's own foreground (`SyntaxClass::Text`
    // maps to no brush at all); the offset is a gutter and takes the dimmest colour there is;
    // the ASCII column takes the string colour, which is the one furthest from both in every
    // palette. Comment and Number were the obvious pair and are the wrong one: in Dark+ they are
    // two greens, and the offsets disappeared into the bytes beside them.
    let spans = vec![
        SyntaxSpan::new(0, offset_length, SyntaxClass::Comment),
        SyntaxSpan::new(
            offset_length,
            ascii_start - offset_length,
            SyntaxClass::Text,
        ),
        SyntaxSpan::new(ascii_start, row.len() - ascii_start, SyntaxClass::String),
    ];

    HexRow {
        offset,
        text: row,
        spans,
    }
}
