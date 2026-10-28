//! Reads one file's text for a content search. Spec: Core/Services/Search/ContentReader.cs.
//!
//! A trait for the reason the hasher has one: it is what lets a cancel land in the *middle* of a
//! file deterministically, in a test that takes milliseconds. **`Unreadable` and `Err(Cancelled)`
//! mean different things, and the difference is load-bearing.** Unreadable is this file having a
//! problem; the scan carries on and reports itself incomplete, as one item's failure never costs
//! the others anywhere else in this app. `Cancelled` is the whole run stopping. Conflate them and
//! a cancel looks like a disk full of unreadable files.
//!
//! The decoding is `TextPreviewReader`'s, which the content search shares with the preview pane
//! so the grep and the pane cannot come to different conclusions about what a file is.

use std::fs::File;
use std::io::Read;

use super::{ContentSearchRules, ContentText};
use crate::cancel::{CancelToken, Cancelled};
use crate::fs::ReadOnlyFile;
use crate::preview::{TextPreviewReader, TextReadOptions};

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
        let sample = TextPreviewReader::decode(
            &buffer[..head.min(budget)],
            !at_end,
            &TextReadOptions::UNCAPPED,
        );
        if !TextPreviewReader::is_convincing_text(&sample) {
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

        // The preview pane's line and line-length caps are deliberately not applied: the first
        // would stop searching at line 5,000 and the second would insert line breaks the snippet's
        // line numbers then counted. Clipping a long line is the snippet's job.
        let body = TextPreviewReader::decode(
            &buffer[..total.min(budget)],
            more_remains,
            &TextReadOptions::UNCAPPED,
        );

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
