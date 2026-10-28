//! Decodes the front of a file for display. Pure over a `Read`: it never opens anything, which is
//! what lets every rule in it be tested against a `Cursor`.
//!
//! There is no encoding detection in the standard library and no dependency here to borrow one
//! from, so this is the usual ladder: byte-order mark, bomless UTF-16, NUL-means-binary, strict
//! UTF-8 validation, then Latin-1 as the fallback that cannot fail. Latin-1 rather than the
//! machine's ANSI codepage on purpose: it maps every byte to a character, never fails, never
//! substitutes, and gives the same answer on every machine, which is what makes the tests mean
//! anything.
//!
//! **This ladder is the only one in the app.** The content search decodes through `decode` too,
//! so the grep and the pane cannot come to different conclusions about what a file is. A second
//! implementation would be two answers to one question.
//! Spec: Core/Services/Preview/TextPreviewReader.cs.

use std::io::Read;

/// What a text preview turned out to be. Everything the pane's footer says comes from here, so
/// nothing about the read is inferred twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPreview {
    /// The decoded text, with line endings normalised to `\n`.
    pub text: String,
    /// How it was decoded, in words a person would use.
    pub encoding_name: &'static str,
    /// "CRLF", "LF", "CR", "Mixed", or empty for a single-line file.
    pub line_ending: &'static str,
    /// Lines in `text`, not in the file; they differ when `truncated` is set.
    pub line_count: usize,
    /// The file has more than this. Said out loud rather than hidden: a preview that silently
    /// stops is worse than no preview, because it looks like the whole file.
    pub truncated: bool,
    /// Not text at all. `text` is empty.
    pub looks_binary: bool,
}

/// The caps a read is held to, and whether to decode what is plainly not text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextReadOptions {
    /// Lines beyond this are not shown.
    pub max_lines: usize,
    /// Characters beyond this are folded onto the next line; zero means no folding.
    pub max_line_length: usize,
    /// Decode the bytes even when they are plainly not text. This is raw mode: it skips the NUL
    /// rung of the ladder and *only* that rung, so a forced read of a UTF-16 file is still UTF-16
    /// and everything else lands on Latin-1, which maps every byte. It is what lets the pane show
    /// a PDF's header.
    pub force_text: bool,
}

impl Default for TextReadOptions {
    fn default() -> Self {
        TextReadOptions {
            max_lines: TextPreviewReader::DEFAULT_MAX_LINES,
            max_line_length: TextPreviewReader::DEFAULT_MAX_LINE_LENGTH,
            force_text: false,
        }
    }
}

impl TextReadOptions {
    /// No line cap and no folding: what a search wants, where the first would stop searching at
    /// line 5,000 and the second would insert line breaks the snippet's line numbers then counted.
    pub const UNCAPPED: TextReadOptions = TextReadOptions {
        max_lines: usize::MAX,
        max_line_length: 0,
        force_text: false,
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
    Latin1,
}

pub struct TextPreviewReader;

impl TextPreviewReader {
    /// Lines beyond this are not shown. A minified bundle is one line and a log is millions; both
    /// need a ceiling, and the line ceiling is the one a text control cares about.
    pub const DEFAULT_MAX_LINES: usize = 5_000;

    /// Characters beyond this are folded onto the next line. The line *count* cap above does not
    /// bound the work a text control does, because one line can be the whole budget: a minified
    /// bundle is one, and a binary read under `force_text` almost always is. A single
    /// megabyte-long run in a non-wrapping text control is a stall, so the length is capped here,
    /// beside the count, rather than in the view where no test could reach it. At 4,096 it is
    /// inert for anything a person wrote.
    pub const DEFAULT_MAX_LINE_LENGTH: usize = 4_096;

