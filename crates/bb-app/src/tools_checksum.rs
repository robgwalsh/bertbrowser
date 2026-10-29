//! The checksum window's state: files hashed under the algorithms ticked, a verify run over a
//! checksum file, the paste box, and writing a checksum file back out.
//! Spec: ViewModels/ChecksumViewModel.cs.
//!
//! Digests are uppercase inside; the case a digest is *shown* in is decided by
//! `ChecksumAlgorithms::is_written_lowercase` and nowhere else. A checksum file is untrusted input:
//! every name in one goes through `ChecksumPath::resolve` before it becomes a path to open.

use std::collections::{BTreeMap, HashMap};

use bb_core::cancel::CancelToken;
use bb_core::checksums::{
    ChecksumAlgorithm, ChecksumAlgorithms, ChecksumCompare, ChecksumFile, ChecksumFileParse,
    ChecksumLine, ChecksumPath, ChecksumRunner, ChecksumVerify, ChecksumVerifyState,
    FileSystemFileHasher,
};
use serde_json::{Value, json};

use crate::ids::{AppError, AppResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Hash,
    Verify,
}

struct Row {
    path: String,
    name: String,
    computed: BTreeMap<usize, String>,
    error: Option<String>,
    verify_state: Option<ChecksumVerifyState>,
    verify_detail: Option<String>,
}

fn slot(a: ChecksumAlgorithm) -> usize {
    ChecksumAlgorithms::ALL
        .iter()
        .position(|x| *x == a)
        .unwrap_or(0)
}

fn shown(algorithm: ChecksumAlgorithm, digest: &str) -> String {
    if ChecksumAlgorithms::is_written_lowercase(algorithm) {
        digest.to_lowercase()
    } else {
        digest.to_uppercase()
    }
}

/// The algorithms a settings string names, in table order; Sha256 when it names none.
pub(crate) fn parse_algorithms(text: &str) -> Vec<ChecksumAlgorithm> {
    let found: Vec<_> = text
        .split(',')
        .filter_map(|t| ChecksumAlgorithm::parse_ignore_case(t.trim()))
        .collect();
    if found.is_empty() {
        vec![ChecksumAlgorithm::Sha256]
    } else {
        ChecksumAlgorithms::normalise(found)
    }
}

pub(crate) struct ChecksumSession {
    mode: Mode,
    title: String,
    rows: Vec<Row>,
    selected: Vec<ChecksumAlgorithm>,
    has_run: bool,
    banner: Option<String>,
    expected: String,
    paste_verdict: Option<String>,
    paste_matched: bool,
    verify_summary: Option<String>,
    verifying: Option<(ChecksumFileParse, String)>,
}

fn file_name_of(path: &str) -> String {
    let name = bb_paths::file_name(path.trim_end_matches(['\\', '/']));
    if name.is_empty() {
        path.to_string()
    } else {
        name.to_string()
    }
}

impl ChecksumSession {
    fn blank(mode: Mode, selected: Vec<ChecksumAlgorithm>) -> ChecksumSession {
        ChecksumSession {
            mode,
            title: "Checksums".into(),
            rows: Vec::new(),
            selected,
            has_run: false,
            banner: None,
            expected: String::new(),
            paste_verdict: None,
            paste_matched: false,
            verify_summary: None,
            verifying: None,
        }
    }

    pub fn hash(paths: &[String], selected: Vec<ChecksumAlgorithm>) -> ChecksumSession {
        let mut s = ChecksumSession::blank(Mode::Hash, selected);
        s.title = if paths.len() == 1 {
            "Checksum".into()
        } else {
            format!(
                "Checksums \u{2014} {} files",
                bb_core::format::thousands(paths.len() as i64)
            )
        };
        s.rows = paths
            .iter()
            .map(|p| Row {
                path: p.clone(),
                name: file_name_of(p),
                computed: BTreeMap::new(),
                error: None,
                verify_state: None,
                verify_detail: None,
            })
            .collect();
        s.run(false);
        s
    }

