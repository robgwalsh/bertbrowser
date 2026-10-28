//! Comparing two files by content: a verdict for any pair, a line diff for two text files, and a hex
//! dump around the first differing byte for anything else. Spec: ViewModels/FileCompareViewModel.cs.
//!
//! Which shape the window takes is `FileComparePlan`'s decision over the pair, not a setting.

use std::io::{Seek, SeekFrom};

use bb_core::cancel::CancelToken;
use bb_core::compare::{
    ContentComparer, ContentComparison, ContentVerdict, FileCompareMode, FileComparePlan,
    FileCompareSide, FileContentComparer,
};
use bb_core::diff::{DiffOp, DiffOptions, TextDiffer};
use bb_core::fs::ReadOnlyFile;
use bb_core::preview::{HexPreviewReader, TextPreviewReader, TextReadOptions};
use serde_json::{Value, json};

use bb_core::format::thousands;

/// How much of each file is read as text before the comparison says so.
const TEXT_BUDGET: i64 = 8 << 20;
/// Rows of hex either side of the first difference.
const HEX_CONTEXT_ROWS: usize = 24;

pub(crate) struct FileCompareSession {
    left: String,
    right: String,
    ignore_whitespace: bool,
    ignore_case: bool,
    forced_text: bool,
    model: Value,
    hunks: Vec<usize>,
    hunk_index: Option<usize>,
}

fn name_of(path: &str) -> String {
    bb_paths::file_name(path).to_string()
}

struct Loaded {
    lines: Vec<String>,
    convincing: bool,
    truncated: bool,
}

fn read_text(path: &str) -> Loaded {
    let Some(mut file) = ReadOnlyFile::try_open(path, false) else {
        return Loaded {
            lines: Vec::new(),
            convincing: false,
            truncated: false,
        };
    };
    let preview = TextPreviewReader::read(
        &mut file,
        TEXT_BUDGET,
        &TextReadOptions {
            max_lines: usize::MAX,
            max_line_length: 0,
            force_text: false,
        },
    );
    let lines = if preview.text.is_empty() {
        Vec::new()
    } else {
        preview.text.split('\n').map(str::to_string).collect()
    };
    Loaded {
        lines,
        convincing: TextPreviewReader::is_convincing_text(&preview),
        truncated: preview.truncated,
    }
}

fn dump(path: &str, start: i64) -> Vec<String> {
    let Some(mut file) = ReadOnlyFile::try_open(path, false) else {
        return Vec::new();
    };
    if start > 0 && file.seek(SeekFrom::Start(start as u64)).is_err() {
        return Vec::new();
    }
    let budget = (HEX_CONTEXT_ROWS * 2 * HexPreviewReader::BYTES_PER_ROW) as i64;
    HexPreviewReader::read(&mut file, budget, HEX_CONTEXT_ROWS * 2, start)
        .rows
        .into_iter()
        .map(|r| r.text)
        .collect()
}

fn describe(c: &ContentComparison) -> String {
    match c.verdict {
        ContentVerdict::Identical => {
            format!("Identical \u{2014} {} bytes", thousands(c.left_bytes))
        }
        ContentVerdict::Unreadable => "Could not be compared".to_string(),
        ContentVerdict::Differs => match c.first_difference_offset {
            Some(offset) => format!(
                "Different \u{2014} first difference at byte {} (left {}, right {} bytes)",
                thousands(offset),
                thousands(c.left_bytes),
                thousands(c.right_bytes)
            ),
            None => format!(
                "Different (left {}, right {} bytes)",
                thousands(c.left_bytes),
                thousands(c.right_bytes)
            ),
        },
    }
}

fn op_name(op: DiffOp) -> &'static str {
    match op {
        DiffOp::Equal => "equal",
        DiffOp::Delete => "delete",
        DiffOp::Insert => "insert",
        DiffOp::Replace => "replace",
    }
}

impl FileCompareSession {
    pub fn new(left: &str, right: &str) -> FileCompareSession {
        let mut s = FileCompareSession {
            left: left.to_string(),
            right: right.to_string(),
            ignore_whitespace: true,
            ignore_case: false,
            forced_text: false,
            model: Value::Null,
            hunks: Vec::new(),
            hunk_index: None,
        };
        s.compare();
        s
    }

    pub fn set_options(&mut self, ignore_whitespace: bool, ignore_case: bool, force_text: bool) {
        self.ignore_whitespace = ignore_whitespace;
        self.ignore_case = ignore_case;
        self.forced_text = force_text;
        self.compare();
    }

