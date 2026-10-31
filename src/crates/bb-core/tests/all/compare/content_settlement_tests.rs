// Port of tests/BertBrowser.Core.Tests/ContentSettlementTests.cs
use std::borrow::Cow;
use std::collections::HashMap;

use bb_core::FileTime;
use bb_core::compare::{
    CompareEntry, CompareResult, CompareRules, CompareTolerance, CompareVerdict, ContentSettlement,
    ContentVerdict, FolderComparer,
};
use bb_paths::file_name;

// --- the rule ---

/// Identical bytes are stronger evidence than any timestamp, so they may raise anything to a
/// match, including an Unknown, which is the point: an unknown that has been read is no longer
/// unknown.
#[test]
fn identical_bytes_settle_to_same() {
    for current in [
        CompareVerdict::Differs,
        CompareVerdict::LeftNewer,
        CompareVerdict::RightNewer,
        CompareVerdict::Unknown,
    ] {
        assert_eq!(
            CompareVerdict::Same,
            ContentSettlement::settle(current, ContentVerdict::Identical),
            "{current:?}"
        );
    }
}

/// Proving two files differ tells the comparison nothing it did not already act on, and must not
/// *lower* a verdict either: a LeftNewer is still a LeftNewer.
#[test]
fn differing_bytes_change_nothing() {
    for current in [
        CompareVerdict::Differs,
        CompareVerdict::LeftNewer,
        CompareVerdict::Same,
        CompareVerdict::Unknown,
    ] {
        assert_eq!(
            current,
            ContentSettlement::settle(current, ContentVerdict::Differs),
            "{current:?}"
        );
    }
}

/// The rule that matters. A comparison's "same" is what authorises a delete, so a read that
/// failed must never produce one, not even from an Unknown, where the temptation is greatest.
#[test]
fn an_unreadable_side_changes_nothing() {
    for current in [
        CompareVerdict::Differs,
        CompareVerdict::Unknown,
        CompareVerdict::LeftOnly,
    ] {
        assert_eq!(
            current,
            ContentSettlement::settle(current, ContentVerdict::Unreadable),
            "{current:?}"
        );
    }
}

#[test]
fn nothing_reaches_same_without_identical_bytes() {
    for verdict in CompareVerdict::ALL {
        for content in [ContentVerdict::Differs, ContentVerdict::Unreadable] {
            if verdict == CompareVerdict::Same {
                continue;
            }

            assert_ne!(
                CompareVerdict::Same,
                ContentSettlement::settle(verdict, content),
                "{verdict:?} {content:?}"
            );
        }
    }
}

// --- applying it to a whole result ---

const DIR: &str = "DIR";
const A: &str = r"DIR\A.TXT";
const B: &str = r"DIR\B.TXT";
const ONLY: &str = r"DIR\ONLY.TXT";

fn file(key: &str, size: i64, modified: FileTime) -> CompareEntry {
    CompareEntry::new(key, file_name(key), false, size, modified)
}

fn folder(key: &str) -> CompareEntry {
    CompareEntry::new(key, file_name(key), true, 0, FileTime::UNKNOWN)
}

fn when() -> FileTime {
    FileTime::from_ymd_hms(2026, 1, 1, 12, 0, 0)
}

/// A folder holding two files that differ only in size, at the same instant, which is exactly the
/// pair a timestamp comparison has to call Differs and only bytes can settle.
fn built() -> CompareResult {
    let left = [folder(DIR), file(A, 10, when()), file(B, 10, when())];
    let right = [folder(DIR), file(A, 20, when()), file(B, 20, when())];

    FolderComparer::compare(&left, &right, CompareTolerance::STRICT)
}

fn settled(pairs: &[(&str, ContentVerdict)]) -> HashMap<String, ContentVerdict> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

#[test]
fn the_fixture_starts_out_differing() {
    let result = built();

    assert_eq!(CompareVerdict::Differs, result.verdict_for(A));
    assert_eq!(CompareVerdict::Differs, result.verdict_for(B));
    assert_eq!(CompareVerdict::Differs, result.verdict_for(DIR));
}

#[test]
fn settling_one_leaf_leaves_the_other_alone() {
    let built = built();
    let settled = ContentSettlement::apply(&built, &settled(&[(A, ContentVerdict::Identical)]));

    assert_eq!(CompareVerdict::Same, settled.verdict_for(A));
    assert_eq!(CompareVerdict::Differs, settled.verdict_for(B));
}

/// The visible point of the feature: a folder goes green once its last differing child does.
/// `CompareRules::roll_up` only ever raises rank, so this only works because the fold is re-run
/// rather than patched.
#[test]
fn a_folder_goes_green_only_when_every_child_has() {
    let built = built();
    let one = ContentSettlement::apply(&built, &settled(&[(A, ContentVerdict::Identical)]));

    assert_eq!(CompareVerdict::Differs, one.verdict_for(DIR));

    let both = ContentSettlement::apply(
        &built,
        &settled(&[
            (A, ContentVerdict::Identical),
            (B, ContentVerdict::Identical),
        ]),
    );

    assert_eq!(CompareVerdict::Same, both.verdict_for(DIR));
}

#[test]
fn the_counts_are_recomputed() {
    let before = built();
    let after = ContentSettlement::apply(
        &before,
        &settled(&[
            (A, ContentVerdict::Identical),
            (B, ContentVerdict::Identical),
        ]),
    );

    assert!(after.same_count > before.same_count);
    assert_eq!(0, after.difference_count);
    assert!(!after.any_difference());
}

#[test]
fn settling_nothing_returns_the_same_result() {
    let before = built();

    let settled = ContentSettlement::apply(&before, &HashMap::new());
    assert!(matches!(settled, Cow::Borrowed(_)));
    assert!(std::ptr::eq(&before, settled.as_ref()));
}

/// A key nobody compared is ignored rather than invented into the result.
#[test]
fn an_unknown_key_is_ignored() {
    let built = built();
    let settled = ContentSettlement::apply(
        &built,
        &settled(&[(r"DIR\NOPE.TXT", ContentVerdict::Identical)]),
    );

    assert!(!settled.by_relative_key.contains_key(r"DIR\NOPE.TXT"));
    assert_eq!(CompareVerdict::Differs, settled.verdict_for(DIR));
}

/// A settlement must not disturb rows it was not about: a file only one side has stays that way,
/// whatever happened to its neighbours.
#[test]
fn one_sided_rows_survive_a_settlement() {
    let left = [folder(DIR), file(A, 10, when()), file(ONLY, 5, when())];
    let right = [folder(DIR), file(A, 20, when())];

    let result = FolderComparer::compare(&left, &right, CompareTolerance::STRICT);
    let settled = ContentSettlement::apply(&result, &settled(&[(A, ContentVerdict::Identical)]));

    assert_eq!(CompareVerdict::Same, settled.verdict_for(A));
    assert_eq!(CompareVerdict::LeftOnly, settled.verdict_for(ONLY));

    // The folder still holds something unsettled, so it must not have gone green.
    assert_ne!(CompareVerdict::Same, settled.verdict_for(DIR));
}

/// The sync reads its verdicts off the result, which is the whole reason settlement produces a
/// new one rather than an overlay beside the rows. If this ever stops holding, a row saying
/// "Same" and a sync copying it anyway is the bug that follows.
#[test]
fn the_sync_sees_a_settled_verdict() {
    let built = built();
    let settled = ContentSettlement::apply(&built, &settled(&[(A, ContentVerdict::Identical)]));

    assert!(!CompareRules::would_copy(settled.verdict_for(A)));
    assert!(CompareRules::would_copy(settled.verdict_for(B)));
}
