// Port of tests/BertBrowser.Core.Tests/LineChannelTests.cs: the framing under both of this app's
// pipes. The bound is the part worth testing: a peer that never sends a newline must be cut off
// rather than allowed to grow the buffer forever.
use std::io::{self, Read};

use bb_core::ipc::{LineReader, read_line, write_line};

const MAX: usize = 64 * 1024;

fn read_bytes(bytes: &[u8], max_length: usize) -> Option<String> {
    read_line(&mut io::Cursor::new(bytes), max_length).unwrap()
}

fn read(text: &str) -> Option<String> {
    read_bytes(text.as_bytes(), MAX)
}

#[test]
fn reads_one_line() {
    assert_eq!(
        Some("OPEN\tDefault\t-C:\\Temp".to_string()),
        read("OPEN\tDefault\t-C:\\Temp\n")
    );
}

/// The one that caught the bug worth catching: a static read-one-line drops whatever the same
/// buffer fill carried past the newline, so the second message never arrives.
#[test]
fn consecutive_lines_from_one_buffer_fill_all_arrive() {
    let mut reader = LineReader::new(io::Cursor::new(b"first\nsecond\nthird\n".to_vec()), MAX);

    assert_eq!(Some("first".to_string()), reader.read_line().unwrap());
    assert_eq!(Some("second".to_string()), reader.read_line().unwrap());
    assert_eq!(Some("third".to_string()), reader.read_line().unwrap());
    assert_eq!(None, reader.read_line().unwrap());
}

/// Many small messages, more than one buffer fill's worth.
#[test]
fn many_lines_across_many_buffer_fills_all_arrive() {
    let lines: Vec<String> = (0..500)
        .map(|i| format!("IDX\tComplete\tC:\\LINE{i}"))
        .collect();
    let text = lines.join("\n") + "\n";
    let mut reader = LineReader::new(io::Cursor::new(text.into_bytes()), MAX);

    for expected in &lines {
        assert_eq!(Some(expected.clone()), reader.read_line().unwrap());
    }

    assert_eq!(None, reader.read_line().unwrap());
}

/// A stream that ends without a newline may still have delivered a whole message.
#[test]
fn accepts_a_line_with_no_trailing_newline() {
    assert_eq!(Some("OPEN\tDefault".to_string()), read("OPEN\tDefault"));
}

#[test]
fn returns_null_on_an_empty_stream() {
    assert_eq!(None, read(""));
}

#[test]
fn returns_empty_for_a_bare_newline() {
    assert_eq!(Some(String::new()), read("\n"));
}

/// The internal read buffer is 1 KB, so this line spans several of them.
#[test]
fn reads_a_line_spanning_many_buffer_fills() {
    let line = "a".repeat(5000);

    assert_eq!(Some(line.clone()), read(&(line.clone() + "\n")));
}

/// Decoding happens once over the accumulated bytes, so a character whose bytes land in different
/// reads still comes back whole. Decode-as-you-go would mangle this.
#[test]
fn reads_multi_byte_characters_split_across_reads() {
    // 400 three-byte characters straddles the 1 KB buffer boundary mid-character.
    let line = "…".repeat(400);

    assert_eq!(Some(line.clone()), read(&(line.clone() + "\n")));
}

/// A newline byte can never appear inside a UTF-8 sequence (continuation bytes all have the high
/// bit set), so scanning bytes for 0x0A cannot split a character.
#[test]
fn multi_byte_characters_do_not_produce_false_newlines() {
    let line = "路徑\u{e9}\u{20ac}\u{1F600}";

    assert_eq!(Some(line.to_string()), read(&format!("{line}\n")));
}

/// The cap is per byte, not per buffer fill: a single 1 KB read must not be able to carry the line
/// well past the limit before anything checks it.
#[test]
fn cuts_off_a_peer_that_never_sends_a_newline() {
    let result = read_bytes("a".repeat(900).as_bytes(), 64);

    assert_eq!(64, result.unwrap().len());
}

/// A peer that writes forever and never terminates a line.
struct EndlessStream;

impl Read for EndlessStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        buf.fill(b'a');
        Ok(buf.len())
    }
}

#[test]
fn cuts_off_an_endless_stream_at_the_cap() {
    let result = read_line(&mut EndlessStream, 4096).unwrap();

    assert_eq!(4096, result.unwrap().len());
}

#[test]
fn write_line_terminates_with_a_newline_in_utf8() {
    let mut stream = Vec::new();

    write_line(&mut stream, "IDX\tComplete\tC:\\").unwrap();

    assert_eq!("IDX\tComplete\tC:\\\n", String::from_utf8(stream).unwrap());
}

#[test]
fn written_lines_round_trip() {
    let mut stream = Vec::new();
    write_line(&mut stream, "one").unwrap();
    write_line(&mut stream, "two…").unwrap();

    let mut reader = LineReader::new(io::Cursor::new(stream), MAX);
    assert_eq!(Some("one".to_string()), reader.read_line().unwrap());
    assert_eq!(Some("two…".to_string()), reader.read_line().unwrap());
    assert_eq!(None, reader.read_line().unwrap());
}
