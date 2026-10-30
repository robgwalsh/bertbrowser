// Port of tests/BertBrowser.Core.Tests/SessionLayoutTests.cs: restoring a session reads a file that a
// previous version wrote or that somebody edited by hand, so a layout that cannot be honoured degrades
// to the ordinary single-pane start.
use bb_core::layout::SplitOrientation::{Horizontal, Vertical};
use bb_core::layout::{SessionLayout, SessionLayoutRules, SessionTab};

fn pane(paths: &[&str]) -> SessionLayout {
    SessionLayout::pane(paths)
}

fn split(
    orientation: bb_core::layout::SplitOrientation,
    children: Vec<SessionLayout>,
) -> SessionLayout {
    SessionLayout::split(orientation, children)
}

/// Everything exists.
fn all(_: &str) -> bool {
    true
}

fn tab_paths(layout: &SessionLayout) -> Vec<String> {
    SessionLayoutRules::panes(layout)
        .iter()
        .flat_map(|p| p.tabs.iter().flatten().map(|t| t.path.clone()))
        .collect()
}

// --- Round trip ---

#[test]
fn a_nested_layout_survives_json() {
    let mut original = split(
        Vertical,
        vec![
            pane(&[r"C:\a"]),
            split(
                Horizontal,
                vec![pane(&[r"C:\b", r"C:\c"]), pane(&[r"C:\d"])],
            ),
        ],
    );
    original.children.as_mut().unwrap()[1].weight = 2.5;
    original.children.as_mut().unwrap()[1]
        .children
        .as_mut()
        .unwrap()[0]
        .active_tab_index = 1;

    let restored: SessionLayout =
        serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();

    // Three panes, not four: the middle one holds two tabs.
    assert_eq!(3, SessionLayoutRules::count_panes(&restored));
    assert_eq!(2.5, restored.children.as_ref().unwrap()[1].weight);
    assert_eq!(
        1,
        restored.children.as_ref().unwrap()[1]
            .children
            .as_ref()
            .unwrap()[0]
            .active_tab_index
    );
    assert_eq!(
        vec![r"C:\a", r"C:\b", r"C:\c", r"C:\d"],
        tab_paths(&restored)
    );
}

/// On-screen order, which is what "focus the next pane" walks.
#[test]
fn panes_come_back_in_screen_order() {
    let layout = split(
        Vertical,
        vec![
            pane(&[r"C:\left"]),
            split(Horizontal, vec![pane(&[r"C:\top"]), pane(&[r"C:\bottom"])]),
        ],
    );

    let first_tabs: Vec<String> = SessionLayoutRules::panes(&layout)
        .iter()
        .map(|p| p.tabs.as_ref().unwrap()[0].path.clone())
        .collect();
    assert_eq!(vec![r"C:\left", r"C:\top", r"C:\bottom"], first_tabs);
}

// --- Pruning ---

#[test]
fn a_missing_path_is_dropped_and_the_survivors_keep_their_weights() {
    let mut layout = split(
        Vertical,
        vec![pane(&[r"C:\gone", r"C:\here"]), pane(&[r"C:\also"])],
    );
    layout.children.as_mut().unwrap()[0].weight = 3.0;

    let pruned = SessionLayoutRules::prune(Some(layout), &|p| p != r"C:\gone").unwrap();

    assert_eq!(2, SessionLayoutRules::count_panes(&pruned));
    assert_eq!(3.0, pruned.children.as_ref().unwrap()[0].weight);
    let tabs = pruned.children.as_ref().unwrap()[0].tabs.as_ref().unwrap();
    assert_eq!(1, tabs.len());
    assert_eq!(r"C:\here", tabs[0].path);
}

