//! Reading and writing the checksum files the rest of the world exchanges. Spec:
//! Core/Services/Checksums/ChecksumFile.cs.
//!
//! Two shapes, and the difference is the order. The `*sum` family writes `<digest>  <name>`; SFV
//! writes `<name> <CRC>`. Which one applies is decided by the file's *extension*, not by sniffing
//! the first line — a file called `things.sfv` is an SFV even if its names happen to look like hex,
//! and guessing would make that file unreadable in a way its author could never diagnose. Sniffing
//! is the fallback for a name that says nothing, like `CHECKSUMS.txt`.
//!
//! **Problems are collected, never returned as errors.** One unparseable line must not cost the
//! other three hundred their answer — the same rule every executor in this app follows, and the
//! reason a half-corrupt checksum file still verifies the half that is intact and says so.

use std::collections::HashSet;

use bb_paths::fold_upper;

use super::{ChecksumAlgorithm, ChecksumAlgorithms};

/// One listed file and the digest claimed for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksumLine {
    pub name: String,
    pub digest: String,
    /// The GNU tools' `*` marker, meaning the file was read in binary mode. Recorded so a
    /// round-trip preserves it; this app always reads binary, so it changes nothing about the
    /// answer.
    pub binary: bool,
}

impl ChecksumLine {
    /// A binary-mode line, which is the default the C# record carried.
    pub fn new(name: &str, digest: &str) -> ChecksumLine {
        ChecksumLine {
            name: name.to_string(),
            digest: digest.to_string(),
            binary: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksumFileProblemKind {
    /// Not a checksum line at all, or a digest of a length no algorithm produces.
    Malformed,
    /// The name would resolve outside the folder being verified. See `ChecksumPath`.
    UnsafePath,
    /// The same name listed twice, with no way to say which digest is meant.
    DuplicateName,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksumFileProblem {
    /// 1-based, so it can be quoted back to the user.
    pub line_number: usize,
    pub text: String,
    pub kind: ChecksumFileProblemKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksumFileParse {
    /// What the file holds, or None when it could not be established — an unknown extension whose
    /// digests disagree about their length.
    pub algorithm: Option<ChecksumAlgorithm>,
    pub lines: Vec<ChecksumLine>,
    pub problems: Vec<ChecksumFileProblem>,
}

pub struct ChecksumFile;

impl ChecksumFile {
    /// Parses the text of a checksum file. `file_name` is the file's own name, which is what
    /// decides the line order; None or unrecognised means the shape is worked out from the
    /// content instead.
    pub fn parse(text: &str, file_name: Option<&str>) -> ChecksumFileParse {
        let declared = ChecksumAlgorithms::from_extension(file_name);
        let digest_last = declared == Some(ChecksumAlgorithm::Crc32)
            || (declared.is_none() && Self::sniffs_as_digest_last(text));

        let mut lines = Vec::new();
        let mut problems = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut lengths: HashSet<usize> = HashSet::new();

        for (index, raw) in split_lines(text).enumerate() {
            let number = index + 1;

            let line = raw.trim_end();
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                continue;
            }

            // Both comment markers are accepted in every format: SFV writes ';', the *sum family
            // and hand-written lists use '#'. Reading is generous; writing is conventional.
            if trimmed.starts_with([';', '#']) {
                continue;
            }

            let Some((name, digest, binary)) = Self::split(trimmed, digest_last) else {
                problems.push(ChecksumFileProblem {
                    line_number: number,
                    text: line.to_string(),
                    kind: ChecksumFileProblemKind::Malformed,
                });
                continue;
            };

            if ChecksumAlgorithms::recognise(digest).is_none() {
                problems.push(ChecksumFileProblem {
                    line_number: number,
                    text: line.to_string(),
                    kind: ChecksumFileProblemKind::Malformed,
                });
                continue;
            }

            if !seen.insert(fold_upper(name)) {
                problems.push(ChecksumFileProblem {
                    line_number: number,
                    text: line.to_string(),
                    kind: ChecksumFileProblemKind::DuplicateName,
                });
                continue;
            }

            lengths.insert(digest.len());
            lines.push(ChecksumLine {
                name: name.to_string(),
                digest: digest.to_string(),
                binary,
            });
        }

        // The extension is the label; the digests are the fact. When they disagree the fact wins,
        // and when the digests disagree among themselves there is no single answer to give.
        let mut algorithm = declared;
        if lengths.len() == 1 {
            let only = lengths.into_iter().next().unwrap_or(0);
            if let Some(by_length) = ChecksumAlgorithms::from_digest_length(only) {
                algorithm = Some(by_length);
            }
        } else if lengths.len() > 1 {
            algorithm = None;
        }

        ChecksumFileParse {
            algorithm,
            lines,
            problems,
        }
    }

    /// Renders a checksum file. Casing follows `ChecksumAlgorithms::is_written_lowercase`, which is
    /// the only place in the app that decides it.
    pub fn render<'a>(
        algorithm: ChecksumAlgorithm,
        lines: impl IntoIterator<Item = &'a ChecksumLine>,
    ) -> String {
        let sfv = algorithm == ChecksumAlgorithm::Crc32;
        let lower = ChecksumAlgorithms::is_written_lowercase(algorithm);

        let mut text = String::new();

        // SFV's own convention. The *sum family carries no header, and adding one would make the
        // file differ from what sha256sum produces for no gain.
        if sfv {
            text.push_str("; Generated by BertBrowser\n");
        }

        for line in lines {
            let digest = if lower {
                line.digest.to_lowercase()
            } else {
                line.digest.to_uppercase()
            };

            if sfv {
                text.push_str(&line.name);
                text.push(' ');
                text.push_str(&digest);
            } else {
                text.push_str(&digest);
                text.push_str(if line.binary { "  *" } else { "  " });
                text.push_str(&line.name);
            }
            text.push('\n');
        }

        text
    }

    /// Whether an unlabelled file looks like it puts the digest last. Only consulted when the
    /// extension says nothing. A first line whose *first* token is a well-formed digest is the
    /// ordinary form; otherwise, if its last token is, it is SFV-shaped.
    fn sniffs_as_digest_last(text: &str) -> bool {
        for raw in split_lines(text) {
            let line = raw.trim();
            if line.is_empty() || line.starts_with([';', '#']) {
                continue;
            }

            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.len() < 2 {
                continue;
            }

            if ChecksumAlgorithms::recognise(tokens[0].trim_start_matches('*')).is_some() {
                return false;
            }
            return ChecksumAlgorithms::recognise(tokens[tokens.len() - 1]).is_some();
        }

        false
    }

    /// Splits one line into (name, digest, binary), per the format's order.
    fn split(line: &str, digest_last: bool) -> Option<(&str, &str, bool)> {
        const WHITESPACE: [char; 2] = [' ', '\t'];

        if digest_last {
            // "some file name.zip A1B2C3D4" — the name may hold spaces, so cut at the last run.
            let cut = line.rfind(WHITESPACE)?;
            if cut == 0 {
                return None;
            }
            return Some((line[..cut].trim_end(), &line[cut + 1..], true));
        }

        // "<digest>  <name>" or "<digest> *<name>" — the digest never holds a space, so cut at the
        // first run and let the name keep whatever it contains.
        let end = line.find(WHITESPACE)?;
        if end == 0 || end == line.len() - 1 {
            return None;
        }

        let digest = &line[..end];
        let rest = line[end + 1..].trim_start();
        if rest.is_empty() {
            return None;
        }

        let binary = rest.starts_with('*');
        Some((if binary { &rest[1..] } else { rest }, digest, binary))
    }
}

/// `text.ReplaceLineEndings("\n").Split('\n')`: every line ending .NET recognises (CR, LF, CRLF,
/// NEL, LS, FF, PS) ends a line, and a trailing one yields a final empty line as `Split` did.
fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let current = rest?;
        let bytes = current.as_bytes();
        let mut i = 0;
        while i < current.len() {
            let c = bytes[i];
            match c {
                b'\r' => {
                    let next = if bytes.get(i + 1) == Some(&b'\n') {
                        i + 2
                    } else {
                        i + 1
                    };
                    rest = Some(&current[next..]);
                    return Some(&current[..i]);
                }
                b'\n' | 0x0C => {
                    rest = Some(&current[i + 1..]);
                    return Some(&current[..i]);
                }
                // NEL (U+0085 = C2 85), LS (U+2028 = E2 80 A8), PS (U+2029 = E2 80 A9).
                0xC2 if bytes.get(i + 1) == Some(&0x85) => {
                    rest = Some(&current[i + 2..]);
                    return Some(&current[..i]);
                }
                0xE2 if bytes.get(i + 1) == Some(&0x80)
                    && matches!(bytes.get(i + 2), Some(&0xA8) | Some(&0xA9)) =>
                {
                    rest = Some(&current[i + 3..]);
                    return Some(&current[..i]);
                }
                _ => i += 1,
            }
        }
        rest = None;
        Some(current)
    })
}
