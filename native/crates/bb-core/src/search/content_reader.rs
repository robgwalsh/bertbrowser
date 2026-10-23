//! Reads one file's text for a content search. Spec: Core/Services/Search/ContentReader.cs.
//!
//! A trait for the reason the hasher has one: it is what lets a cancel land in the *middle* of a
//! file deterministically, in a test that takes milliseconds. **`Unreadable` and `Err(Cancelled)`
//! mean different things, and the difference is load-bearing.** Unreadable is this file having a
//! problem; the scan carries on and reports itself incomplete, as one item's failure never costs
//! the others anywhere else in this app. `Cancelled` is the whole run stopping. Conflate them and
//! a cancel looks like a disk full of unreadable files.

use std::fs::File;
use std::io::Read;

use super::{ContentSearchRules, ContentText};
use crate::cancel::{CancelToken, Cancelled};
use crate::fs::ReadOnlyFile;

/// What reading a file for a content search produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileText {
    /// The file's text, line endings normalised, possibly only its front.
    Text(ContentText),
    /// Nothing to search: it is not text. Not a failure; a folder of images must not report itself
    /// incomplete.
    NotText,
    /// It could not, or must not, be read.
    Unreadable,
}

pub trait ContentReader: Sync {
    fn read(&self, path: &str, max_bytes: u64, cancel: &CancelToken)
    -> Result<FileText, Cancelled>;
}

/// The real one, over the filesystem. Opens through `ReadOnlyFile`, so the share flags, the
/// cloud-placeholder refusal and the reparse-point rule are the ones every other reader uses.
pub struct FileSystemContentReader;

impl ContentReader for FileSystemContentReader {
    fn read(
        &self,
        path: &str,
        max_bytes: u64,
        cancel: &CancelToken,
    ) -> Result<FileText, Cancelled> {
        cancel.check()?;

        let Some(mut file) = ReadOnlyFile::try_open(path, false) else {
            return Ok(FileText::Unreadable);
        };

        let budget = max_bytes.min(i32::MAX as u64 - 1) as usize;

        // One byte past the budget, so "is there more?" is answered without seeking or trusting a
        // length that may be stale.
        let mut buffer = vec![0u8; budget + 1];
        let head_target = ContentSearchRules::HEAD_SAMPLE_BYTES.min(budget + 1);
        let Some(head) = fill(&mut file, &mut buffer, 0, head_target, cancel)? else {
            return Ok(FileText::Unreadable);
        };
        let at_end = head < head_target;

        // Decide from the head whether this is worth reading whole, so a binary costs 8 KB rather
        // than a megabyte. `is_convincing_text` is the stricter of the pair because here we really
        // are guessing: nothing said this file was text.
        let sample = decode(&buffer[..head.min(budget)], !at_end);
        if !is_convincing_text(&sample) {
            return Ok(FileText::NotText);
        }
        if at_end {
            return Ok(FileText::Text(ContentText::new(
                sample.text,
                sample.truncated,
            )));
        }

        let Some(total) = fill(&mut file, &mut buffer, head, budget + 1, cancel)? else {
            return Ok(FileText::Unreadable);
        };
        let more_remains = total > budget;
        let body = decode(&buffer[..total.min(budget)], more_remains);

        // The preview pane's line and line-length caps are deliberately not applied: the first
        // would stop searching at line 5,000 and the second would insert line breaks the snippet's
        // line numbers then counted. Clipping a long line is the snippet's job.
        Ok(if body.looks_binary {
            FileText::NotText
        } else {
            FileText::Text(ContentText::new(body.text, more_remains))
        })
    }
}

/// Reads until `up_to` or end of file. `None` when the read failed: failing part-way is still a
/// failure, since searching the first half of a file and reporting "not found" would be a wrong
/// answer rather than a missing one.
fn fill(
    file: &mut File,
    buffer: &mut [u8],
    from: usize,
    up_to: usize,
    cancel: &CancelToken,
) -> Result<Option<usize>, Cancelled> {
    let mut total = from;
    while total < up_to {
        cancel.check()?;
        match file.read(&mut buffer[total..up_to]) {
            Ok(0) => break,
            Ok(got) => total += got,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Ok(None),
        }
    }
    Ok(Some(total))
}