    /// Whether a read is convincing enough to show as text when nothing *said* it was text.
    ///
    /// The stricter half of the pair. `looks_binary` answers "did this file with a text extension
    /// turn out to be binary?", which only has to catch the obvious case; this answers "we are
    /// guessing — is this worth showing?", and a wrong yes puts a screen of mojibake where an
    /// honest "no preview available" belonged.
    ///
    /// The extra test is the proportion of control characters. A binary file with no NUL in its
    /// first 8 KB gets past the NUL check, but decoding it as Latin-1 turns bytes 0x00–0x1F and
    /// 0x80–0x9F into control characters, and real text has almost none of those; tab, newline
    /// and carriage return being the three that are ordinary and so are not counted.
    pub fn is_convincing_text(preview: &TextPreview) -> bool {
        if preview.looks_binary {
            return false;
        }

        // An empty file is empty text, not a mystery: showing it as a blank text pane is a better
        // answer than refusing it.
        if preview.text.is_empty() {
            return true;
        }

        let mut sample = 0usize;
        let mut control = 0usize;
        for c in preview.text.chars().take(4096) {
            sample += 1;
            if c.is_control() && !matches!(c, '\t' | '\n' | '\r') {
                control += 1;
            }
        }

        control * 100 < sample * 2 // under 2%
    }

    /// Reads at most `byte_budget` bytes and decodes them.
    pub fn read(
        stream: &mut impl Read,
        byte_budget: i64,
        options: &TextReadOptions,
    ) -> TextPreview {
        let budget = byte_budget.clamp(0, i64::from(i32::MAX) - 1) as usize;
        let (bytes, more_remains) = read_at_most(stream, budget);
        Self::decode(&bytes, more_remains, options)
    }

    /// The decoding half of `read`, over bytes somebody else has already read.
    ///
    /// Split out for the content search, which opens thousands of files where the pane opens one
    /// and so cannot afford a fresh `byte[budget + 1]` and the copy out of it per file.
    /// `more_remains` is whether the caller's budget cut the read short; it is passed into the
    /// UTF-8 validation, where a sequence running off the end is evidence of the cut when there
    /// was one and evidence against UTF-8 when there was not.
    pub fn decode(bytes: &[u8], more_remains: bool, options: &TextReadOptions) -> TextPreview {
        if bytes.is_empty() {
            return TextPreview {
                text: String::new(),
                encoding_name: "UTF-8",
                line_ending: "",
                line_count: 0,
                truncated: more_remains,
                looks_binary: false,
            };
        }

        let Some((encoding, name, bom_length)) =
            detect_encoding(bytes, more_remains, options.force_text)
        else {
            return TextPreview {
                text: String::new(),
                encoding_name: "Binary",
                line_ending: "",
                line_count: 0,
                truncated: more_remains,
                looks_binary: true,
            };
        };

        let mut body = &bytes[bom_length..];

        // A budget cut lands wherever it lands, which for a multi-byte encoding is often mid
        // character. Trimming the partial tail is cheaper than explaining the replacement glyph
        // that would otherwise appear at the end of every truncated preview.
        if more_remains {
            body = trim_partial_tail(body, encoding);
        }

        let text = decode_bytes(body, encoding);
        let line_ending = detect_line_ending(&text);
        let mut text = normalise(text);
        if options.force_text {
            text = dot(text);
        }
        text = fold_long_lines(text, options.max_line_length);

        let mut truncated = more_remains;
        let mut line_count = count_lines(&text);
        if line_count > options.max_lines {
            text = take_lines(text, options.max_lines);
            line_count = options.max_lines;
            truncated = true;
        }

        TextPreview {
            text,
            encoding_name: name,
            line_ending,
            line_count,
            truncated,
            looks_binary: false,
        }
    }
}

/// Reads up to `budget` bytes, plus one more purely to answer "is there more?" without seeking or
/// asking the caller for a length it may not have.
pub(crate) fn read_at_most(stream: &mut impl Read, budget: usize) -> (Vec<u8>, bool) {
    let mut buffer = vec![0u8; budget + 1];
    let mut total = 0;
    while total < buffer.len() {
        match stream.read(&mut buffer[total..]) {
            Ok(0) => break,
            Ok(read) => total += read,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }

    let more_remains = total > budget;
    buffer.truncate(total.min(budget));
    (buffer, more_remains)
}

/// None means the bytes are not text.
fn detect_encoding(
    bytes: &[u8],
    truncated: bool,
    force_text: bool,
) -> Option<(Encoding, &'static str, usize)> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some((Encoding::Utf8, "UTF-8 (BOM)", 3));
    }
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        return Some((Encoding::Utf32Le, "UTF-32 LE", 4));
    }
    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return Some((Encoding::Utf32Be, "UTF-32 BE", 4));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Some((Encoding::Utf16Le, "UTF-16 LE", 2));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Some((Encoding::Utf16Be, "UTF-16 BE", 2));
    }

    if let Some(bomless) = detect_bomless_utf16(bytes) {
        return Some(bomless);
    }

    // Raw mode skips this rung and only this one. The BOM checks and the bomless-UTF-16
    // heuristic above still ran, so a forced read of a UTF-16 file is still UTF-16; what is given
    // up is the right to say "binary", which is exactly what raw mode was asked for.
    if !force_text && has_nul(bytes) {
        return None;
    }

    Some(if is_valid_utf8(bytes, truncated) {
        (Encoding::Utf8, "UTF-8", 0)
    } else {
        (Encoding::Latin1, "Latin-1", 0)
    })
}

