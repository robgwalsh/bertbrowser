//! The preview pane's model: what a tab's selection turns into to look at. Spec:
//! PreviewPaneViewModel (Reload, Build*, Explain) in the C# app.
//!
//! Everything here is a blocking read of a file's front, so the host calls it from a worker thread
//! and posts the answer to its own. **No file is held open**: a file is opened sharing everything,
//! read into memory and closed before a byte is decoded, so previewing never blocks a rename, move
//! or delete. A cloud placeholder is refused by the classifier before anything is read. Images and
//! media are the host's (it owns the decoders): this module classifies them and hands back the
//! path, the size and the details line.
//!
//! The answer is JSON, since it crosses the C ABI as text:
//! `{state, title, subtitle, message, canChooseMode, path, lines, spans, footer, entries, ...}`
//! where `state` is the harness's vocabulary (`none`, `text`, `hex`, `image`, `document`, `archive`,
//! `font`, `media`). Spans are `[line, start, length, class]` with starts in UTF-16 units, the unit
//! the host's text layout counts in.

use std::io::{Cursor, Read};

use bb_core::FileTime;
use bb_core::archives::{ArchiveBrowser, ArchiveListing, ArchiveReader, LibraryArchiveReader};
use bb_core::cancel::CancelToken;
use bb_core::format::{byte_size, thousands};
use bb_core::fs::ReadOnlyFile;
use bb_core::preview::{
    HexPreviewReader, PreviewClassifier, PreviewKind, PreviewMode, PreviewRefusal, PreviewRequest,
    PreviewTarget, SyntaxClass, SyntaxSpan, SyntaxTokenizer, TextPreview, TextPreviewReader,
    TextReadOptions,
};
use serde_json::{Value, json};

use crate::app::{Inner, lock};
use crate::ids::{AppResult, TabId};

/// Beyond this many lines the text is still shown, uncoloured: every coloured span is a run in
/// the host's layout, and tens of thousands of them take seconds to lay out.
pub const MAX_COLOURED_LINES: usize = 1_500;

/// The host's mode numbers: 0 auto, 1 raw (text, uncoloured), 2 hex. 3 is the document fallback:
/// the host asked the shell for a preview, got none, and wants the bytes looked at as text.
pub const MODE_AUTO: u8 = 0;
pub const MODE_RAW: u8 = 1;
pub const MODE_HEX: u8 = 2;
pub const MODE_GUESS: u8 = 3;

/// What a selected row contributes to a preview.
struct Picked {
    path: String,
    name: String,
    size: Option<i64>,
    modified: FileTime,
    is_directory: bool,
    type_name: String,
}

fn class_number(class: SyntaxClass) -> u8 {
    match class {
        SyntaxClass::Text => 0,
        SyntaxClass::Keyword => 1,
        SyntaxClass::String => 2,
        SyntaxClass::Comment => 3,
        SyntaxClass::Number => 4,
        SyntaxClass::Punctuation => 5,
    }
}

fn join(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|p| !p.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("  \u{b7}  ")
}

fn explain(refusal: PreviewRefusal, count: usize) -> String {
    match refusal {
        PreviewRefusal::NothingSelected => "Select a file to preview".into(),
        PreviewRefusal::MultipleSelected => format!("{} items selected", thousands(count as i64)),
        PreviewRefusal::TooLarge => "Too large to preview".into(),
        PreviewRefusal::NotDownloaded => {
            "Stored in the cloud and not downloaded.\nPreviewing it would fetch the whole file, so it is left alone."
                .into()
        }
        _ => "No preview available".into(),
    }
}

