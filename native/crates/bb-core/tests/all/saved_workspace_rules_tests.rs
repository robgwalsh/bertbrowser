// Port of tests/BertBrowser.Core.Tests/SavedWorkspaceRulesTests.cs.
use bb_core::FileTime;
use bb_core::layout::{SessionLayout, SplitOrientation};
use bb_core::rules::SavedWorkspaceRules;

fn no_name_taken(_: &str) -> bool {
    false
}

#[test]
fn a_valid_name_has_no_problem() {
    assert_eq!(None, SavedWorkspaceRules::validate("Work", no_name_taken));
}

#[test]
fn a_blank_name_is_refused() {
    for name in ["", "   "] {
        assert!(
            SavedWorkspaceRules::validate(name, no_name_taken)
                .unwrap()
                .to_lowercase()
                .contains("name"),
            "{name:?}"
        );
    }
}

#[test]
fn a_taken_name_is_refused() {
    let problem = SavedWorkspaceRules::validate("Work", |n| n == "Work").unwrap();
    assert!(problem.contains("Work"));
}

#[test]
fn a_name_over_the_length_cap_is_refused() {
    let too_long = "a".repeat(SavedWorkspaceRules::MAX_NAME_LENGTH + 1);
    assert!(SavedWorkspaceRules::validate(&too_long, no_name_taken).is_some());
}

#[test]
fn a_name_at_the_length_cap_is_accepted() {
    let exact = "a".repeat(SavedWorkspaceRules::MAX_NAME_LENGTH);
    assert_eq!(None, SavedWorkspaceRules::validate(&exact, no_name_taken));
}

#[test]
fn the_name_is_trimmed_before_validating() {
    assert_eq!(
        None,
        SavedWorkspaceRules::validate("  Work  ", no_name_taken)
    );
}

#[test]
fn default_name_includes_the_timestamp() {
    let now = FileTime::from_ymd_hms(2026, 3, 5, 14, 30, 0);
    assert_eq!(
        "Workspace 2026-03-05 14:30",
        SavedWorkspaceRules::default_name(now)
    );
}

fn two_panes() -> SessionLayout {
    SessionLayout::split(
        SplitOrientation::Horizontal,
        vec![
            SessionLayout::pane(&[r"C:\A", r"C:\B"]),
            SessionLayout::pane(&[r"c:\a", r"D:\C"]),
        ],
    )
}

#[test]
fn shape_text_counts_panes_and_tabs() {
    assert_eq!(
        "2 panes, 4 tabs",
        SavedWorkspaceRules::shape_text(&two_panes())
    );
    assert_eq!(
        "1 pane, 1 tab",
        SavedWorkspaceRules::shape_text(&SessionLayout::pane(&[r"C:\"]))
    );
}

#[test]
fn folders_are_listed_once_in_pane_then_tab_order_ignoring_case() {
    assert_eq!(
        vec![r"C:\A", r"C:\B", r"D:\C"],
        SavedWorkspaceRules::folders(&two_panes())
    );
}