// --- the decoding ladder ---
// TODO(port): this is `TextPreviewReader.Decode` / `IsConvincingText` (Core/Services/Preview),
// which the content search shares with the preview pane so the grep and the pane cannot come to
// different conclusions about what a file is. When Preview is ported, these move there and this
// module calls them; the rules below are copied from it exactly.

/// What the bytes decoded to.
struct Decoded {
    /// Line endings normalised to `\n`.
    text: String,
    /// The caller's budget cut the read short.
    truncated: bool,
    /// Not text at all; `text` is empty.
    looks_binary: bool,
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

/// The ladder: byte-order mark, bomless UTF-16, NUL-means-binary, strict UTF-8, Latin-1 as the
/// fallback that cannot fail. Latin-1 rather than the machine's ANSI codepage on purpose: it maps
/// every byte, never substitutes, and gives the same answer on every machine.
fn decode(bytes: &[u8], more_remains: bool) -> Decoded {
    if bytes.is_empty() {
        return Decoded {
            text: String::new(),
            truncated: more_remains,
            looks_binary: false,
        };
    }
    let Some((encoding, bom)) = detect_encoding(bytes, more_remains) else {
        return Decoded {
            text: String::new(),
            truncated: more_remains,
            looks_binary: true,
        };
    };
    let mut body = &bytes[bom..];
    // A budget cut lands wherever it lands, which for a multi-byte encoding is often mid character.
    // Trimming the partial tail is cheaper than explaining the replacement glyph.
    if more_remains {
        body = trim_partial_tail(body, encoding);
    }
    let text = normalise(decode_bytes(body, encoding));
    Decoded {
        text,
        truncated: more_remains,
        looks_binary: false,
    }
}

/// None means the bytes are not text.
fn detect_encoding(bytes: &[u8], truncated: bool) -> Option<(Encoding, usize)> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some((Encoding::Utf8, 3));
    }
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        return Some((Encoding::Utf32Le, 4));
    }
    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return Some((Encoding::Utf32Be, 4));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Some((Encoding::Utf16Le, 2));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Some((Encoding::Utf16Be, 2));
    }
    if let Some(bomless) = detect_bomless_utf16(bytes) {
        return Some((bomless, 0));
    }
    if has_nul(bytes) {
        return None;
    }
    Some((
        if is_valid_utf8(bytes, truncated) {
            Encoding::Utf8
        } else {
            Encoding::Latin1
        },
        0,
    ))
}

/// UTF-16 with no BOM is what most Windows tools emit, and it is the one case where NUL bytes mean
/// text rather than binary. The tell is that they are all on one parity: three quarters of the
/// high bytes zero and essentially none of the low bytes.
fn detect_bomless_utf16(bytes: &[u8]) -> Option<Encoding> {
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
        return Some(Encoding::Utf16Le);
    }
    if even_nuls * 4 >= pairs * 3 && odd_nuls * 20 < pairs {
        return Some(Encoding::Utf16Be);
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
        let b = bytes[i];
        let continuations = match b {
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

/// Every line ending becomes `\n`, so offsets into the text mean one thing.
fn normalise(text: String) -> String {
    if !text.contains('\r') {
        return text;
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Whether a read is convincing enough to count as text when nothing *said* it was text. The extra
/// test over the NUL check is the proportion of control characters: a binary with no NUL in its
/// first 8 KB decodes as Latin-1 into a stream of C0/C1 controls, and real text has almost none of
/// those; tab, newline and carriage return are the three that are ordinary and so are not counted.
fn is_convincing_text(decoded: &Decoded) -> bool {
    if decoded.looks_binary {
        return false;
    }
    // An empty file is empty text, not a mystery.
    if decoded.text.is_empty() {
        return true;
    }
    let mut sample = 0usize;
    let mut control = 0usize;
    for c in decoded.text.chars().take(4096) {
        sample += 1;
        if c.is_control() && !matches!(c, '\t' | '\n' | '\r') {
            control += 1;
        }
    }
    control * 100 < sample * 2 // under 2%
}