/// Cuts `text` into lines and gives each its spans, converting the tokenizer's character indices
/// to UTF-16 offsets within the line. The spans cover the text exactly once and in order, so this
/// is one pass with no lookahead; a span straddling a newline is visited once per line.
pub fn split_lines(text: &str, spans: &[SyntaxSpan]) -> (Vec<String>, Vec<[u32; 4]>) {
    if text.is_empty() {
        return (vec![String::new()], Vec::new());
    }
    let mut lines = Vec::new();
    let mut out_spans = Vec::new();
    let mut span_index = 0usize;
    let mut line_start = 0usize; // in chars
    for (number, line) in text.split('\n').enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let line_end = line_start + chars.len();
        let mut utf16: Option<Vec<u32>> = None;
        while span_index < spans.len() && spans[span_index].start < line_end {
            let span = spans[span_index];
            let from = span.start.max(line_start);
            let to = (span.start + span.length).min(line_end);
            if to > from {
                let offsets = utf16.get_or_insert_with(|| {
                    let mut acc = Vec::with_capacity(chars.len() + 1);
                    let mut at = 0u32;
                    acc.push(0);
                    for c in &chars {
                        at += c.len_utf16() as u32;
                        acc.push(at);
                    }
                    acc
                });
                let a = offsets[from - line_start];
                let b = offsets[to - line_start];
                out_spans.push([number as u32, a, b - a, u32::from(class_number(span.class))]);
            }
            if span.start + span.length > line_end {
                break; // continues on the next line
            }
            span_index += 1;
        }
        lines.push(line.to_string());
        line_start = line_end + 1; // the newline is a char of the text
    }
    // A trailing newline ends the last line; it does not start another.
    if text.ends_with('\n') {
        lines.pop();
    }
    (lines, out_spans)
}

/// The bytes of a path's front, from a real file or from inside a container. Bounding the read
/// bounds the decompression, which is what a zip bomb cannot get around.
fn open_bounded(inner: &Inner, path: &str, limit: i64) -> Result<Box<dyn Read>, String> {
    if let Some(entry) = inner.fs.resolve(path)
        && !entry.is_root()
    {
        let password = bb_core::archives::ArchivePasswords::for_archive(
            &*inner.extra.ops.passwords,
            &entry.archive_file,
        );
        let bytes = LibraryArchiveReader::default()
            .read_entry(
                &entry.archive_file,
                &entry.entry_path,
                limit.saturating_add(1),
                password.as_deref(),
                &CancelToken::new(),
            )
            .ok_or_else(|| {
                // An encrypted zip lists in full and only refuses its contents, so this is the
                // ordinary way a preview fails in one, and "could not be read" would send the user
                // looking for a corrupt file rather than at the Unlock button above the list.
                let encrypted = inner
                    .fs
                    .read_archive(&entry.archive_file)
                    .find(&entry.entry_path)
                    .is_some_and(|n| n.is_encrypted);
                if encrypted {
                    "This file is encrypted. Unlock the archive to preview it.".to_string()
                } else {
                    "That entry could not be read from the archive.".to_string()
                }
            })?;
        return Ok(Box::new(Cursor::new(bytes)));
    }
    ReadOnlyFile::try_open(path, false)
        .map(|f| Box::new(f) as Box<dyn Read>)
        .ok_or_else(|| "This file could not be read.".to_string())
}

/// At most `limit` bytes of a path, real or inside a container, read whole: a picture inside a zip
/// is decoded from memory, as is a zip inside a zip listed. Spec: PreviewPaneViewModel.ReadFully.
pub(crate) fn read_bounded(inner: &Inner, path: &str, limit: i64) -> Result<Vec<u8>, String> {
    let reader = open_bounded(inner, path, limit)?;
    let mut bytes = Vec::new();
    reader
        .take(limit.max(0) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "This file could not be read.".to_string())?;
    Ok(bytes)
}

fn attributes_of(path: &str) -> u32 {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .map(|m| m.file_attributes())
        .unwrap_or(0)
}

fn base(picked: Option<&Picked>, count: usize) -> Value {
    let (title, subtitle) = match picked {
        Some(p) => {
            let mut parts = vec![p.type_name.clone()];
            if let Some(size) = p.size
                && !p.is_directory
            {
                parts.push(byte_size(size));
            }
            if !p.modified.is_unknown() {
                parts.push(bb_core::time::format_short_local(p.modified));
            }
            (p.name.clone(), join(&parts))
        }
        None => (String::new(), String::new()),
    };
    json!({
        "state": "none", "title": title, "subtitle": subtitle, "message": Value::Null,
        "canChooseMode": false, "count": count,
        "path": picked.map(|p| p.path.clone()).unwrap_or_default(),
        "sizeBytes": picked.and_then(|p| p.size).unwrap_or(-1),
    })
}

