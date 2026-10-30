// Port of tests/BertBrowser.Core.Tests/SelectionPatternTests.cs.
use bb_core::FileTime;
use bb_core::attributes::{ARCHIVE, DIRECTORY, HIDDEN, READONLY};
use bb_core::commands::{SelectionPattern, SelectionRow};

fn day() -> FileTime {
    FileTime::from_ymd_hms(2026, 3, 14, 9, 0, 0)
}

fn row(name: &str, is_dir: bool, size: i64, attributes: u32) -> SelectionRow {
    SelectionRow {
        name: name.to_string(),
        full_path: format!(r"C:\x\{name}"),
        is_directory: is_dir,
        size_bytes: size,
        modified_utc: day(),
        created_utc: day(),
        attributes,
    }
}

fn rows() -> Vec<SelectionRow> {
    vec![
        row("Photos", true, 0, DIRECTORY),
        row("IMG_0001.jpg", false, 4_000_000, ARCHIVE),
        row("IMG_0002.JPG", false, 500, ARCHIVE),
        row("notes.txt", false, 120, ARCHIVE | READONLY),
        row(".secret", false, 10, HIDDEN),
    ]
}

fn picked(pattern: &str) -> String {
    let rows = rows();
    let m = SelectionPattern::matches(pattern, &rows);
    assert!(m.problem.is_none(), "{pattern}: {:?}", m.problem);
    m.indexes
        .iter()
        .map(|&i| rows[i].name.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_wildcard_picks_by_name_whatever_the_case() {
    assert_eq!("IMG_0001.jpg IMG_0002.JPG", picked("*.jpg"));
    assert_eq!("IMG_0001.jpg IMG_0002.JPG", picked("img_"));
}

#[test]
fn the_search_boxs_filters_mean_the_same_thing_here() {
    assert_eq!("IMG_0001.jpg", picked("ext:jpg size:>1mb"));
    assert_eq!("notes.txt", picked("is:readonly"));
    assert_eq!("IMG_0002.JPG", picked(r"re:^IMG_\d+2"));
    assert_eq!("Photos notes.txt", picked("photos OR notes"));
    assert_eq!("Photos notes.txt .secret", picked("!img"));
}

#[test]
fn a_hidden_row_on_screen_is_as_selectable_as_its_neighbours() {
    assert_eq!(".secret", picked("secret"));
    assert_eq!(".secret", picked("is:hidden"));
}

#[test]
fn nothing_typed_picks_nothing_and_is_not_a_problem() {
    let m = SelectionPattern::matches("   ", &rows());
    assert!(m.indexes.is_empty());
    assert!(m.problem.is_none());
}

#[test]
fn a_pattern_that_cannot_be_read_says_why() {
    let m = SelectionPattern::matches("re:(", &rows());
    assert!(m.indexes.is_empty());
    assert!(m.problem.is_some());
}

#[test]
fn terms_that_would_read_files_are_refused_rather_than_answered_wrongly() {
    for pattern in ["content:todo", "jpg in:archives"] {
        let m = SelectionPattern::matches(pattern, &rows());
        assert!(m.indexes.is_empty(), "{pattern}");
        assert!(m.problem.is_some(), "{pattern}");
    }
}
