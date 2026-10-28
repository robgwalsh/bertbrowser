//! Reconciling what a checksum file claims against what the folder actually holds. Spec:
//! Core/Services/Checksums/ChecksumVerify.cs.
//!
//! Pure: the caller does the reading and hands the results in, so every rule here is testable
//! without a disk. The six states are deliberately six rather than a boolean — "could not be read"
//! is not "wrong", and "there but unlisted" is not a failure at all, it is how you find out a
//! folder gained a file.

use std::collections::{HashMap, HashSet};

use bb_paths::fold_upper;

use super::ChecksumLine;
use super::compare::eq_ignore_case;
use crate::format::thousands;

/// How one file came out of a verify run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksumVerifyState {
    /// Listed, present, and the digest agrees.
    Ok,
    /// Listed and present, and the bytes are not what the file says they should be.
    Mismatch,
    /// Listed, and not there.
    Missing,
    /// Listed, present, and could not be read — which is not the same as wrong.
    Unreadable,
    /// The listed name would resolve outside the folder. See `ChecksumPath`.
    Refused,
    /// Present in the folder and absent from the checksum file.
    NotListed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksumVerifyRow {
    pub name: String,
    pub state: ChecksumVerifyState,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

pub struct ChecksumVerify;

impl ChecksumVerify {
    /// Builds the report. `listed` is the checksum file's lines, already parsed; `computed` holds a
    /// digest per listed name, or None against a name that is present but could not be read — a
    /// name absent from it was not found on disk at all; `refused` is the listed names
    /// `ChecksumPath` would not resolve; `present` is every file actually in the folder, so the
    /// ones nothing listed can be reported (pass an empty slice to leave `NotListed` out of the
    /// report entirely).
    pub fn reconcile(
        listed: &[ChecksumLine],
        computed: &HashMap<String, Option<String>>,
        refused: &[String],
        present: &[String],
    ) -> Vec<ChecksumVerifyRow> {
        let mut rows = Vec::with_capacity(listed.len());
        let mut accounted: HashSet<String> = HashSet::new();
        let refused_set: HashSet<String> = refused.iter().map(|r| fold_upper(r)).collect();

        for line in listed {
            accounted.insert(fold_upper(&line.name));

            let row = |state, actual: Option<String>| ChecksumVerifyRow {
                name: line.name.clone(),
                state,
                expected: Some(line.digest.clone()),
                actual,
            };

            if refused_set.contains(&fold_upper(&line.name)) {
                rows.push(row(ChecksumVerifyState::Refused, None));
                continue;
            }

            let Some(actual) = computed.get(&line.name) else {
                rows.push(row(ChecksumVerifyState::Missing, None));
                continue;
            };

            let Some(actual) = actual else {
                rows.push(row(ChecksumVerifyState::Unreadable, None));
                continue;
            };

            let state = if eq_ignore_case(actual, &line.digest) {
                ChecksumVerifyState::Ok
            } else {
                ChecksumVerifyState::Mismatch
            };

            rows.push(row(state, Some(actual.clone())));
        }

        for name in present {
            if !accounted.contains(&fold_upper(name)) {
                rows.push(ChecksumVerifyRow {
                    name: name.clone(),
                    state: ChecksumVerifyState::NotListed,
                    expected: None,
                    actual: None,
                });
            }
        }

        rows
    }

    /// Whether a state means the verify failed. `NotListed` is not a failure: a folder holding a
    /// file the checksum file never mentioned is normal — the checksum file covers what it covers.
    /// Saying otherwise would make every download folder fail its own verification.
    pub fn is_failure(state: ChecksumVerifyState) -> bool {
        matches!(
            state,
            ChecksumVerifyState::Mismatch
                | ChecksumVerifyState::Missing
                | ChecksumVerifyState::Unreadable
                | ChecksumVerifyState::Refused
        )
    }

    /// One line summarising a report, in the order that matters most first.
    pub fn summarise(rows: &[ChecksumVerifyRow]) -> String {
        if rows.is_empty() {
            return "Nothing to verify.".to_string();
        }

        let mut parts = Vec::new();
        let mut add = |state: ChecksumVerifyState, word: &str| {
            let n = rows.iter().filter(|r| r.state == state).count();
            if n > 0 {
                parts.push(format!("{} {word}", thousands(n as i64)));
            }
        };

        add(ChecksumVerifyState::Mismatch, "mismatched");
        add(ChecksumVerifyState::Missing, "missing");
        add(ChecksumVerifyState::Unreadable, "unreadable");
        add(ChecksumVerifyState::Refused, "refused");
        add(ChecksumVerifyState::Ok, "OK");
        add(ChecksumVerifyState::NotListed, "not listed");

        parts.join(" · ")
    }
}
