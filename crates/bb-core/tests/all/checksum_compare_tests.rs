// Port of tests/BertBrowser.Core.Tests/ChecksumCompareTests.cs
use bb_core::checksums::{ChecksumCompare, ChecksumMatchState};

const DIGEST: &str = "7B0E07BCF7285BAE7B39CFF9BEF5E05EFE2BC1E5EF221130E0A8F338EAA4E849";

#[test]
fn no_digest_yet_is_unknown() {
    assert_eq!(
        ChecksumMatchState::Unknown,
        ChecksumCompare::evaluate(None, "anything")
    );
}

#[test]
fn blank_input_is_unknown() {
    assert_eq!(
        ChecksumMatchState::Unknown,
        ChecksumCompare::evaluate(Some(DIGEST), "   ")
    );
}

#[test]
fn an_exact_case_insensitive_match_matches() {
    assert_eq!(
        ChecksumMatchState::Match,
        ChecksumCompare::evaluate(Some(DIGEST), &DIGEST.to_lowercase())
    );
}

#[test]
fn a_different_value_mismatches() {
    assert_eq!(
        ChecksumMatchState::Mismatch,
        ChecksumCompare::evaluate(Some(DIGEST), "not the right hash")
    );
}

#[test]
fn only_the_first_token_is_compared() {
    // sha256sum-style listings pair the hash with a file name.
    assert_eq!(
        ChecksumMatchState::Match,
        ChecksumCompare::evaluate(Some(DIGEST), &format!("{DIGEST}  download.zip"))
    );
}

#[test]
fn surrounding_whitespace_is_ignored() {
    assert_eq!(
        ChecksumMatchState::Match,
        ChecksumCompare::evaluate(Some(DIGEST), &format!("  {DIGEST}\n"))
    );
}

/// An SFV line puts the name first and the checksum last — the opposite of every other format.
/// Taking the first token unconditionally read the file name as the digest and reported a
/// mismatch against a checksum that was in fact correct.
#[test]
fn an_sfv_style_line_is_recognised_from_the_other_end() {
    assert_eq!(
        ChecksumMatchState::Match,
        ChecksumCompare::evaluate(Some("CBF43926"), "download.zip CBF43926")
    );
}

/// What decides which end holds the digest is its shape, so a name that happens to be hex but is
/// not a digest length cannot be mistaken for one.
#[test]
fn a_token_that_is_not_a_digest_length_is_not_taken_as_one() {
    assert_eq!(None, ChecksumCompare::extract_digest("deadbeef0 notes.txt"));
}

#[test]
fn the_first_token_wins_when_both_ends_look_like_digests() {
    assert_eq!(
        Some(DIGEST),
        ChecksumCompare::extract_digest(&format!("{DIGEST} CBF43926"))
    );
}
