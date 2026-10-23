//! The decoded text of one file, held just long enough for the query to be asked about it. **Never
//! retain one**: at the candidate ceiling, keeping them would mean tens of thousands of megabyte
//! strings alive until the search ended. Spec: Core/Services/Search/ContentText.cs, Models/ContentScanModels.cs.

use std::sync::LazyLock;

/// Where a `content:` term found its needle: the line, and the match inside it, in characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentMatch {
    /// 1-based, counted over the text as read.
    pub line_number: i32,
    pub line: String,
    /// Offset of the match in `line`, in characters.
    pub match_start: usize,
    /// Length of the match, in characters.
    pub match_length: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentText {
    /// The decoded text, line endings normalised to `\n`.
    pub text: String,
    /// The file was longer than the per-file budget and this is only its front. A miss against a
    /// truncated read is "not in the first megabyte", not "not in the file".
    pub truncated: bool,
}

static NONE: LazyLock<ContentText> = LazyLock::new(|| ContentText {
    text: String::new(),
    truncated: false,
});

impl ContentText {
    pub fn new(text: impl Into<String>, truncated: bool) -> ContentText {
        ContentText {
            text: text.into(),
            truncated,
        }
    }

    /// Nothing to search: a directory, an unreadable file, or one that is not text. Not the same as
    /// an unread candidate: this settles a content term as `No`.
    pub fn none() -> &'static ContentText {
        &NONE
    }

    /// The byte offset where `needle` first appears, ignoring case, or None. Compared with the
    /// simple per-character case fold rather than by folding the whole text, which would allocate a
    /// second megabyte per file.
    pub fn index_of(&self, needle: &str) -> Option<usize> {
        find_ignore_case(&self.text, needle)
    }
}

/// Case-insensitive substring search returning the byte offset in `hay`.
pub fn find_ignore_case(hay: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let folded: Vec<char> = needle.chars().map(fold_char).collect();
    let first = folded[0];
    let hay_chars: Vec<(usize, char)> = hay.char_indices().collect();
    'outer: for (i, &(offset, c)) in hay_chars.iter().enumerate() {
        if fold_char(c) != first {
            continue;
        }
        for (k, &want) in folded.iter().enumerate().skip(1) {
            match hay_chars.get(i + k) {
                Some(&(_, h)) if fold_char(h) == want => {}
                _ => continue 'outer,
            }
        }
        return Some(offset);
    }
    None
}

fn fold_char(c: char) -> char {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}