/// An unplugged drive takes its pane with it rather than leaving an empty one.
#[test]
fn a_pane_whose_every_tab_is_gone_is_dropped() {
    let layout = split(
        Vertical,
        vec![pane(&[r"D:\removable"]), pane(&[r"C:\here"])],
    );

    let pruned = SessionLayoutRules::prune(Some(layout), &|p| p.starts_with(r"C:\")).unwrap();

    assert_eq!(1, SessionLayoutRules::count_panes(&pruned));
    assert_eq!(r"C:\here", pruned.tabs.as_ref().unwrap()[0].path);
}

#[test]
fn a_split_left_with_one_child_collapses_into_it() {
    let mut layout = split(Vertical, vec![pane(&[r"C:\gone"]), pane(&[r"C:\here"])]);
    layout.weight = 4.0;

    let pruned = SessionLayoutRules::prune(Some(layout), &|p| p == r"C:\here").unwrap();

    assert!(!pruned.is_split());
    assert_eq!(4.0, pruned.weight);
}

#[test]
fn nothing_left_is_null_so_the_caller_starts_normally() {
    let layout = split(
        Vertical,
        vec![pane(&[r"C:\gone"]), pane(&[r"D:\also-gone"])],
    );

    assert!(SessionLayoutRules::prune(Some(layout), &|_| false).is_none());
    assert!(!SessionLayoutRules::is_usable(None));
}

#[test]
fn an_active_tab_index_past_the_end_is_clamped() {
    let mut p = pane(&[r"C:\a", r"C:\b"]);
    p.active_tab_index = 99;

    let pruned = SessionLayoutRules::prune(Some(p), &all).unwrap();

    assert_eq!(1, pruned.active_tab_index);
}

/// The index follows what survived, not what was saved.
#[test]
fn an_active_tab_index_is_clamped_after_pruning() {
    let mut p = pane(&[r"C:\a", r"C:\gone", r"C:\gone2"]);
    p.active_tab_index = 2;

    let pruned = SessionLayoutRules::prune(Some(p), &|x| x == r"C:\a").unwrap();

    assert_eq!(0, pruned.active_tab_index);
}

/// A weight of zero, NaN or infinity would give a grid a column it can never lay out.
#[test]
fn an_unusable_weight_becomes_an_even_share() {
    for weight in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut p = pane(&[r"C:\a"]);
        p.weight = weight;
        assert_eq!(
            1.0,
            SessionLayoutRules::prune(Some(p), &all).unwrap().weight,
            "weight {weight}"
        );
    }
}

#[test]
fn empty_and_blank_paths_are_dropped_rather_than_restored_as_blank_tabs() {
    let p = SessionLayout {
        tabs: Some(vec![SessionTab::at(""), SessionTab::at(r"C:\real")]),
        ..Default::default()
    };

    let pruned = SessionLayoutRules::prune(Some(p), &all).unwrap();

    let tabs = pruned.tabs.as_ref().unwrap();
    assert_eq!(1, tabs.len());
    assert_eq!(r"C:\real", tabs[0].path);
}

// --- Guards against a file claiming an absurd arrangement ---

#[test]
fn more_panes_than_the_cap_is_refused_rather_than_built() {
    let children: Vec<SessionLayout> = (0..=SessionLayoutRules::MAX_PANES)
        .map(|i| pane(&[&format!(r"C:\p{i}")]))
        .collect();

    let layout = SessionLayoutRules::prune(Some(split(Vertical, children)), &all);

    assert!(!SessionLayoutRules::is_usable(layout.as_ref()));
}

#[test]
fn too_many_tabs_are_truncated_rather_than_refused() {
    let paths: Vec<String> = (0..SessionLayoutRules::MAX_TABS_PER_PANE + 10)
        .map(|i| format!(r"C:\t{i}"))
        .collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();

    let pruned = SessionLayoutRules::prune(Some(pane(&refs)), &all).unwrap();

    assert_eq!(
        SessionLayoutRules::MAX_TABS_PER_PANE,
        pruned.tabs.as_ref().unwrap().len()
    );
}

/// A node claiming to be both is read as a split, and the stray tabs are dropped.
#[test]
fn a_node_that_is_both_is_resolved_to_a_split() {
    let mut confused = split(Vertical, vec![pane(&[r"C:\a"]), pane(&[r"C:\b"])]);
    confused.tabs = Some(vec![SessionTab::at(r"C:\stray")]);

    let pruned = SessionLayoutRules::prune(Some(confused), &all).unwrap();

    assert!(pruned.is_split());
    assert!(pruned.tabs.is_none());
}