fn with(mut v: Value, extra: Value) -> Value {
    if let (Some(map), Some(more)) = (v.as_object_mut(), extra.as_object()) {
        for (k, val) in more {
            map.insert(k.clone(), val.clone());
        }
    }
    v
}

fn message(mut v: Value, text: &str) -> Value {
    v["message"] = Value::String(text.to_string());
    v
}

fn build_text(inner: &Inner, v: Value, path: &str, plan: &PreviewRequest, guessing: bool) -> Value {
    let mut stream = match open_bounded(inner, path, plan.byte_budget) {
        Ok(s) => s,
        Err(m) => return message(v, &m),
    };
    let raw = plan.mode == PreviewMode::Text;
    let options = TextReadOptions {
        force_text: raw,
        ..TextReadOptions::default()
    };
    let preview: TextPreview = TextPreviewReader::read(&mut stream, plan.byte_budget, &options);

    if guessing {
        if !TextPreviewReader::is_convincing_text(&preview) {
            return message(v, "No preview available");
        }
    } else if preview.looks_binary {
        // Offered, not taken: nothing starts dumping bytes at someone arrowing down a folder of
        // executables. The mode buttons that answer this are already on screen.
        return message(
            v,
            "Binary file \u{2014} nothing to show as text. Switch to Hex to see the bytes.",
        );
    }

    let coloured = preview.line_count <= MAX_COLOURED_LINES;
    let spans = if coloured {
        SyntaxTokenizer::tokenize(&preview.text, plan.language)
    } else {
        Vec::new()
    };
    let footer = join(&[
        if raw { "Raw".into() } else { String::new() },
        preview.encoding_name.to_string(),
        preview.line_ending.to_string(),
        format!("{} lines", thousands(preview.line_count as i64)),
        if preview.truncated {
            "truncated".into()
        } else {
            String::new()
        },
        if coloured {
            String::new()
        } else {
            "colouring off (large file)".into()
        },
    ]);
    let (lines, spans) = split_lines(&preview.text, &spans);
    with(
        v,
        json!({"state": "text", "lines": lines, "spans": spans, "footer": footer}),
    )
}

fn build_hex(inner: &Inner, v: Value, path: &str, plan: &PreviewRequest) -> Value {
    let budget = plan.byte_budget.min(HexPreviewReader::max_bytes(
        HexPreviewReader::DEFAULT_MAX_ROWS,
    ));
    let mut stream = match open_bounded(inner, path, budget) {
        Ok(s) => s,
        Err(m) => return message(v, &m),
    };
    let preview =
        HexPreviewReader::read(&mut stream, budget, HexPreviewReader::DEFAULT_MAX_ROWS, 0);
    if preview.rows.is_empty() {
        return message(v, "This file is empty.");
    }
    let coloured = preview.rows.len() <= MAX_COLOURED_LINES;
    let mut lines = Vec::with_capacity(preview.rows.len());
    let mut spans = Vec::new();
    for (i, row) in preview.rows.iter().enumerate() {
        if coloured {
            for s in &row.spans {
                spans.push([
                    i as u32,
                    s.start as u32,
                    s.length as u32,
                    u32::from(class_number(s.class)),
                ]);
            }
        }
        lines.push(row.text.clone());
    }
    let footer = join(&[
        "Hex".into(),
        format!("{} bytes shown", thousands(preview.bytes_shown)),
        if preview.truncated {
            "truncated".into()
        } else {
            String::new()
        },
        if coloured {
            String::new()
        } else {
            "colouring off (large file)".into()
        },
    ]);
    with(
        v,
        json!({"state": "hex", "lines": lines, "spans": spans, "footer": footer}),
    )
}

fn build_archive(v: Value, path: &str) -> Value {
    let Some(mut file) = ReadOnlyFile::try_open(path, false) else {
        return message(v, "This file could not be read.");
    };
    let contents = ArchiveListing::read(&mut file, ArchiveListing::DEFAULT_MAX_ENTRIES);
    if let Some(error) = &contents.error {
        return message(v, error);
    }
    let footer = join(&[
        format!("{} entries", thousands(i64::from(contents.total_count))),
        format!("{} uncompressed", byte_size(contents.total_bytes)),
        if contents.compression_ratio() > 0.0 {
            format!("{:.0}% saved", contents.compression_ratio() * 100.0)
        } else {
            String::new()
        },
        if contents.truncated {
            format!(
                "showing the first {}",
                thousands(contents.entries.len() as i64)
            )
        } else {
            String::new()
        },
    ]);
    let entries: Vec<Value> = contents
        .entries
        .iter()
        .map(|e| {
            json!({
                "path": e.path, "size": e.size_bytes, "packed": e.compressed_bytes,
                "directory": e.is_directory,
                "modified": if e.modified.is_unknown() { String::new() } else { bb_core::time::format_short_local(e.modified) },
            })
        })
        .collect();
    with(
        v,
        json!({"state": "archive", "entries": entries, "footer": footer}),
    )
}

