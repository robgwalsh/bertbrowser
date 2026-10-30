// Port of tests/BertBrowser.Core.Tests/FlatViewRulesTests.cs: when a flat view asks before it lists,
// and what it says. The wording is under test as much as the threshold.
use bb_core::FileTime;
use bb_core::data::DirSizeResult;
use bb_core::rules::FlatViewRules;

const CAP: i64 = 50_000;

fn size(files: i32, dirs: i32, incomplete: bool) -> DirSizeResult {
    DirSizeResult {
        path_key: r"D:\MEDIA".to_string(),
        size_bytes: 0,
        file_count: files,
        dir_count: dirs,
        incomplete,
        computed_utc: FileTime::now(),
    }
}

#[test]
fn no_cached_size_means_go_ahead() {
    // A missing dir_size_cache row means *unknown*, never zero.
    assert!(!FlatViewRules::decide(r"D:\Media", None, CAP).confirm);
}

#[test]
fn at_or_under_the_cap_it_just_runs() {
    for files in [0, 1, (CAP - 1) as i32, CAP as i32] {
        assert!(
            !FlatViewRules::decide(r"D:\Media", Some(&size(files, 0, false)), CAP).confirm,
            "{files}"
        );
    }
}

#[test]
fn over_the_cap_it_asks_first() {
    let decision = FlatViewRules::decide(r"D:\Media", Some(&size(CAP as i32 + 1, 0, false)), CAP);
    assert!(decision.confirm);
    assert!(
        decision.message.contains("50,001 files"),
        "{}",
        decision.message
    );
    assert!(decision.message.contains(r"D:\Media"));
    assert_eq!("Show 50,000", decision.confirm_label);
}

#[test]
fn folders_do_not_count_towards_the_ceiling() {
    assert!(!FlatViewRules::decide(r"D:\Media", Some(&size(40_000, 20_000, false)), CAP).confirm);
}

#[test]
fn an_incomplete_row_is_a_floor_and_says_so() {
    let decision = FlatViewRules::decide(r"D:\Media", Some(&size(CAP as i32 + 1, 0, true)), CAP);
    assert!(
        decision.message.contains("at least 50,001 files"),
        "{}",
        decision.message
    );
}