    /// Moves to the next (`+1`) or previous (`-1`) difference; the model says which row to scroll to.
    pub fn step(&mut self, direction: i32) {
        if self.hunks.is_empty() {
            return;
        }
        let last = self.hunks.len() - 1;
        self.hunk_index = Some(match self.hunk_index {
            None if direction > 0 => 0,
            None => last,
            Some(i) => (i as i64 + i64::from(direction)).clamp(0, last as i64) as usize,
        });
        self.update_counter();
    }

    fn update_counter(&mut self) {
        let total = self.hunks.len();
        let identical = self.model["identical"].as_bool().unwrap_or(false);
        let counter = if total == 0 {
            if identical {
                String::new()
            } else {
                "\u{2014}".to_string()
            }
        } else if let Some(i) = self.hunk_index {
            format!("Difference {} of {}", i + 1, thousands(total as i64))
        } else {
            format!(
                "{} difference{}",
                thousands(total as i64),
                if total == 1 { "" } else { "s" }
            )
        };
        self.model["counter"] = json!(counter);
        self.model["scrollTo"] = match self.hunk_index {
            Some(i) => json!(self.hunks[i]),
            None => Value::Null,
        };
    }

    fn compare(&mut self) {
        let comparison = FileContentComparer
            .compare(&self.left, &self.right, None, &CancelToken::new())
            .unwrap_or(ContentComparison {
                verdict: ContentVerdict::Unreadable,
                first_difference_offset: None,
                left_bytes: 0,
                right_bytes: 0,
            });
        let mut model = json!({
            "leftPath": self.left, "rightPath": self.right,
            "leftName": name_of(&self.left), "rightName": name_of(&self.right),
            "verdict": describe(&comparison),
            "identical": comparison.identical(),
            "banner": Value::Null,
            "mode": "text",
            "canForceText": false,
            "ignoreWhitespace": self.ignore_whitespace,
            "ignoreCase": self.ignore_case,
            "rows": [],
            "counter": "",
            "scrollTo": Value::Null,
        });
        self.hunks.clear();
        self.hunk_index = None;

        if comparison.verdict == ContentVerdict::Unreadable {
            model["banner"] =
                json!("One of these files could not be read, so they have not been compared.");
            self.model = model;
            return;
        }

        let left = read_text(&self.left);
        let right = read_text(&self.right);
        let mode = FileComparePlan::mode_for(
            &FileCompareSide::new(name_of(&self.left), left.convincing),
            &FileCompareSide::new(name_of(&self.right), right.convincing),
            self.forced_text,
        );

        let mut rows: Vec<Value> = Vec::new();
        if mode == FileCompareMode::Text {
            let l: Vec<&str> = left.lines.iter().map(String::as_str).collect();
            let r: Vec<&str> = right.lines.iter().map(String::as_str).collect();
            let diff = TextDiffer::compare(
                &l,
                &r,
                &DiffOptions {
                    ignore_trailing_whitespace: self.ignore_whitespace,
                    ignore_all_whitespace: false,
                    ignore_case: self.ignore_case,
                },
            );
            for row in &diff.rows {
                rows.push(json!({
                    "op": op_name(row.op),
                    "ln": if row.left_line > 0 { row.left_line.to_string() } else { String::new() },
                    "lt": if row.left_line > 0 { l[row.left_line - 1] } else { "" },
                    "rn": if row.right_line > 0 { row.right_line.to_string() } else { String::new() },
                    "rt": if row.right_line > 0 { r[row.right_line - 1] } else { "" },
                }));
            }
            self.hunks = diff.hunks.iter().map(|h| h.first_row).collect();
            let mut notes = Vec::new();
            if diff.budget_exceeded {
                notes.push("These files are too different to align line by line; they are shown side by side as they are.".to_string());
            }
            if left.truncated || right.truncated {
                notes.push(format!(
                    "Only the first {} MB of each file was read.",
                    TEXT_BUDGET >> 20
                ));
            }
            if !notes.is_empty() {
                model["banner"] = json!(notes.join(" "));
            }
        } else {
            model["mode"] = json!("binary");
            model["canForceText"] = json!(!self.forced_text);
            let start = comparison
                .first_difference_offset
                .map(|o| {
                    (o & !0xF).saturating_sub(
                        (HEX_CONTEXT_ROWS / 2 * HexPreviewReader::BYTES_PER_ROW) as i64,
                    )
                })
                .unwrap_or(0)
                .max(0);
            let l = dump(&self.left, start);
            let r = dump(&self.right, start);
            for i in 0..l.len().max(r.len()) {
                let same = i < l.len() && i < r.len() && l[i] == r[i];
                rows.push(json!({
                    "op": if same { "equal" } else { "replace" },
                    "ln": "", "lt": l.get(i).cloned().unwrap_or_default(),
                    "rn": "", "rt": r.get(i).cloned().unwrap_or_default(),
                }));
            }
            if rows.is_empty() {
                model["banner"] = json!("Neither file could be read as bytes.");
            } else if start > 0 {
                model["banner"] = json!(format!(
                    "Shown from byte {}, around the first difference.",
                    thousands(start)
                ));
            }
        }
        model["rows"] = Value::Array(rows);
        model["hunkCount"] = json!(self.hunks.len());
        model["canStep"] = json!(!self.hunks.is_empty());
        self.model = model;
        self.update_counter();
    }