impl Inner {
    /// The preview of a tab's selection, as JSON (see the module comment). Blocking.
    pub(crate) fn preview_model(&self, tab: TabId, mode: u8) -> AppResult<Value> {
        let cell = self.tab(tab)?;
        let picked: Vec<Picked> = {
            let t = lock(&cell);
            t.listing
                .rows()
                .iter()
                .filter(|r| t.selection.contains(&r.id))
                .map(|r| Picked {
                    path: r.full_path.clone(),
                    name: r.name.clone(),
                    size: r.size,
                    modified: r.modified,
                    is_directory: r.is_directory,
                    type_name: r.type_name.clone(),
                })
                .collect()
        };
        let budget = i64::from(lock(&self.settings).preview_text_max_bytes).max(4096);
        Ok(self.preview_of(&picked, mode, budget))
    }

    /// The preview of one path (the metadata and the tests have no tab to select it in).
    pub(crate) fn preview_path(&self, path: &str, mode: u8) -> Value {
        let is_directory = std::path::Path::new(path).is_dir();
        let meta = std::fs::metadata(path).ok();
        let picked = Picked {
            path: path.to_string(),
            name: bb_paths::file_name(path).to_string(),
            size: meta
                .as_ref()
                .filter(|_| !is_directory)
                .map(|m| m.len() as i64),
            modified: meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .map(FileTime::from_system_time)
                .unwrap_or(FileTime::UNKNOWN),
            is_directory,
            type_name: String::new(),
        };
        let budget = i64::from(lock(&self.settings).preview_text_max_bytes).max(4096);
        self.preview_of(&[picked], mode, budget)
    }

    fn preview_of(&self, picked: &[Picked], mode: u8, budget: i64) -> Value {
        let count = picked.len();
        let one = (count == 1).then(|| &picked[0]);
        let v = base(one, count);

        let Some(item) = one else {
            return message(
                v,
                &explain(
                    if count == 0 {
                        PreviewRefusal::NothingSelected
                    } else {
                        PreviewRefusal::MultipleSelected
                    },
                    count,
                ),
            );
        };
        if item.is_directory {
            let text = match item.size {
                Some(bytes) => format!("Folder  \u{b7}  {}", byte_size(bytes)),
                None => "Folder  \u{b7}  size not indexed yet".to_string(),
            };
            return message(v, &text);
        }

        let wanted = match mode {
            MODE_RAW => PreviewMode::Text,
            MODE_HEX => PreviewMode::Hex,
            _ => PreviewMode::Auto,
        };
        let target = PreviewTarget {
            name: item.name.clone(),
            size_bytes: item.size.unwrap_or(0),
            attributes: attributes_of(&item.path),
            is_directory: false,
        };
        let mut plan = PreviewClassifier::classify_one(&target, budget, wanted);
        if mode == MODE_GUESS {
            // The shell declined: a document is read as text on spec.
            plan.kind = PreviewKind::Document;
        }
        if plan.is_refused() {
            return message(v, &explain(plan.refusal, count));
        }

        let in_archive = self.fs.resolve(&item.path).is_some_and(|e| !e.is_root());
        let v = with(v, json!({"canChooseMode": true}));
        match plan.kind {
            PreviewKind::Hex => build_hex(self, v, &item.path, &plan),
            // The host decodes the picture from the entry's bytes (`App::preview_read`).
            PreviewKind::Image => with(v, json!({"state": "image", "inArchive": in_archive})),
            PreviewKind::Text => build_text(self, v, &item.path, &plan, false),
            // A zip inside a zip is no virtual path at all (`ArchivePath::parse` refuses it), so
            // it is recognised by its folder.
            PreviewKind::Archive
                if item
                    .path
                    .rsplit_once('\\')
                    .is_some_and(|(folder, _)| self.fs.resolve(folder).is_some()) =>
            {
                message(v, "Extract this archive to preview it.")
            }
            PreviewKind::Archive => build_archive(v, &item.path),
            PreviewKind::Font if in_archive => message(v, "Extract this font to preview it."),
            PreviewKind::Font => with(v, json!({"state": "font"})),
            PreviewKind::Media if in_archive => message(v, "Extract this file to play it."),
            PreviewKind::Media => with(
                v,
                json!({"state": "media", "isVideo": PreviewClassifier::is_video(&item.name)}),
            ),
            // The shell goes first in the host (a .docx reads as gibberish but has a page-one
            // thumbnail); the bytes get a look only when it declines, by asking again with
            // `MODE_GUESS`.
            _ if in_archive || mode == MODE_GUESS => build_text(self, v, &item.path, &plan, true),
            _ => with(
                v,
                json!({"state": "document", "textBudget": plan.byte_budget}),
            ),
        }
    }
}

