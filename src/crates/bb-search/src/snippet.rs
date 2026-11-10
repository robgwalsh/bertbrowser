//! Turns "this file matched" into "line 42, and here it is": pure, so every rule about clipping and
//! counting is testable without a file. Spec: Core/Services/Search/ContentSnippet.cs.

use super::{ContentMatch, ContentText};

pub struct ContentSnippet;

impl ContentSnippet {
    /// How much of a matching line is worth showing in a column. A minified bundle is one line a
    /// megabyte long, and putting it in a list cell would stall the whole window.
    pub const MAX_LINE_CHARS: usize = 300;
    /// How much of the line before the match is kept when clipping: enough to see what the match is
    /// part of, little enough that the needle is still near the left edge.
    pub const LEADING_CONTEXT: usize = 32;

    const ELLIPSIS: &'static str = "\u{2026}";

    /// The first place any needle appears, or None when none does. **First-occurring wins, not
    /// first-listed.**
    pub fn for_content(content: &ContentText, needles: &[String]) -> Option<ContentMatch> {
        let text = &content.text;
        let mut at: Option<usize> = None;
        let mut length_chars = 0;
        for needle in needles {
            let Some(found) = content.index_of(needle) else {
                continue;
            };
            if at.is_some_and(|a| found >= a) {
                continue;
            }
            at = Some(found);
            length_chars = needle.chars().count();
        }
        let at = at?;

        let line_start = if at == 0 {
            0
        } else {
            text[..at].rfind('\n').map_or(0, |i| i + 1)
        };
        let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);

        // The reader normalises every line ending to \n, but a lone \r can still survive inside a
        // line of genuinely mixed endings.
        let line = text[line_start..line_end].trim_end_matches('\r');

        // 1-based, counted over the text as read.
        let line_number = text[..line_start].bytes().filter(|&b| b == b'\n').count() as i32 + 1;

        let offset_chars = text[line_start..at].chars().count();
        let (clipped, clipped_offset) = clip(line, offset_chars, length_chars);

        Some(ContentMatch {
            line_number,
            line: clipped,
            match_start: clipped_offset,
            match_length: length_chars,
        })
    }
}

/// Keeps a window around the match when the line is too long to show, and reports where the match
/// ended up in what is left. Never clips so late that the match falls off the right-hand end.
fn clip(line: &str, offset: usize, _length: usize) -> (String, usize) {
    let total = line.chars().count();
    if total <= ContentSnippet::MAX_LINE_CHARS {
        return (line.to_string(), offset);
    }
    let mut start = offset.saturating_sub(ContentSnippet::LEADING_CONTEXT);
    start = start.min(total.saturating_sub(ContentSnippet::MAX_LINE_CHARS));
    let end = total.min(start + ContentSnippet::MAX_LINE_CHARS);

    let head = if start > 0 {
        ContentSnippet::ELLIPSIS
    } else {
        ""
    };
    let tail = if end < total {
        ContentSnippet::ELLIPSIS
    } else {
        ""
    };
    let window: String = line.chars().skip(start).take(end - start).collect();
    (
        format!("{head}{window}{tail}"),
        offset - start + head.chars().count(),
    )
}
