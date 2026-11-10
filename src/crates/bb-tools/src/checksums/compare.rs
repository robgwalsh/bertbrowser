//! Comparing a computed digest against a value a user pasted in, e.g. from a download page's
//! published checksum. Spec: Core/Services/Checksums/ChecksumCompare.cs.

use super::ChecksumAlgorithms;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksumMatchState {
    Unknown,
    Match,
    Mismatch,
}

pub struct ChecksumCompare;

impl ChecksumCompare {
    /// The digest inside whatever the user pasted, or None if there is not one that looks like a
    /// digest.
    ///
    /// People paste a whole line, not a bare hash, and the two conventions put the hash at
    /// opposite ends: `sha256sum` writes `<hash>  file.zip` and an SFV file writes
    /// `file.zip A1B2C3D4`. So both ends are considered, and what settles it is the shape — hex,
    /// of a length exactly one algorithm produces. A file name is almost never both. Preferring
    /// the first token when both ends qualify keeps the common case exact: a file genuinely named
    /// after its own hash is a curiosity, and it is the `sha256sum` form that is pasted a thousand
    /// times more often.
    pub fn extract_digest(pasted: &str) -> Option<&str> {
        let mut tokens = pasted.split_whitespace();
        let first = tokens.next()?;
        if ChecksumAlgorithms::recognise(first).is_some() {
            return Some(first);
        }

        let last = tokens.last().unwrap_or(first);
        if ChecksumAlgorithms::recognise(last).is_some() {
            Some(last)
        } else {
            None
        }
    }

    /// Whether a digest matches what the user pasted.
    ///
    /// Falls back to the first token when nothing in the paste looks like a known digest, so a
    /// truncated or mistyped hash still compares — and mismatches — rather than silently reading
    /// as "nothing to compare". Case is ignored: hex digests are conventionally lower case and are
    /// very often published upper.
    pub fn evaluate(digest: Option<&str>, expected_input: &str) -> ChecksumMatchState {
        let Some(digest) = digest else {
            return ChecksumMatchState::Unknown;
        };

        let expected = expected_input.trim();
        if expected.is_empty() {
            return ChecksumMatchState::Unknown;
        }

        let candidate = Self::extract_digest(expected)
            .or_else(|| expected.split_whitespace().next())
            .unwrap_or("");

        if eq_ignore_case(candidate, digest) {
            ChecksumMatchState::Match
        } else {
            ChecksumMatchState::Mismatch
        }
    }
}

/// `StringComparison.OrdinalIgnoreCase`: per-character simple upper-casing on both sides.
pub(crate) fn eq_ignore_case(a: &str, b: &str) -> bool {
    bb_paths::fold_upper(a) == bb_paths::fold_upper(b)
}