/// UTF-16 with no BOM is what most Windows tools emit, and it is the one case where NUL bytes mean
/// text rather than binary. The tell is that they are all on one parity: three quarters of the
/// high bytes zero and essentially none of the low bytes, which is ASCII-range text in a two-byte
/// encoding, not a binary file that happens to have zeros.
fn detect_bomless_utf16(bytes: &[u8]) -> Option<(Encoding, &'static str, usize)> {
    let sample = bytes.len().min(8192);
    if sample < 4 {
        return None;
    }

    let (mut even_nuls, mut odd_nuls) = (0usize, 0usize);
    for (i, &b) in bytes[..sample].iter().enumerate() {
        if b != 0 {
            continue;
        }
        if i % 2 == 0 {
            even_nuls += 1;
        } else {
            odd_nuls += 1;
        }
    }

    let pairs = sample / 2;
    if pairs == 0 {
        return None;
    }

    if odd_nuls * 4 >= pairs * 3 && even_nuls * 20 < pairs {
        return Some((Encoding::Utf16Le, "UTF-16 LE", 0));
    }
    if even_nuls * 4 >= pairs * 3 && odd_nuls * 20 < pairs {
        return Some((Encoding::Utf16Be, "UTF-16 BE", 0));
    }
    None
}

fn has_nul(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8192)].contains(&0)
}

/// `truncated`: a sequence running off the end is evidence of the cut when there was one, and
/// evidence against UTF-8 when there was not, which is the difference between reading a Latin-1
/// file correctly and mangling it.
fn is_valid_utf8(bytes: &[u8], truncated: bool) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        let continuations = match bytes[i] {
            0x00..=0x7F => {
                i += 1;
                continue;
            }
            0xC2..=0xDF => 1,
            0xE0..=0xEF => 2,
            0xF0..=0xF4 => 3,
            _ => return false,
        };

        if i + continuations >= bytes.len() {
            return truncated;
        }

        for k in 1..=continuations {
            if bytes[i + k] & 0xC0 != 0x80 {
                return false;
            }
        }
        i += continuations + 1;
    }
    true
}

fn trim_partial_tail(body: &[u8], encoding: Encoding) -> &[u8] {
    match encoding {
        Encoding::Utf32Le | Encoding::Utf32Be => &body[..body.len() - body.len() % 4],
        Encoding::Utf16Le | Encoding::Utf16Be => &body[..body.len() - body.len() % 2],
        Encoding::Latin1 => body,
        Encoding::Utf8 => {
            // Walk back over continuation bytes to the lead byte, and drop the sequence if the
            // bytes it needs are not all here.
            let end = body.len();
            let mut back = 0;
            while end > back && back < 3 && body[end - 1 - back] & 0xC0 == 0x80 {
                back += 1;
            }
            if end <= back {
                return body;
            }
            let lead_index = end - 1 - back;
            let needed = match body[lead_index] {
                0xF0..=0xFF => 4,
                0xE0..=0xEF => 3,
                0xC0..=0xDF => 2,
                _ => 1,
            };
            if needed > 1 && end - lead_index < needed {
                &body[..lead_index]
            } else {
                body
            }
        }
    }
}

