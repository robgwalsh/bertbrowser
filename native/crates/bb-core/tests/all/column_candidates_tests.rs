// Port of tests/BertBrowser.Core.Tests/ColumnCandidatesTests.cs.
use bb_core::columns::{
    ColumnCandidate, ColumnCandidateGroup, ColumnCandidateKind, ColumnCandidates, ColumnKind,
    ColumnLayoutRules, ColumnSetting,
};

fn c(id: &str) -> ColumnSetting {
    ColumnSetting::new(id, 100.0)
}

/// A stand-in for what propsys.dll hands back, so this can run off Windows shells.
const MACHINE: &[(&str, &str)] = &[
    ("System.Photo.DateTaken", "Date taken"),
    ("System.DateAcquired", "Date acquired"),
    ("System.DateArchived", "Date archived"),
    ("System.Music.Artist", "Contributing artists"),
    ("System.ItemTypeText", "Type"),
    ("System.Size", "Size"),
    ("System.IsFolder", "Folder"),
];

fn build(layout: Option<&[ColumnSetting]>) -> ColumnCandidates {
    build_with(layout, "", MACHINE, true)
}

fn build_search(search: &str) -> ColumnCandidates {
    build_with(None, search, MACHINE, true)
}

fn build_with(
    layout: Option<&[ColumnSetting]>,
    search: &str,
    machine: &[(&str, &str)],
    loaded: bool,
) -> ColumnCandidates {
    ColumnCandidates::build(layout, machine, search, loaded)
}

fn ids(candidates: &[ColumnCandidate]) -> Vec<&str> {
    candidates.iter().map(|c| c.id.as_str()).collect()
}

fn group_ids(group: &ColumnCandidateGroup) -> Vec<&str> {
    ids(&group.items)
}

fn group(candidates: &ColumnCandidates, kind: ColumnCandidateKind) -> &ColumnCandidateGroup {
    let found: Vec<&ColumnCandidateGroup> = candidates
        .groups
        .iter()
        .filter(|g| g.kind == kind)
        .collect();
    assert_eq!(1, found.len(), "exactly one {kind:?} group");
    found[0]
}

fn common(candidates: &ColumnCandidates) -> &ColumnCandidateGroup {
    group(candidates, ColumnCandidateKind::Common)
}

fn all(candidates: &ColumnCandidates) -> &ColumnCandidateGroup {
    group(candidates, ColumnCandidateKind::All)
}

// --- What the two groups hold ---

#[test]
fn common_leads_with_the_built_ins_in_catalogue_order() {
    // Catalogue order, not alphabetical: it is the order a column set is usually built in, and
    // sorting would put Accessed above Modified.
    let candidates = build(None);
    let common = group_ids(common(&candidates));

    assert_eq!(
        vec!["Created", "Accessed", "Attributes", "Extension"],
        common[..4].to_vec()
    );
}

#[test]
fn common_then_carries_the_curated_properties() {
    let candidates = build(None);
    let common = &common(&candidates).items;

    assert!(common.iter().any(|c| c.id == "System.Photo.DateTaken"));
    for c in common.iter().skip_while(|c| c.kind == ColumnKind::BuiltIn) {
        assert_eq!(ColumnKind::ShellProperty, c.kind, "{}", c.id);
    }
}

#[test]
fn a_column_already_in_the_layout_is_not_offered_again() {
    // The popup adds; it never removes. A row that is already showing would do nothing.
    let candidates = build(Some(&[
        c("Name"),
        c("Size"),
        c("Created"),
        c("System.Photo.DateTaken"),
    ]));

    assert!(!group_ids(common(&candidates)).contains(&"Created"));
    assert!(!group_ids(common(&candidates)).contains(&"System.Photo.DateTaken"));
    assert!(!group_ids(all(&candidates)).contains(&"System.Photo.DateTaken"));
}

#[test]
fn the_injected_columns_are_never_offered() {
    // Folder and Match follow the list's mode, not anyone's choice.
    let candidates = build(None);
    let common = group_ids(common(&candidates));

    assert!(!common.contains(&"RelativePath"));
    assert!(!common.contains(&"Match"));
}

#[test]
fn a_property_that_would_shadow_a_built_in_is_not_offered() {
    let candidates = build(None);
    let all = group_ids(all(&candidates));

    assert!(!all.contains(&"System.ItemTypeText"));
    assert!(!all.contains(&"System.Size"));
}

#[test]
fn a_property_already_on_the_curated_list_is_not_repeated_under_all_properties() {
    let candidates = build(None);

    assert!(group_ids(common(&candidates)).contains(&"System.Photo.DateTaken"));
    assert!(!group_ids(all(&candidates)).contains(&"System.Photo.DateTaken"));
}

