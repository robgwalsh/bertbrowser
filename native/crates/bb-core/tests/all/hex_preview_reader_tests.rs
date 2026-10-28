// Port of tests/BertBrowser.Core.Tests/HexPreviewReaderTests.cs
use std::io::Cursor;

use bb_core::preview::{HexPreview, HexPreviewReader, SyntaxClass, SyntaxSpan};

fn read_with(bytes: &[u8], budget: i64, max_rows: usize) -> HexPreview {
    HexPreviewReader::read(&mut Cursor::new(bytes), budget, max_rows, 0)
}

fn read(bytes: &[u8]) -> HexPreview {
    read_with(bytes, 1 << 20, 5000)
}

fn bytes(count: usize) -> Vec<u8> {
    (0..count).map(|i| i as u8).collect()
}

// --- the row ---

#[test]
fn the_first_bytes_of_an_executable_look_like_an_executable() {
    let preview = read(&[0x4D, 0x5A, 0x90, 0x00, 0x03, 0x00, 0x00, 0x00]);
    assert_eq!(1, preview.rows.len());
    let row = &preview.rows[0];
    assert!(
        row.text.starts_with("00000000  4D 5A 90 00 03 00 00 00 "),
        "{:?}",
        row.text
    );
    assert!(row.text.ends_with("MZ......        "), "{:?}", row.text);
}

#[test]
fn offsets_advance_by_sixteen_and_are_eight_hex_digits() {
    let rows = read(&bytes(40)).rows;
    assert_eq!(
        vec![0, 16, 32],
        rows.iter().map(|r| r.offset).collect::<Vec<_>>()
    );
    assert_eq!(
        vec!["00000000", "00000010", "00000020"],
        rows.iter().map(|r| &r.text[..8]).collect::<Vec<_>>()
    );
}

#[test]
fn a_short_final_row_pads_both_columns() {
    // Otherwise the ASCII gutter slides left on the last line of every file whose length is not
    // a multiple of sixteen — which is nearly all of them.
    let rows = read(&bytes(20)).rows;
    assert_eq!(rows[0].text.len(), rows[1].text.len());
}

#[test]
fn the_ascii_column_shows_what_can_be_shown_and_a_dot_for_the_rest() {
    for (value, shown) in [
        (0x20u8, ' '), // space is printable, and a run of them is meaningful
        (0x41, 'A'),
        (0x7E, '~'),
        (0x00, '.'),
        (0x1F, '.'),
        (0x7F, '.'), // DEL is not printable however tempting its position is
        (0xFF, '.'),
    ] {
        let preview = read(&[value]);
        assert_eq!(1, preview.rows.len());
        let text = &preview.rows[0].text;
        let at = text
            .chars()
            .nth_back(HexPreviewReader::BYTES_PER_ROW - 1)
            .unwrap();
        assert_eq!(shown, at, "byte {value:#04x}");
    }
}

// --- the cover, which is the one that must never break ---

#[test]
fn spans_cover_every_row_exactly_once() {
    // The view builds one run per span and renders nothing else, so a gap here does not show as
    // uncoloured text — it deletes the characters underneath it.
    for length in [1, 8, 16, 17, 255] {
        for row in read(&bytes(length)).rows {
            let mut covered = 0;
            for span in &row.spans {
                assert!(
                    span.length > 0,
                    "length {length}: an empty span reached the view"
                );
                assert_eq!(covered, span.start, "length {length}");
                covered += span.length;
            }
            assert_eq!(row.text.len(), covered, "length {length}");
        }
    }
}

// --- how much is read ---

#[test]
fn a_file_exactly_the_budget_is_not_reported_as_truncated() {
    let preview = read_with(&bytes(64), 64, 5000);
    assert!(!preview.truncated);
    assert_eq!(64, preview.bytes_shown);
}

#[test]
fn one_byte_more_than_the_budget_is_truncated() {
    let preview = read_with(&bytes(65), 64, 5000);
    assert!(preview.truncated);
    assert_eq!(64, preview.bytes_shown);
}

#[test]
fn the_row_cap_bounds_the_read_whatever_budget_it_is_given() {
    // The point of the cap: a megabyte of hex is 65,536 paragraphs, so the budget alone is not a
    // ceiling on the work the view does.
    let preview = read_with(&bytes(200), 1 << 20, 4);
    assert_eq!(4, preview.rows.len());
    assert_eq!(64, preview.bytes_shown);
    assert!(preview.truncated);
}

#[test]
fn an_empty_file_has_no_rows() {
    assert!(read(&[]).rows.is_empty());
}

#[test]
fn a_budget_of_zero_reads_nothing_but_still_notices_there_is_more() {
    let preview = read_with(&bytes(16), 0, 5000);
    assert!(preview.rows.is_empty());
    assert!(preview.truncated);
}

// --- the meta-test: the cover check above can actually fail ---

#[test]
fn the_cover_check_notices_a_gap() {
    let holed = [
        SyntaxSpan::new(0, 4, SyntaxClass::Comment),
        SyntaxSpan::new(5, 3, SyntaxClass::Number),
    ];

    let mut covered = 0;
    let mut contiguous = true;
    for span in holed {
        if span.start != covered {
            contiguous = false;
        }
        covered += span.length;
    }
    assert!(!contiguous);
}