fn decode_bytes(body: &[u8], encoding: Encoding) -> String {
    match encoding {
        Encoding::Utf8 => String::from_utf8_lossy(body).into_owned(),
        Encoding::Latin1 => body.iter().map(|&b| b as char).collect(),
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let units: Vec<u16> = body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| {
                    if encoding == Encoding::Utf16Le {
                        u16::from_le_bytes(*p)
                    } else {
                        u16::from_be_bytes(*p)
                    }
                })
                .collect();
            String::from_utf16_lossy(&units)
        }
        Encoding::Utf32Le | Encoding::Utf32Be => body
            .as_chunks::<4>()
            .0
            .iter()
            .map(|q| {
                let v = if encoding == Encoding::Utf32Le {
                    u32::from_le_bytes(*q)
                } else {
                    u32::from_be_bytes(*q)
                };
                char::from_u32(v).unwrap_or('\u{FFFD}')
            })
            .collect(),
    }
}

fn detect_line_ending(text: &str) -> &'static str {
    let (mut crlf, mut lf, mut cr) = (false, false, false);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    crlf = true;
                    i += 1;
                } else {
                    cr = true;
                }
            }
            b'\n' => lf = true,
            _ => {}
        }
        i += 1;
    }
    match (crlf as u8 + lf as u8 + cr as u8, crlf, lf) {
        (0, _, _) => "",
        (1, true, _) => "CRLF",
        (1, _, true) => "LF",
        (1, _, _) => "CR",
        _ => "Mixed",
    }
}

/// Every line ending becomes `\n`, so offsets into the text mean one thing: the tokenizer's spans
/// and the view's line numbers both index this string.
fn normalise(text: String) -> String {
    if !text.contains('\r') {
        return text;
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Raw mode only: every control character but tab and newline becomes a dot, the way the hex
/// dump's ASCII column already spells them.
///
/// This is not cosmetic. A text layout treats U+000B, U+000C, U+0085, U+2028 and U+2029 as line
/// breaks in their own right, and Latin-1 turns byte 0x85 into the third of those, so a binary
/// read of any file containing one renders more rows than the string has lines, and the gutter
/// beside it stops lining up from that point down. It is also the honest rendering: in a file
/// being read as bytes, 0x85 is a byte, not a paragraph mark.
///
/// Ordinary text is deliberately left alone. There those characters really are separators, and
/// they are rare enough that dotting them would cost more than it saved.
fn dot(text: String) -> String {
    if !text.chars().any(is_unshowable) {
        return text;
    }
    text.chars()
        .map(|c| if is_unshowable(c) { '.' } else { c })
        .collect()
}

fn is_unshowable(c: char) -> bool {
    // `is_control` covers C0 and C1 (which is where 0x85 lives) but not the two Unicode
    // separators, and a text layout breaks a line on those just as readily.
    (c.is_control() && !matches!(c, '\t' | '\n')) || matches!(c, '\u{2028}' | '\u{2029}')
}

/// Breaks any line longer than `max_length` characters into pieces of that length. Runs after
/// `normalise` so there is one line terminator to look for, and before the line count, so the
/// count describes what will actually be shown.
fn fold_long_lines(text: String, max_length: usize) -> String {
    if max_length == 0 {
        return text;
    }

    // Almost every file is under the cap on every line, and walking once to find that out costs
    // nothing next to building a second string that would be identical.
    let longest = text
        .split('\n')
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    if longest <= max_length {
        return text;
    }

    let mut folded = String::with_capacity(text.len() + text.len() / max_length + 1);
    let mut run = 0;
    for c in text.chars() {
        if c == '\n' {
            folded.push(c);
            run = 0;
            continue;
        }
        if run == max_length {
            folded.push('\n');
            run = 0;
        }
        folded.push(c);
        run += 1;
    }
    folded
}

fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let lines = 1 + text.bytes().filter(|&b| b == b'\n').count();
    // A file ending in a newline has no empty last line worth counting.
    if text.ends_with('\n') {
        lines - 1
    } else {
        lines
    }
}

fn take_lines(text: String, max_lines: usize) -> String {
    let mut seen = 0;
    for (i, b) in text.bytes().enumerate() {
        if b != b'\n' {
            continue;
        }
        seen += 1;
        if seen == max_lines {
            return text[..i].to_string();
        }
    }
    text
}
