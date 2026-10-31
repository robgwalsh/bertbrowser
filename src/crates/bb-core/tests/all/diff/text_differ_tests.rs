// Port of tests/BertBrowser.Core.Tests/TextDifferTests.cs
use bb_core::diff::{DiffOp, DiffOptions, TextDiff, TextDiffer};

fn lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').collect()
    }
}

fn diff(left: &str, right: &str) -> TextDiff {
    diff_with(left, right, &DiffOptions::default())
}

fn diff_with(left: &str, right: &str, options: &DiffOptions) -> TextDiff {
    TextDiffer::compare(&lines(left), &lines(right), options)
}

/// A compact rendering of the rows, so a test reads as what the view would show.
fn render(diff: &TextDiff) -> String {
    diff.rows
        .iter()
        .map(|r| match r.op {
            DiffOp::Equal => format!("={}/{}", r.left_line, r.right_line),
            DiffOp::Replace => format!("~{}/{}", r.left_line, r.right_line),
            DiffOp::Delete => format!("-{}", r.left_line),
            DiffOp::Insert => format!("+{}", r.right_line),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// --- the shape of an answer ---

#[test]
fn identical_texts_are_all_equal_and_have_no_hunks() {
    let diff = diff("a\nb\nc", "a\nb\nc");

    assert_eq!("=1/1 =2/2 =3/3", render(&diff));
    assert!(diff.hunks.is_empty());
    assert!(diff.identical());
    assert!(!diff.budget_exceeded);
}

#[test]
fn two_empty_texts_are_identical() {
    let diff = diff("", "");

    assert!(diff.rows.is_empty());
    assert!(diff.identical());
}

#[test]
fn a_one_line_change_is_one_replace_and_one_hunk() {
    let diff = diff("a\nb\nc", "a\nB\nc");

    assert_eq!("=1/1 ~2/2 =3/3", render(&diff));
    assert_eq!(1, diff.changed_line_count);

    assert_eq!(1, diff.hunks.len());
    let hunk = diff.hunks[0];
    assert_eq!(1, hunk.first_row);
    assert_eq!(1, hunk.row_count);
}

#[test]
fn a_pure_insertion_leaves_the_left_blank() {
    let diff = diff("a\nc", "a\nb\nc");

    assert_eq!("=1/1 +2 =2/3", render(&diff));
}

#[test]
fn a_pure_deletion_leaves_the_right_blank() {
    let diff = diff("a\nb\nc", "a\nc");

    assert_eq!("=1/1 -2 =3/2", render(&diff));
}

#[test]
fn an_insertion_at_the_top_is_a_hunk_at_row_zero() {
    let diff = diff("a", "z\na");

    assert_eq!("+1 =1/2", render(&diff));
    assert_eq!(1, diff.hunks.len());
    assert_eq!(0, diff.hunks[0].first_row);
}

#[test]
fn an_insertion_at_the_end_is_a_hunk_at_the_end() {
    let diff = diff("a", "a\nz");

    assert_eq!("=1/1 +2", render(&diff));
}

#[test]
fn everything_deleted_is_all_deletions() {
    let diff = diff("a\nb", "");

    assert_eq!("-1 -2", render(&diff));
}

#[test]
fn everything_inserted_is_all_insertions() {
    let diff = diff("", "a\nb");

    assert_eq!("+1 +2", render(&diff));
}

// --- the pairing, which is the piece most likely to be wrong ---

/// Without pairing this renders as three blank-right rows then three blank-left rows, and the
/// reader has to work out which line became which.
#[test]
fn a_run_of_deletions_beside_a_run_of_insertions_is_paired_into_replaces() {
    let diff = diff("a\n1\n2\n3\nz", "a\nX\nY\nZ\nz");

    assert_eq!("=1/1 ~2/2 ~3/3 ~4/4 =5/5", render(&diff));
}

/// Four lines becoming two is two replacements and two deletions, not four of anything. The
/// remainder of the longer run has to stay honest about which side it is on.
#[test]
fn an_unequal_run_pads_the_shorter_side() {
    let diff = diff("a\n1\n2\n3\n4\nz", "a\nX\nY\nz");

    assert_eq!("=1/1 ~2/2 ~3/3 -4 -5 =6/4", render(&diff));
}

#[test]
fn more_insertions_than_deletions_pads_the_left() {
    let diff = diff("a\n1\nz", "a\nX\nY\nZ\nz");

    assert_eq!("=1/1 ~2/2 +3 +4 =3/5", render(&diff));
}

// --- hunk grouping ---

#[test]
fn changes_further_apart_than_twice_the_context_are_separate_hunks() {
    let same: Vec<&str> = vec!["same"; 20];
    let left = format!("X\n{}\nY", same.join("\n"));
    let right = format!("1\n{}\n2", same.join("\n"));

    assert_eq!(2, diff(&left, &right).hunks.len());
}

#[test]
fn changes_closer_than_twice_the_context_are_one_hunk() {
    let left = "X\nsame\nsame\nY";
    let right = "1\nsame\nsame\n2";

    assert_eq!(1, diff(left, right).hunks.len());
}

#[test]
fn every_hunk_points_at_a_real_changed_row() {
    let diff = diff(
        "a\nX\nb\nc\nd\ne\nf\ng\nh\ni\nj\nY\nk",
        "a\n1\nb\nc\nd\ne\nf\ng\nh\ni\nj\n2\nk",
    );

    assert!(!diff.hunks.is_empty());
    for hunk in &diff.hunks {
        assert_ne!(DiffOp::Equal, diff.rows[hunk.first_row].op);
    }
}

// --- the options ---

/// On by default, because a difference of trailing spaces is the one difference a person cannot
/// see and will not believe.
#[test]
fn trailing_whitespace_is_ignored_by_default() {
    assert!(diff("a  \nb", "a\nb").identical());
}

#[test]
fn trailing_whitespace_can_be_made_to_matter() {
    let options = DiffOptions {
        ignore_trailing_whitespace: false,
        ..DiffOptions::default()
    };
    assert!(!diff_with("a  \nb", "a\nb", &options).identical());
}

#[test]
fn leading_whitespace_still_matters_by_default() {
    assert!(!diff("    a", "a").identical());
}

#[test]
fn ignore_all_whitespace_reaches_indentation() {
    let options = DiffOptions {
        ignore_all_whitespace: true,
        ..DiffOptions::default()
    };
    assert!(diff_with("    a", "a", &options).identical());
}

#[test]
fn ignore_case_does_what_it_says() {
    assert!(!diff("Hello", "hello").identical());
    let options = DiffOptions {
        ignore_case: true,
        ..DiffOptions::default()
    };
    assert!(diff_with("Hello", "hello", &options).identical());
}

// --- the budget ---

/// Two files sharing nothing are further apart than the budget allows. The answer is a
/// positional pairing and a flag saying so: never a failure, and never an empty result, which a
/// view would have nothing to show for.
#[test]
fn two_wholly_different_files_fall_back_to_positional_alignment() {
    let left: Vec<String> = (0..3000).map(|i| format!("left {i}")).collect();
    let right: Vec<String> = (0..3000).map(|i| format!("right {i}")).collect();

    let diff = diff(&left.join("\n"), &right.join("\n"));

    assert!(diff.budget_exceeded);
    assert_eq!(3000, diff.rows.len());
    assert!(diff.rows.iter().all(|r| r.op == DiffOp::Replace));
    assert!(!diff.hunks.is_empty());
}

#[test]
fn a_file_longer_than_the_line_cap_falls_back_rather_than_running() {
    let many = vec!["x"; TextDiffer::MAX_LINES + 1];

    assert!(TextDiffer::compare(&many, &["x"], &DiffOptions::default()).budget_exceeded);
}

/// The reduction that makes this practical: one changed line in a large file is a tiny edit
/// script, however large the file, because the matching ends are trimmed before Myers starts.
#[test]
fn a_large_file_with_one_changed_line_still_aligns() {
    let left: Vec<String> = (0..50_000).map(|i| format!("line {i}")).collect();
    let mut right = left.clone();
    right[25_000] = "changed".to_string();

    let left: Vec<&str> = left.iter().map(String::as_str).collect();
    let right: Vec<&str> = right.iter().map(String::as_str).collect();
    let diff = TextDiffer::compare(&left, &right, &DiffOptions::default());

    assert!(!diff.budget_exceeded);
    assert_eq!(1, diff.changed_line_count);
    assert_eq!(1, diff.hunks.len());
}

// --- what a row means ---

#[test]
fn every_row_numbers_only_the_sides_it_has() {
    let diff = diff("a\nb\nc", "a\nx");

    for row in &diff.rows {
        match row.op {
            DiffOp::Equal | DiffOp::Replace => {
                assert!(row.left_line > 0 && row.right_line > 0, "{row:?}");
            }
            DiffOp::Delete => assert!(row.left_line > 0 && row.right_line == 0, "{row:?}"),
            DiffOp::Insert => assert!(row.left_line == 0 && row.right_line > 0, "{row:?}"),
        }
    }
}

/// Line numbers must run 1, 2, 3 down each side with no gaps and no repeats: they are the gutter
/// a reader checks the diff against.
#[test]
fn line_numbers_are_consecutive_down_each_side() {
    let diff = diff("a\n1\n2\nb\nc", "a\nX\nb\nY\nc");

    let left: Vec<usize> = diff
        .rows
        .iter()
        .filter(|r| r.left_line > 0)
        .map(|r| r.left_line)
        .collect();
    let right: Vec<usize> = diff
        .rows
        .iter()
        .filter(|r| r.right_line > 0)
        .map(|r| r.right_line)
        .collect();

    assert_eq!((1..=5).collect::<Vec<_>>(), left);
    assert_eq!((1..=5).collect::<Vec<_>>(), right);
}