#[test]
fn all_properties_is_sorted_by_the_localised_name_the_shell_gave_back() {
    let candidates = build(None);
    let headers: Vec<&str> = all(&candidates)
        .items
        .iter()
        .map(|c| c.header.as_str())
        .collect();
    assert_eq!(vec!["Date acquired", "Date archived", "Folder"], headers);
}

#[test]
fn a_property_whose_name_is_not_canonical_is_refused() {
    // The same rule that keeps a hand-edited settings file from putting arbitrary text into a
    // header: no dot means it is a built-in id, and the shell has no business minting those.
    let candidates = build_with(
        None,
        "",
        &[("NotCanonical", "Bare word"), ("System.Ok", "Fine")],
        true,
    );

    assert_eq!(vec!["System.Ok"], group_ids(all(&candidates)));
}

#[test]
fn the_display_name_the_shell_gave_back_wins_over_the_synthetic_one() {
    // ColumnCatalog::spec_for_property shortens the canonical name as a fallback; when the
    // machine has a localised name it is the better label and the whole reason the list is
    // enumerated.
    let candidates = build(None);
    let taken: Vec<&ColumnCandidate> = common(&candidates)
        .items
        .iter()
        .filter(|c| c.id == "System.Photo.DateTaken")
        .collect();
    assert_eq!(1, taken.len());

    assert_eq!("Date taken", taken[0].header);
}

// --- Searching ---

#[test]
fn search_matches_the_displayed_name_in_either_group() {
    let candidates = build_search("date");

    assert!(group_ids(common(&candidates)).contains(&"System.Photo.DateTaken"));
    assert_eq!(
        vec!["System.DateAcquired", "System.DateArchived"],
        group_ids(all(&candidates))
    );
}

#[test]
fn search_also_matches_the_canonical_name() {
    // The canonical name is what ends up in settings.json and is shown on the row, so it has to
    // be searchable too — nothing about the label "Folder" contains the word searched for here.
    let candidates = build_search("isfolder");
    assert_eq!(vec!["System.IsFolder"], group_ids(all(&candidates)));
}

#[test]
fn search_ignores_case_and_surrounding_space() {
    let upper = build_search("DATE");
    let padded = build_search("  date  ");
    assert_eq!(group_ids(all(&upper)), group_ids(all(&padded)));
}

#[test]
fn search_finds_a_built_in_by_its_header() {
    let candidates = build_search("attributes");
    assert_eq!(vec!["Attributes"], group_ids(common(&candidates)));
}

#[test]
fn a_group_with_nothing_left_in_it_is_dropped() {
    // "acquired" is a word only one enumerated property has, and no built-in or curated one does.
    let candidates = build_search("acquired");

    let kinds: Vec<ColumnCandidateKind> = candidates.groups.iter().map(|g| g.kind).collect();
    assert_eq!(vec![ColumnCandidateKind::All], kinds);
}

#[test]
fn a_search_that_matches_nothing_returns_no_groups_and_says_so() {
    let candidates = build_search("zzzz");

    assert!(candidates.groups.is_empty());
    assert!(candidates.is_empty());
}

// --- While the property system is still being read ---

#[test]
fn common_is_offered_before_the_machine_has_been_enumerated() {
    // A few hundred COM activations happen off the UI thread; the curated list needs no machine
    // at all, so it renders first rather than the popup opening blank.
    let candidates = build_with(None, "", &[], false);

    assert!(group_ids(common(&candidates)).contains(&"Created"));
    assert!(
        !candidates
            .groups
            .iter()
            .any(|g| g.kind == ColumnCandidateKind::All)
    );
    assert!(candidates.is_loading);
    assert!(!candidates.is_empty());
}

#[test]
fn once_loaded_a_machine_with_no_properties_is_not_still_loading() {
    let candidates = build_with(None, "", &[], true);

    assert!(!candidates.is_loading);
    assert!(group_ids(common(&candidates)).contains(&"Created"));
}

#[test]
fn a_layout_at_the_column_limit_offers_nothing() {
    // MAX_COLUMNS is a hard cap; offering a row that normalize would silently drop would read as
    // the popup being broken.
    let full: Vec<ColumnSetting> = (0..ColumnLayoutRules::MAX_COLUMNS)
        .map(|i| {
            if i == 0 {
                c("Name")
            } else {
                c(&format!("System.Filler{i}"))
            }
        })
        .collect();

    let candidates = build(Some(&full));

    assert!(candidates.groups.is_empty());
    assert!(candidates.is_full);
}