impl crate::app::App {
    /// What the preview pane shows for the tab's selection under `mode` (`MODE_*`), as JSON.
    /// Blocking; call it from a worker.
    pub fn preview_build(&self, tab: TabId, mode: u8) -> AppResult<String> {
        Ok(self.inner.preview_model(tab, mode)?.to_string())
    }

    /// As `preview_build` for one path that need not be selected anywhere.
    pub fn preview_build_path(&self, path: &str, mode: u8) -> String {
        self.inner.preview_path(path, mode).to_string()
    }

    /// A picture's bytes for the host to decode, from disk or from inside a container (bounded
    /// like every preview read). Blocking; call it from a worker.
    pub fn preview_read(&self, path: &str) -> AppResult<Vec<u8>> {
        read_bounded(&self.inner, path, PreviewClassifier::MAX_IMAGE_BYTES)
            .map_err(crate::ids::AppError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::preview::{SyntaxLanguage, SyntaxTokenizer};

    #[test]
    fn lines_are_cut_and_a_trailing_newline_is_not_a_line() {
        let (lines, spans) = split_lines("a\nbb\n", &[]);
        assert_eq!(lines, vec!["a", "bb"]);
        assert!(spans.is_empty());
        assert_eq!(split_lines("", &[]).0, vec![String::new()]);
        assert_eq!(split_lines("\n", &[]).0, vec![String::new()]);
    }

    #[test]
    fn spans_land_on_their_own_line_in_utf16_units() {
        // A letter outside the BMP is two UTF-16 units and one character: the host counts the former.
        let text = "x \u{1F600} // note\nint y;\n";
        let spans = SyntaxTokenizer::tokenize(text, SyntaxLanguage::CSharp);
        let (lines, out) = split_lines(text, &spans);
        assert_eq!(lines.len(), 2);
        // The comment starts after "x ", the emoji (2 units) and a space: unit 5, five chars of "// no".
        let comment = out
            .iter()
            .find(|s| s[0] == 0 && s[3] == 3)
            .expect("a comment span");
        assert_eq!(comment[1], 5);
        assert_eq!(comment[2], 7);
        assert!(
            out.iter().any(|s| s[0] == 1 && s[3] == 1),
            "`int` is a keyword on the second line"
        );
        assert!(
            out.iter().all(|s| (s[0] as usize) < lines.len()),
            "no span names a line that is not there"
        );
    }

    #[test]
    fn a_span_that_straddles_a_newline_is_cut_at_it() {
        let text = "/* one\ntwo */ x";
        let spans = SyntaxTokenizer::tokenize(text, SyntaxLanguage::CSharp);
        let (lines, out) = split_lines(text, &spans);
        assert_eq!(lines, vec!["/* one", "two */ x"]);
        let comments: Vec<_> = out.iter().filter(|s| s[3] == 3).collect();
        assert_eq!(comments.len(), 2);
        assert_eq!((comments[0][0], comments[0][1], comments[0][2]), (0, 0, 6));
        assert_eq!((comments[1][0], comments[1][1], comments[1][2]), (1, 0, 6));
    }
}
