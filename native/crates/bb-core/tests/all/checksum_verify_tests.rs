// Port of tests/BertBrowser.Core.Tests/ChecksumVerifyTests.cs
use std::collections::HashMap;

use bb_core::checksums::{ChecksumLine, ChecksumVerify, ChecksumVerifyRow, ChecksumVerifyState};

const A: &str = "AAAAAAAA";
const B: &str = "BBBBBBBB";

fn computed(pairs: &[(&str, Option<&str>)]) -> HashMap<String, Option<String>> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.map(str::to_string)))
        .collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn reconcile(
    listed: &[ChecksumLine],
    computed: Option<HashMap<String, Option<String>>>,
    refused: Option<&[&str]>,
    present: Option<&[&str]>,
) -> Vec<ChecksumVerifyRow> {
    ChecksumVerify::reconcile(
        listed,
        &computed.unwrap_or_default(),
        &strings(refused.unwrap_or(&[])),
        &strings(present.unwrap_or(&[])),
    )
}

fn single(rows: &[ChecksumVerifyRow]) -> &ChecksumVerifyRow {
    assert_eq!(1, rows.len(), "expected exactly one row");
    &rows[0]
}

fn state_of(rows: &[ChecksumVerifyRow], name: &str) -> ChecksumVerifyState {
    let matching: Vec<&ChecksumVerifyRow> = rows.iter().filter(|r| r.name == name).collect();
    assert_eq!(1, matching.len(), "expected exactly one row named {name}");
    matching[0].state
}

#[test]
fn a_matching_digest_is_ok() {
    let rows = reconcile(
        &[ChecksumLine::new("a.bin", A)],
        Some(computed(&[("a.bin", Some(A))])),
        None,
        None,
    );

    assert_eq!(ChecksumVerifyState::Ok, single(&rows).state);
}

#[test]
fn a_different_digest_is_a_mismatch_and_both_values_are_reported() {
    let rows = reconcile(
        &[ChecksumLine::new("a.bin", A)],
        Some(computed(&[("a.bin", Some(B))])),
        None,
        None,
    );
    let row = single(&rows);

    assert_eq!(ChecksumVerifyState::Mismatch, row.state);
    assert_eq!(Some(A.to_string()), row.expected);
    assert_eq!(Some(B.to_string()), row.actual);
}

#[test]
fn a_line_with_no_file_is_missing() {
    let rows = reconcile(&[ChecksumLine::new("a.bin", A)], None, None, None);
    assert_eq!(ChecksumVerifyState::Missing, single(&rows).state);
}

/// A file that is there and could not be read is not a file that is wrong. Collapsing the two
/// would report a locked file as corruption.
#[test]
fn a_present_but_unreadable_file_is_unreadable_not_a_mismatch() {
    let rows = reconcile(
        &[ChecksumLine::new("a.bin", A)],
        Some(computed(&[("a.bin", None)])),
        None,
        None,
    );

    assert_eq!(ChecksumVerifyState::Unreadable, single(&rows).state);
}

#[test]
fn an_escaping_name_is_refused_and_is_never_read() {
    let rows = reconcile(
        &[ChecksumLine::new(r"..\..\evil", A)],
        None,
        Some(&[r"..\..\evil"]),
        None,
    );

    assert_eq!(ChecksumVerifyState::Refused, single(&rows).state);
}

#[test]
fn a_file_nothing_listed_is_not_listed() {
    let rows = reconcile(
        &[ChecksumLine::new("a.bin", A)],
        Some(computed(&[("a.bin", Some(A))])),
        None,
        Some(&["a.bin", "extra.bin"]),
    );

    assert_eq!(ChecksumVerifyState::Ok, state_of(&rows, "a.bin"));
    assert_eq!(ChecksumVerifyState::NotListed, state_of(&rows, "extra.bin"));
}

/// A folder holding something the checksum file never mentioned is normal — the file covers what
/// it covers. Calling that a failure would make every download folder fail.
#[test]
fn not_listed_is_not_a_failure() {
    assert!(!ChecksumVerify::is_failure(ChecksumVerifyState::NotListed));
    assert!(!ChecksumVerify::is_failure(ChecksumVerifyState::Ok));

    assert!(ChecksumVerify::is_failure(ChecksumVerifyState::Mismatch));
    assert!(ChecksumVerify::is_failure(ChecksumVerifyState::Missing));
    assert!(ChecksumVerify::is_failure(ChecksumVerifyState::Unreadable));
    assert!(ChecksumVerify::is_failure(ChecksumVerifyState::Refused));
}

#[test]
fn names_match_case_insensitively_because_windows() {
    let rows = reconcile(
        &[ChecksumLine::new("Setup.exe", A)],
        Some(computed(&[("Setup.exe", Some(A))])),
        None,
        Some(&["SETUP.EXE"]),
    );

    assert_eq!(ChecksumVerifyState::Ok, single(&rows).state);
}

#[test]
fn digests_match_case_insensitively_because_publishers_disagree() {
    let rows = reconcile(
        &[ChecksumLine::new("a.bin", "cbf43926")],
        Some(computed(&[("a.bin", Some("CBF43926"))])),
        None,
        None,
    );

    assert_eq!(ChecksumVerifyState::Ok, single(&rows).state);
}

#[test]
fn every_listed_line_gets_exactly_one_row() {
    let rows = reconcile(
        &[
            ChecksumLine::new("a.bin", A),
            ChecksumLine::new("b.bin", A),
            ChecksumLine::new("c.bin", A),
        ],
        Some(computed(&[("a.bin", Some(A)), ("b.bin", Some(B))])),
        None,
        None,
    );

    assert_eq!(3, rows.len());
    assert_eq!(ChecksumVerifyState::Ok, state_of(&rows, "a.bin"));
    assert_eq!(ChecksumVerifyState::Mismatch, state_of(&rows, "b.bin"));
    assert_eq!(ChecksumVerifyState::Missing, state_of(&rows, "c.bin"));
}

#[test]
fn the_summary_leads_with_what_went_wrong() {
    let rows = reconcile(
        &[ChecksumLine::new("a.bin", A), ChecksumLine::new("b.bin", A)],
        Some(computed(&[("a.bin", Some(A)), ("b.bin", Some(B))])),
        None,
        None,
    );

    assert!(ChecksumVerify::summarise(&rows).starts_with("1 mismatched"));
}

#[test]
fn an_empty_report_says_so() {
    assert_eq!("Nothing to verify.", ChecksumVerify::summarise(&[]));
}