    pub fn verify(checksum_file: &str, selected: Vec<ChecksumAlgorithm>) -> ChecksumSession {
        let mut s = ChecksumSession::blank(Mode::Verify, selected);
        let file_name = file_name_of(checksum_file);
        s.title = format!("Verify \u{2014} {file_name}");
        let text = match std::fs::read(checksum_file) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => {
                s.banner = Some("That checksum file could not be read.".into());
                return s;
            }
        };
        let folder = bb_paths::split_last(checksum_file).0.to_string();
        let parse = ChecksumFile::parse(&text, Some(&file_name));
        if parse.lines.is_empty() {
            s.banner = Some(if parse.problems.is_empty() {
                format!("{file_name} lists nothing.")
            } else {
                format!("No checksum lines could be read from {file_name}.")
            });
            return s;
        }
        // The file's own algorithm is what it must be checked against; the ticks describe the other
        // mode, and checking against a different one would be checking it against the wrong thing.
        s.selected = vec![parse.algorithm.unwrap_or(ChecksumAlgorithm::Sha256)];
        s.banner = (!parse.problems.is_empty()).then(|| {
            format!(
                "{} line(s) in {file_name} could not be read and are not checked.",
                bb_core::format::thousands(parse.problems.len() as i64)
            )
        });
        for line in &parse.lines {
            let resolved = ChecksumPath::resolve(&folder, &line.name);
            let mut row = Row {
                path: resolved.clone().unwrap_or_else(|| line.name.clone()),
                name: line.name.clone(),
                computed: BTreeMap::new(),
                error: None,
                verify_state: None,
                verify_detail: None,
            };
            if resolved.is_none() {
                row.verify_state = Some(ChecksumVerifyState::Refused);
                row.verify_detail =
                    Some("This name points outside the folder and was not read.".into());
            }
            s.rows.push(row);
        }
        s.verifying = Some((parse, folder));
        s.run(false);
        s
    }

    fn is_refused(row: &Row) -> bool {
        row.verify_state == Some(ChecksumVerifyState::Refused)
    }

    /// Reads whatever is missing (everything, when forced). Ticking a fifth box after a run over a
    /// big folder must not re-read the other four algorithms' worth of bytes.
    pub fn run(&mut self, force: bool) {
        if self.selected.is_empty() || self.rows.is_empty() {
            return;
        }
        let wanted = self.selected.clone();
        let missing: Vec<ChecksumAlgorithm> = if force {
            wanted
        } else {
            wanted
                .into_iter()
                .filter(|a| {
                    self.rows
                        .iter()
                        .any(|r| !Self::is_refused(r) && !r.computed.contains_key(&slot(*a)))
                })
                .collect()
        };
        if missing.is_empty() {
            self.project();
            return;
        }
        let targets: Vec<usize> = (0..self.rows.len())
            .filter(|i| !Self::is_refused(&self.rows[*i]))
            .collect();
        let paths: Vec<String> = targets.iter().map(|i| self.rows[*i].path.clone()).collect();
        let sizes: HashMap<String, i64> = paths
            .iter()
            .filter_map(|p| {
                std::fs::metadata(p)
                    .ok()
                    .map(|m| (p.clone(), m.len() as i64))
            })
            .collect();
        let hasher = FileSystemFileHasher::new();
        let outcome = ChecksumRunner::new(&hasher).run(
            &paths,
            &missing,
            Some(&sizes),
            None,
            &CancelToken::new(),
        );
        for (k, result) in outcome.rows.iter().enumerate() {
            let Some(&i) = targets.get(k) else { break };
            let row = &mut self.rows[i];
            match &result.digests {
                None => {
                    row.error = Some(
                        "Could not be read \u{2014} it may be locked, gone, or a cloud file that is not on this device."
                            .into(),
                    );
                }
                Some(d) => {
                    row.error = None;
                    for (a, digest) in &d.by_algorithm {
                        row.computed.insert(slot(*a), digest.clone());
                    }
                }
            }
        }
        self.has_run = true;
        self.project();
        self.reconcile();
    }

    fn project(&mut self) {
        self.evaluate_paste();
    }

    pub fn set_algorithms(&mut self, algorithms: Vec<ChecksumAlgorithm>) {
        if self.mode == Mode::Verify {
            // Verify checks against the file's own algorithm; nothing here can change that.
            return;
        }
        self.selected = ChecksumAlgorithms::normalise(algorithms);
        self.run(false);
        self.project();
    }

    pub fn set_expected(&mut self, text: &str) {
        self.expected = text.to_string();
        self.evaluate_paste();
    }

    fn reconcile(&mut self) {
        if self.mode != Mode::Verify {
            return;
        }
        let Some((parse, _)) = &self.verifying else {
            return;
        };
        let algorithm = parse.algorithm.unwrap_or(ChecksumAlgorithm::Sha256);
        let mut computed: HashMap<String, Option<String>> = HashMap::new();
        let mut refused: Vec<String> = Vec::new();
        for row in &self.rows {
            if Self::is_refused(row) {
                refused.push(row.name.clone());
                continue;
            }
            if let Some(d) = row.computed.get(&slot(algorithm)) {
                computed.insert(row.name.clone(), Some(d.clone()));
            } else if row.error.is_some() && std::path::Path::new(&row.path).is_file() {
                computed.insert(row.name.clone(), None);
            }
        }
        let report = ChecksumVerify::reconcile(&parse.lines, &computed, &refused, &[]);
        let summary = ChecksumVerify::summarise(&report);
        for row in &mut self.rows {
            if Self::is_refused(row) {
                continue;
            }
            let Some(result) = report.iter().find(|r| r.name == row.name) else {
                continue;
            };
            row.verify_state = Some(result.state);
            row.verify_detail = match result.state {
                ChecksumVerifyState::Mismatch => result
                    .expected
                    .as_ref()
                    .map(|e| format!("expected {}", e.to_lowercase())),
                ChecksumVerifyState::Missing => Some("not found in this folder".into()),
                ChecksumVerifyState::Unreadable => Some("could not be read".into()),
                _ => None,
            };
            // The state and its detail already say why there is no digest, and say it better.
            if result.state != ChecksumVerifyState::Ok {
                row.error = None;
            }
        }
        self.verify_summary = Some(summary);
    }

    fn evaluate_paste(&mut self) {
        self.paste_matched = false;
        let pasted = self.expected.trim().to_string();
        if pasted.is_empty() || self.rows.is_empty() {
            self.paste_verdict = None;
            return;
        }
        let Some(digest_text) = ChecksumCompare::extract_digest(&pasted) else {
            self.paste_verdict = Some("That does not look like a checksum.".into());
            return;
        };
        let digest_text = digest_text.to_string();
        if let Some(needed) = ChecksumAlgorithms::recognise(&digest_text)
            && !self.selected.contains(&needed)
            && self.mode == Mode::Hash
        {
            // The paste has said which algorithm is being worked in: tick it and compute it, rather
            // than asking twice.
            let mut next = self.selected.clone();
            next.push(needed);
            self.selected = ChecksumAlgorithms::normalise(next);
            self.run(false);
            return;
        }
        let mut first_match: Option<String> = None;
        for row in &self.rows {
            for a in &self.selected {
                if let Some(d) = row.computed.get(&slot(*a))
                    && d.eq_ignore_ascii_case(&digest_text)
                {
                    first_match.get_or_insert_with(|| row.name.clone());
                }
            }
        }
        self.paste_matched = first_match.is_some();
        self.paste_verdict = Some(match (&first_match, self.rows.len()) {
            (Some(_), 1) => "\u{2713} Matches".to_string(),
            (Some(name), _) => format!("\u{2713} Matches {name}"),
            (None, 1) => "\u{2717} Does not match".to_string(),
            (None, _) => "\u{2717} Matches nothing here".to_string(),
        });
    }

    /// Writes a checksum file listing every row's digest for `algorithm`, relative to the folder it
    /// is saved into. Returns a note to show (files left out), or the reason it failed.
    pub fn save(&self, path: &str, algorithm: ChecksumAlgorithm) -> Result<Option<String>, String> {
        let folder = bb_paths::split_last(path).0.to_string();
        if folder.is_empty() {
            return Err("That is not a folder a checksum file can be written to.".into());
        }
        let mut lines = Vec::new();
        let mut outside = 0;
        for row in &self.rows {
            let Some(digest) = row.computed.get(&slot(algorithm)) else {
                continue;
            };
            match ChecksumPath::relativise(&folder, &row.path) {
                Some(name) => lines.push(ChecksumLine::new(&name, digest)),
                None => outside += 1,
            }
        }
        if lines.is_empty() {
            return Err(if outside > 0 {
                "None of these files are under the folder the checksum file is being saved to, so nothing could be listed relative to it.".into()
            } else {
                format!(
                    "No {} digests have been computed yet.",
                    ChecksumAlgorithms::display_name(algorithm)
                )
            });
        }
        std::fs::write(path, ChecksumFile::render(algorithm, &lines))
            .map_err(|e| format!("The checksum file could not be written: {e}"))?;
        Ok((outside > 0)
            .then(|| format!("{outside} file(s) are not under this folder and were left out.")))
    }

    pub fn model(&self) -> Value {
        let empty_message = if self.has_run {
            "Nothing to show."
        } else {
            "Select files and press Hash."
        };
        json!({
            "mode": if self.mode == Mode::Hash { "hash" } else { "verify" },
            "title": self.title,
            "hasRun": self.has_run,
            "banner": self.banner,
            "expected": self.expected,
            "pasteVerdict": self.paste_verdict,
            "pasteMatched": self.paste_matched,
            "verifySummary": self.verify_summary,
            "emptyMessage": if self.rows.is_empty() { empty_message } else { "" },
            "canCompare": self.mode == Mode::Hash && !self.rows.is_empty(),
            "canSave": self.rows.iter().any(|r| !r.computed.is_empty()),
            "algorithms": ChecksumAlgorithms::ALL.iter().map(|a| json!({
                "id": a.name(),
                "label": ChecksumAlgorithms::display_name(*a),
                "note": (*a == ChecksumAlgorithm::Crc32)
                    .then_some("for .sfv files; catches corruption, not tampering"),
                "selected": self.selected.contains(a),
                "extension": ChecksumAlgorithms::file_extension(*a),
            })).collect::<Vec<_>>(),
            "rows": self.rows.iter().map(|r| {
                let paste = ChecksumCompare::extract_digest(self.expected.trim())
                    .map(str::to_string);
                json!({
                    "name": r.name,
                    "path": r.path,
                    "error": r.error,
                    "verifyState": r.verify_state.map(|s| format!("{s:?}")),
                    "verifyDetail": r.verify_detail,
                    "computed": ChecksumAlgorithms::ALL.iter()
                        .filter_map(|a| r.computed.get(&slot(*a)).map(|d| (a.name().to_string(), json!(d))))
                        .collect::<serde_json::Map<_, _>>(),
                    "digests": self.selected.iter()
                        .filter_map(|a| r.computed.get(&slot(*a)).map(|d| json!({
                            "algorithm": a.name(),
                            "label": ChecksumAlgorithms::display_name(*a),
                            "digest": shown(*a, d),
                            "pasteMatch": paste.as_ref().is_some_and(|p| p.eq_ignore_ascii_case(d)),
                        })))
                        .collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
        })
    }
}

/// Writes a checksum file over real files, through the same `ChecksumFile::render` the window's Save
/// goes through (the harness's `checksum-file`): `names` are full paths under the file's folder.
pub(crate) fn write_checksum_file(
    path: &str,
    algorithm: ChecksumAlgorithm,
    names: &[String],
) -> AppResult<()> {
    let folder = bb_paths::split_last(path).0.to_string();
    let session = ChecksumSession::hash(names, vec![algorithm]);
    match session.save(path, algorithm) {
        Ok(None) => Ok(()),
        Ok(Some(note)) => Err(AppError::Invalid(note)),
        Err(e) => Err(AppError::Invalid(format!("{e} (folder {folder})"))),
    }
}
