// Port of tests/BertBrowser.Core.Tests/SavedWorkspaceRepositoryTests.cs.
use bb_core::FileTime;
use bb_core::data::{SavedWorkspace, SavedWorkspaceRepository};
use bb_core::layout::SessionLayout;
use bb_test_support::TempDb;

fn setup() -> (TempDb, SavedWorkspaceRepository) {
    let t = TempDb::new();
    let repo = SavedWorkspaceRepository::new(t.db());
    (t, repo)
}

fn one_pane(paths: &[&str]) -> SessionLayout {
    SessionLayout::pane(paths)
}

fn single(repo: &SavedWorkspaceRepository) -> SavedWorkspace {
    let all = repo.get_all().unwrap();
    assert_eq!(1, all.len(), "expected one workspace");
    all.into_iter().next().unwrap()
}

fn tab_path(w: &SavedWorkspace, i: usize) -> &str {
    &w.layout.tabs.as_ref().unwrap()[i].path
}

#[test]
fn save_then_get_all_round_trips_the_layout() {
    let (_t, repo) = setup();
    repo.save(&SavedWorkspace::new(
        "Work",
        one_pane(&[r"C:\Work", r"C:\Work\Reports"]),
    ))
    .unwrap();

    let w = single(&repo);
    assert_eq!("Work", w.name);
    assert_eq!(2, w.layout.tabs.as_ref().unwrap().len());
    assert_eq!(r"C:\Work", tab_path(&w, 0));
    assert_eq!(r"C:\Work\Reports", tab_path(&w, 1));
}

#[test]
fn exists_is_case_insensitive() {
    let (_t, repo) = setup();
    repo.save(&SavedWorkspace::new("Dev Setup", one_pane(&[r"C:\Dev"])))
        .unwrap();

    assert!(repo.exists("dev setup").unwrap());
    assert!(!repo.exists("something else").unwrap());
}

#[test]
fn save_under_an_existing_name_in_any_case_replaces_rather_than_duplicating() {
    let (_t, repo) = setup();
    repo.save(&SavedWorkspace::new("Docs", one_pane(&[r"C:\Docs"])))
        .unwrap();
    repo.save(&SavedWorkspace::new("DOCS", one_pane(&[r"C:\Docs2"])))
        .unwrap();

    let w = single(&repo);
    assert_eq!(r"C:\Docs2", tab_path(&w, 0));
}

#[test]
fn rename_moves_the_row() {
    let (_t, repo) = setup();
    repo.save(&SavedWorkspace::new("Old", one_pane(&[r"C:\A"])))
        .unwrap();

    assert!(repo.rename("Old", "New").unwrap());

    let w = single(&repo);
    assert_eq!("New", w.name);
    assert_eq!(r"C:\A", tab_path(&w, 0));
}

#[test]
fn rename_onto_another_row_is_refused_and_changes_nothing() {
    let (_t, repo) = setup();
    repo.save(&SavedWorkspace::new("A", one_pane(&[r"C:\A"])))
        .unwrap();
    repo.save(&SavedWorkspace::new("B", one_pane(&[r"C:\B"])))
        .unwrap();

    assert!(!repo.rename("A", "b").unwrap());

    assert_eq!(2, repo.get_all().unwrap().len());
}

#[test]
fn rename_that_only_changes_case_succeeds() {
    let (_t, repo) = setup();
    repo.save(&SavedWorkspace::new("dev setup", one_pane(&[r"C:\Dev"])))
        .unwrap();

    assert!(repo.rename("dev setup", "Dev Setup").unwrap());

    assert_eq!("Dev Setup", single(&repo).name);
}

#[test]
fn rename_of_a_missing_row_returns_false() {
    let (_t, repo) = setup();
    assert!(!repo.rename("Nope", "Still nope").unwrap());
}

#[test]
fn get_all_is_ordered_by_name_ignoring_case() {
    let (_t, repo) = setup();
    repo.save(&SavedWorkspace::new("zebra", one_pane(&[r"C:\Z"])))
        .unwrap();
    repo.save(&SavedWorkspace::new("Apple", one_pane(&[r"C:\A"])))
        .unwrap();
    repo.save(&SavedWorkspace::new("mango", one_pane(&[r"C:\M"])))
        .unwrap();

    let names: Vec<String> = repo
        .get_all()
        .unwrap()
        .into_iter()
        .map(|w| w.name)
        .collect();
    assert_eq!(vec!["Apple", "mango", "zebra"], names);
}

#[test]
fn remove_deletes_the_row() {
    let (_t, repo) = setup();
    repo.save(&SavedWorkspace::new("Gone", one_pane(&[r"C:\Gone"])))
        .unwrap();

    repo.remove("gone").unwrap();

    assert!(!repo.exists("Gone").unwrap());
    assert!(repo.get_all().unwrap().is_empty());
}

#[test]
fn a_row_with_unparsable_layout_json_is_skipped_rather_than_failing_the_whole_list() {
    let (t, repo) = setup();
    repo.save(&SavedWorkspace::new("Good", one_pane(&[r"C:\Good"])))
        .unwrap();

    // Corrupt the second row directly, as a hand-edited DB or a future format change would.
    t.db.open()
        .unwrap()
        .execute(
            "INSERT INTO saved_workspace(name, layout_json, added_utc) VALUES ('Bad', 'not json', ?1);",
            [FileTime::now().0],
        )
        .unwrap();

    let w = single(&repo);
    assert_eq!("Good", w.name);
}

#[test]
fn a_new_workspace_has_a_creation_time_and_has_never_been_used() {
    let (_t, repo) = setup();
    let before = FileTime::now().add_secs(-1);
    repo.save(&SavedWorkspace::new("Work", one_pane(&[r"C:\Work"])))
        .unwrap();

    let w = single(&repo);
    let created = w.created_utc.expect("created");
    assert!(created >= before && created <= FileTime::now().add_secs(1));
    assert_eq!(None, w.last_used_utc);
}

#[test]
fn mark_used_records_the_time_and_survives_a_rename() {
    let (_t, repo) = setup();
    repo.save(&SavedWorkspace::new("Work", one_pane(&[r"C:\Work"])))
        .unwrap();
    let used = FileTime::from_ymd_hms(2026, 9, 1, 12, 30, 0);

    repo.mark_used("work", used).unwrap();
    repo.rename("Work", "Office").unwrap();

    assert_eq!(Some(used), single(&repo).last_used_utc);
}

#[test]
fn rename_keeps_the_creation_time() {
    let (_t, repo) = setup();
    repo.save(&SavedWorkspace::new("Old", one_pane(&[r"C:\A"])))
        .unwrap();
    let created = single(&repo).created_utc;

    repo.rename("Old", "New").unwrap();

    assert_eq!(created, single(&repo).created_utc);
}