    pub fn model(&self) -> &Value {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bb-fc-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn two_texts_that_differ_are_diffed_by_line() {
        let dir = temp("text");
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        std::fs::write(&a, "one\ntwo\nthree\n").unwrap();
        std::fs::write(&b, "one\n2\nthree\n").unwrap();
        let s = FileCompareSession::new(a.to_str().unwrap(), b.to_str().unwrap());
        let m = s.model();
        assert_eq!(m["mode"], "text");
        assert!(m["verdict"].as_str().unwrap().starts_with("Different"));
        assert_eq!(m["counter"], "1 difference");
        assert!(
            m["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["op"] == "replace")
        );
    }

    #[test]
    fn identical_files_say_so_and_have_no_counter() {
        let dir = temp("same");
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        std::fs::write(&a, "alpha\nbeta\n").unwrap();
        std::fs::write(&b, "alpha\nbeta\n").unwrap();
        let s = FileCompareSession::new(a.to_str().unwrap(), b.to_str().unwrap());
        assert!(
            s.model()["verdict"]
                .as_str()
                .unwrap()
                .starts_with("Identical")
        );
        assert_eq!(s.model()["counter"], "");
    }

    #[test]
    fn two_binaries_are_dumped_around_the_first_difference() {
        let dir = temp("bin");
        let (a, b) = (dir.join("a.bin"), dir.join("b.bin"));
        let mut bytes = vec![0u8; 8192];
        for (i, x) in bytes.iter_mut().enumerate() {
            *x = (i % 251) as u8 | 0x80;
        }
        std::fs::write(&a, &bytes).unwrap();
        bytes[5000] ^= 0xFF;
        std::fs::write(&b, &bytes).unwrap();
        let s = FileCompareSession::new(a.to_str().unwrap(), b.to_str().unwrap());
        let m = s.model();
        assert_eq!(m["mode"], "binary");
        assert!(
            m["verdict"]
                .as_str()
                .unwrap()
                .contains("first difference at byte 5,000")
        );
        assert!(
            m["banner"]
                .as_str()
                .unwrap()
                .contains("around the first difference")
        );
    }

    #[test]
    fn stepping_walks_the_hunks_and_clamps() {
        let dir = temp("step");
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        let mut left = String::new();
        let mut right = String::new();
        for i in 0..40 {
            left.push_str(&format!("line {i}\n"));
            right.push_str(&if i == 5 || i == 30 {
                format!("CHANGED {i}\n")
            } else {
                format!("line {i}\n")
            });
        }
        std::fs::write(&a, left).unwrap();
        std::fs::write(&b, right).unwrap();
        let mut s = FileCompareSession::new(a.to_str().unwrap(), b.to_str().unwrap());
        assert_eq!(s.model()["counter"], "2 differences");
        s.step(1);
        assert_eq!(s.model()["counter"], "Difference 1 of 2");
        s.step(1);
        s.step(1);
        assert_eq!(s.model()["counter"], "Difference 2 of 2");
    }

    #[test]
    fn an_unreadable_side_is_not_compared() {
        let dir = temp("gone");
        let a = dir.join("a.txt");
        std::fs::write(&a, "x").unwrap();
        let s = FileCompareSession::new(a.to_str().unwrap(), dir.join("missing").to_str().unwrap());
        assert_eq!(s.model()["verdict"], "Could not be compared");
        assert!(
            s.model()["banner"]
                .as_str()
                .unwrap()
                .contains("could not be read")
        );
    }
}
