// Port of tests/BertBrowser.Core.Tests/ContentSnippetTests.cs: which line the match was on, and how
// much of it to show.
use bb_core::search::{ContentMatch, ContentSnippet, ContentText};

fn text(s: &str) -> ContentText {
    ContentText::new(s, false)
}

fn snippet(s: &str, needles: &[&str]) -> Option<ContentMatch> {
    let needles: Vec<String> = needles.iter().map(|n| n.to_string()).collect();
    ContentSnippet::for_content(&text(s), &needles)
}

fn highlighted(m: &ContentMatch) -> String {
    m.line
        .chars()
        .skip(m.match_start)
        .take(m.match_length)
        .collect()
}

#[test]
fn the_first_line_is_line_one() {
    let m = snippet("hello TODO world", &["TODO"]).unwrap();
    assert_eq!(1, m.line_number);
    assert_eq!("hello TODO world", m.line);
    assert_eq!(6, m.match_start);
    assert_eq!(4, m.match_length);
}

#[test]
fn lines_are_counted_from_one_not_zero() {
    let m = snippet("a\nb\nTODO here\nd", &["TODO"]).unwrap();
    assert_eq!(3, m.line_number);
    assert_eq!("TODO here", m.line);
    assert_eq!(0, m.match_start);
}

#[test]
fn a_carriage_return_does_not_shift_the_numbering() {
    let unix = snippet("a\nb\nTODO\n", &["TODO"]).unwrap();
    let windows = snippet("a\nb\nTODO\n", &["TODO"]).unwrap();
    assert_eq!(unix.line_number, windows.line_number);
    assert_eq!(3, unix.line_number);
}

#[test]
fn a_lone_carriage_return_is_trimmed_off_the_line() {
    let m = snippet("x\nTODO here\r\ny", &["TODO"]).unwrap();
    assert_eq!("TODO here", m.line);
}

#[test]
fn a_match_on_the_last_line_with_no_trailing_newline_still_works() {
    let m = snippet("a\nb\nlast TODO", &["TODO"]).unwrap();
    assert_eq!(3, m.line_number);
    assert_eq!("last TODO", m.line);
    assert_eq!(5, m.match_start);
}

#[test]
fn no_match_is_null() {
    assert!(snippet("nothing here", &["TODO"]).is_none());
}

#[test]
fn the_earliest_needle_wins_rather_than_the_first_listed() {
    let m = snippet("line one alpha\nline two beta", &["beta", "alpha"]).unwrap();
    assert_eq!(1, m.line_number);
    assert_eq!("alpha", highlighted(&m));
}

#[test]
fn matching_ignores_case_and_the_offset_still_lands_on_the_text() {
    let m = snippet("a Todo item", &["TODO"]).unwrap();
    assert_eq!("Todo", highlighted(&m));
}

// --- clipping ---

#[test]
fn a_short_line_is_not_clipped() {
    let m = snippet(&format!("{}TODO", "x".repeat(100)), &["TODO"]).unwrap();
    assert!(!m.line.contains('\u{2026}'));
    assert_eq!(104, m.line.chars().count());
}

#[test]
fn a_very_long_line_is_clipped_around_the_match() {
    let line = format!("{}TODO{}", "x".repeat(5_000), "y".repeat(5_000));
    let m = snippet(&line, &["TODO"]).unwrap();
    assert!(
        m.line.chars().count() <= ContentSnippet::MAX_LINE_CHARS + 2,
        "was {}",
        m.line.chars().count()
    );
    assert_eq!("TODO", highlighted(&m));
    assert!(m.line.starts_with('\u{2026}'));
    assert!(m.line.ends_with('\u{2026}'));
}

#[test]
fn a_match_near_the_end_of_a_long_line_is_still_inside_the_clip() {
    let line = format!("{}TODO", "x".repeat(5_000));
    let m = snippet(&line, &["TODO"]).unwrap();
    assert!(m.match_start + m.match_length <= m.line.chars().count());
    assert_eq!("TODO", highlighted(&m));
}

#[test]
fn a_match_at_the_start_of_a_long_line_keeps_no_leading_ellipsis() {
    let line = format!("TODO{}", "y".repeat(5_000));
    let m = snippet(&line, &["TODO"]).unwrap();
    assert!(!m.line.starts_with('\u{2026}'));
    assert_eq!(0, m.match_start);
}
