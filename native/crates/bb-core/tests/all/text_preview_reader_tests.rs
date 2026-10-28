// Port of tests/BertBrowser.Core.Tests/TextPreviewReaderTests.cs
use std::io::Cursor;

use bb_core::preview::{TextPreview, TextPreviewReader, TextReadOptions};

fn read_with(bytes: &[u8], budget: i64, max_lines: usize) -> TextPreview {
    TextPreviewReader::read(
        &mut Cursor::new(bytes),
        budget,
        &TextReadOptions {
            max_lines,
            ..TextReadOptions::default()
        },
    )
}

fn read(bytes: &[u8]) -> TextPreview {
    read_with(bytes, 1 << 20, 5000)
}

fn read_budget(bytes: &[u8], budget: i64) -> TextPreview {
    read_with(bytes, budget, 5000)
}

fn utf16_le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn utf16_be(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

fn with_bom(bom: &[u8], body: &[u8]) -> Vec<u8> {
    [bom, body].concat()
}

// --- encodings ---

#[test]
fn plain_ascii_is_utf8() {
    let preview = read(b"hello world");
    assert_eq!("hello world", preview.text);
    assert_eq!("UTF-8", preview.encoding_name);
    assert!(!preview.looks_binary);
}

#[test]
fn utf8_with_a_bom_is_reported_as_such_and_the_bom_is_not_in_the_text() {
    let preview = read(&with_bom(&[0xEF, 0xBB, 0xBF], "héllo".as_bytes()));
    assert_eq!("héllo", preview.text);
    assert_eq!("UTF-8 (BOM)", preview.encoding_name);
}

#[test]
fn utf8_without_a_bom_is_still_decoded_as_utf8() {
    let preview = read("naïve café — ok".as_bytes());
    assert_eq!("naïve café — ok", preview.text);
    assert_eq!("UTF-8", preview.encoding_name);
}

#[test]
fn utf16_little_endian_with_a_bom() {
    let preview = read(&with_bom(&[0xFF, 0xFE], &utf16_le("hello")));
    assert_eq!("hello", preview.text);
    assert_eq!("UTF-16 LE", preview.encoding_name);
}

#[test]
fn utf16_big_endian_with_a_bom() {
    let preview = read(&with_bom(&[0xFE, 0xFF], &utf16_be("hello")));
    assert_eq!("hello", preview.text);
    assert_eq!("UTF-16 BE", preview.encoding_name);
}

#[test]
fn utf16_without_a_bom_is_recognised_rather_than_called_binary() {
    // This is the case that matters: half the bytes are NUL, and a naive binary check would throw
    // away a perfectly ordinary Windows text file.
    let preview = read(&utf16_le("a fairly ordinary line of text"));
    assert_eq!("UTF-16 LE", preview.encoding_name);
    assert_eq!("a fairly ordinary line of text", preview.text);
    assert!(!preview.looks_binary);
}

#[test]
fn utf16_big_endian_without_a_bom_is_recognised_too() {
    let preview = read(&utf16_be("a fairly ordinary line of text"));
    assert_eq!("UTF-16 BE", preview.encoding_name);
    assert_eq!("a fairly ordinary line of text", preview.text);
}

#[test]
fn invalid_utf8_falls_back_to_latin1_rather_than_losing_bytes() {
    // 0xE9 alone is "é" in Latin-1 and an illegal lead byte in UTF-8.
    let preview = read(&[b'c', b'a', b'f', 0xE9]);
    assert_eq!("Latin-1", preview.encoding_name);
    assert_eq!("café", preview.text);
}

#[test]
fn utf32_is_recognised_from_its_bom() {
    let body: Vec<u8> = "hello"
        .chars()
        .flat_map(|c| (c as u32).to_le_bytes())
        .collect();
    let preview = read(&with_bom(&[0xFF, 0xFE, 0x00, 0x00], &body));
    assert_eq!("UTF-32 LE", preview.encoding_name);
    assert_eq!("hello", preview.text);
}

// --- binary ---

#[test]
fn a_nul_byte_makes_it_binary() {
    let preview = read(&[b'M', b'Z', 0x00, 0x03, 0x04, b'x']);
    assert!(preview.looks_binary);
    assert_eq!("", preview.text);
}

#[test]
fn binary_is_detected_even_when_the_nul_is_late() {
    let mut bytes = vec![b'a'; 4096];
    bytes[4000] = 0;
    assert!(read(&bytes).looks_binary);
}

#[test]
fn an_empty_file_is_not_binary() {
    let preview = read(&[]);
    assert!(!preview.looks_binary);
    assert_eq!("", preview.text);
    assert_eq!(0, preview.line_count);
}

// --- line endings and counts ---

#[test]
fn the_line_ending_style_is_reported() {
    for (text, expected) in [
        ("a\r\nb\r\nc", "CRLF"),
        ("a\nb\nc", "LF"),
        ("a\rb\rc", "CR"),
        ("a\r\nb\nc", "Mixed"),
        ("just one line", ""),
    ] {
        assert_eq!(expected, read(text.as_bytes()).line_ending, "{text:?}");
    }
}

#[test]
fn every_line_ending_is_normalised_to_newline() {
    assert_eq!("a\nb\nc", read(b"a\r\nb\rc").text);
}

#[test]
fn lines_are_counted_the_way_an_editor_would() {
    // A trailing newline ends the last line; it does not begin an empty one.
    for (text, expected) in [("a\r\nb\r\nc", 3), ("a\nb\nc\n", 3), ("one", 1), ("", 0)] {
        assert_eq!(expected, read(text.as_bytes()).line_count, "{text:?}");
    }
}

// --- truncation ---

#[test]
fn the_byte_budget_truncates_and_says_so() {
    let preview = read_budget(&[b'x'; 5000], 100);
    assert!(preview.truncated);
    assert_eq!(100, preview.text.len());
}

#[test]
fn a_file_that_fits_is_not_reported_as_truncated() {
    assert!(!read_budget(b"short", 100).truncated);
}

#[test]
fn a_file_exactly_the_budget_is_not_reported_as_truncated() {
    assert!(!read_budget(b"12345", 5).truncated);
}

#[test]
fn the_line_cap_truncates_and_says_so() {
    let text = (0..200)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let preview = read_with(text.as_bytes(), 1 << 20, 10);
    assert!(preview.truncated);
    assert_eq!(10, preview.line_count);
    assert!(preview.text.starts_with("line 0\n"));
    assert!(!preview.text.contains("line 10"));
}

#[test]
fn a_truncated_utf8_sequence_is_dropped_rather_than_shown_as_a_replacement_character() {
    // "aaa" then a 3-byte character, cut after its first byte.
    let mut bytes = b"aaa".to_vec();
    bytes.extend_from_slice("€".as_bytes());
    let preview = read_budget(&bytes, 4);
    assert!(preview.truncated);
    assert_eq!("aaa", preview.text);
    assert!(!preview.text.contains('\u{FFFD}'));
}

#[test]
fn a_truncated_utf16_pair_is_dropped_rather_than_decoded_as_rubbish() {
    let bytes = with_bom(&[0xFF, 0xFE], &utf16_le("abc"));
    // BOM (2) + "ab" (4) + half of 'c'.
    let preview = read_budget(&bytes, 7);
    assert!(preview.truncated);
    assert_eq!("ab", preview.text);
}

// --- guessing: is this worth showing when nothing said it was text? ---

#[test]
fn ordinary_text_is_convincing() {
    assert!(TextPreviewReader::is_convincing_text(&read(
        b"<?xml version=\"1.0\"?>\n<a b=\"c\"/>\n"
    )));
}

#[test]
fn an_empty_file_is_convincing() {
    // Showing it as a blank text pane beats refusing it.
    assert!(TextPreviewReader::is_convincing_text(&read(&[])));
}

#[test]
fn something_with_a_nul_is_not_convincing() {
    assert!(!TextPreviewReader::is_convincing_text(&read(&[
        b'M', b'Z', 0x00, b'x'
    ])));
}

#[test]
fn binary_with_no_nul_is_still_not_convincing() {
    // The case looks_binary alone misses: no NUL in the first 8 KB, but decoded as Latin-1 it is
    // a wall of control characters.
    let bytes: Vec<u8> = (0..2048).map(|i| (1 + i % 0x1F) as u8).collect();

    let preview = read(&bytes);
    assert!(!preview.looks_binary); // the loose check passes it
    assert!(!TextPreviewReader::is_convincing_text(&preview)); // the strict one does not
}

#[test]
fn tabs_and_newlines_do_not_count_against_it() {
    let text = "\tindented\r\n".repeat(200);
    assert!(TextPreviewReader::is_convincing_text(&read(
        text.as_bytes()
    )));
}

#[test]
fn a_stray_control_character_is_tolerated_in_otherwise_good_text() {
    // A form feed in a source file must not cost the whole preview.
    let text = format!("{}\u{000C}{}", "x".repeat(500), "y".repeat(500));
    assert!(TextPreviewReader::is_convincing_text(&read(
        text.as_bytes()
    )));
}

#[test]
fn a_budget_of_zero_reads_nothing_but_still_notices_there_is_more() {
    let preview = read_budget(b"something", 0);
    assert_eq!("", preview.text);
    assert!(preview.truncated);
}

// --- raw mode: decode it anyway ---

fn raw_with(bytes: &[u8], max_line_length: usize) -> TextPreview {
    TextPreviewReader::read(
        &mut Cursor::new(bytes),
        1 << 20,
        &TextReadOptions {
            max_lines: 5000,
            max_line_length,
            force_text: true,
        },
    )
}

fn raw(bytes: &[u8]) -> TextPreview {
    raw_with(bytes, TextPreviewReader::DEFAULT_MAX_LINE_LENGTH)
}

#[test]
fn forced_text_decodes_what_would_otherwise_be_called_binary() {
    // This is what lets the pane show a PDF's header instead of refusing the file.
    let bytes = [&b"%PDF-1.7"[..], &[0x00, 0x01], &b"endobj"[..]].concat();

    assert!(read(&bytes).looks_binary);

    let raw = raw(&bytes);
    assert!(!raw.looks_binary);
    assert!(raw.text.starts_with("%PDF-1.7"));
    assert!(raw.text.ends_with("endobj"));
}

#[test]
fn forced_text_still_honours_a_byte_order_mark() {
    // Forcing skips the NUL rung of the ladder and only that rung — a UTF-16 file is full of
    // NULs and is still UTF-16.
    let all = with_bom(&[0xFF, 0xFE], &utf16_le("hello"));
    let raw = raw(&all);
    assert_eq!("hello", raw.text);
    assert_eq!("UTF-16 LE", raw.encoding_name);
}

#[test]
fn a_forced_read_shows_a_control_byte_as_a_dot_rather_than_as_a_line_break() {
    // WPF's text layout breaks a line on every one of these, so a row would appear that the
    // string has no line for — and the gutter beside it would stop lining up from there down.
    for value in [
        0x00u8, 0x0B, // vertical tab
        0x0C, // form feed
        0x85, // NEL — the one that actually bites, since Latin-1 produces it
    ] {
        let raw = raw(&[b'a', value, b'b']);
        assert_eq!("a.b", raw.text, "byte {value:#04x}");
        assert_eq!(1, raw.line_count, "byte {value:#04x}");
    }
}

#[test]
fn a_forced_read_keeps_real_newlines_and_tabs() {
    let raw = raw(b"one\ttwo\r\nthree");
    assert_eq!("one\ttwo\nthree", raw.text);
    assert_eq!(2, raw.line_count);
}

#[test]
fn ordinary_text_keeps_its_separators() {
    // Not dotted outside raw mode: there a form feed really is a page break somebody typed.
    let preview = read(b"page\x0Cbreak");
    assert_eq!("page\u{000C}break", preview.text);
}

#[test]
fn every_byte_survives_a_forced_read() {
    // Latin-1 is the rung it lands on, and it maps all 256 of them — nothing is dropped and
    // nothing becomes a replacement glyph.
    let bytes: Vec<u8> = (0..=255).collect();
    assert_eq!(256, raw_with(&bytes, 0).text.chars().count());
}

// --- one very long line ---

#[test]
fn a_line_longer_than_the_cap_is_broken_up() {
    // A minified bundle is one line, and a binary read raw almost always is; a single
    // megabyte-long run in a non-wrapping text control is a stall.
    let line = "x".repeat(250);
    let preview = read_with(line.as_bytes(), 1 << 20, 5000);
    assert_eq!(1, preview.line_count);

    let folded = TextPreviewReader::read(
        &mut Cursor::new(line.as_bytes()),
        1 << 20,
        &TextReadOptions {
            max_lines: 5000,
            max_line_length: 100,
            force_text: false,
        },
    );
    assert_eq!(3, folded.line_count);
    assert_eq!("x".repeat(100), folded.text.split('\n').next().unwrap());
}

#[test]
fn ordinary_text_is_untouched_by_the_length_cap() {
    let text = "line one\nline two\nline three";
    assert_eq!(text, read(text.as_bytes()).text);
}

#[test]
fn folding_leaves_the_real_line_breaks_where_they_were() {
    let folded = TextPreviewReader::read(
        &mut Cursor::new(&b"aaaaa\nbb\nccccccc"[..]),
        1 << 20,
        &TextReadOptions {
            max_lines: 5000,
            max_line_length: 3,
            force_text: false,
        },
    );
    assert_eq!(
        vec!["aaa", "aa", "bb", "ccc", "ccc", "c"],
        folded.text.split('\n').collect::<Vec<_>>()
    );
}
