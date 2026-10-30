// Port of tests/BertBrowser.Core.Tests/SavedSearchRepositoryTests.cs.
use bb_core::FileTime;
use bb_core::data::{SavedSearch, SavedSearchRepository, SavedSearchScope};
use bb_test_support::TempDb;

fn setup() -> (TempDb, SavedSearchRepository) {
    let t = TempDb::new();
    let repo = SavedSearchRepository::new(t.db());
    (t, repo)
}

fn single(repo: &SavedSearchRepository) -> SavedSearch {
    let all = repo.get_all().unwrap();
    assert_eq!(1, all.len(), "expected one saved search");
    all.into_iter().next().unwrap()
}

#[test]
fn save_then_get_all_round_trips_every_field() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "Big videos",
        "ext:mp4 size:>1gb",
        SavedSearchScope::Folder,
        Some(r"C:\Videos"),
    ))
    .unwrap();

    let s = single(&repo);
    assert_eq!("Big videos", s.name);
    assert_eq!("ext:mp4 size:>1gb", s.query);
    assert_eq!(SavedSearchScope::Folder, s.scope);
    assert_eq!(Some(r"C:\Videos".to_string()), s.scope_path);
}

#[test]
fn null_scope_path_round_trips() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "Recent",
        "dm:today",
        SavedSearchScope::ThisPc,
        None,
    ))
    .unwrap();

    let s = single(&repo);
    assert_eq!(SavedSearchScope::ThisPc, s.scope);
    assert_eq!(None, s.scope_path);
}

#[test]
fn scope_path_is_stored_with_display_casing_and_no_trailing_separator() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "Pinned",
        "report",
        SavedSearchScope::Folder,
        Some(r"C:\Work\Reports\"),
    ))
    .unwrap();

    assert_eq!(
        Some(r"C:\Work\Reports".to_string()),
        single(&repo).scope_path
    );
}

#[test]
fn exists_is_case_insensitive() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "Big Videos",
        "ext:mp4",
        SavedSearchScope::CurrentFolder,
        None,
    ))
    .unwrap();

    assert!(repo.exists("big videos").unwrap());
    assert!(!repo.exists("small videos").unwrap());
}

#[test]
fn save_under_an_existing_name_in_any_case_replaces_rather_than_duplicating() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "Docs",
        "ext:doc",
        SavedSearchScope::CurrentFolder,
        None,
    ))
    .unwrap();
    repo.save(&SavedSearch::new(
        "DOCS",
        "ext:docx",
        SavedSearchScope::ThisPc,
        None,
    ))
    .unwrap();

    let s = single(&repo);
    assert_eq!("ext:docx", s.query);
    assert_eq!(SavedSearchScope::ThisPc, s.scope);
}

#[test]
fn rename_moves_the_row() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "Old",
        "ext:txt",
        SavedSearchScope::CurrentFolder,
        None,
    ))
    .unwrap();

    assert!(repo.rename("Old", "New").unwrap());

    let s = single(&repo);
    assert_eq!("New", s.name);
    assert_eq!("ext:txt", s.query);
}

#[test]
fn rename_onto_another_row_is_refused_and_changes_nothing() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "A",
        "ext:a",
        SavedSearchScope::CurrentFolder,
        None,
    ))
    .unwrap();
    repo.save(&SavedSearch::new(
        "B",
        "ext:b",
        SavedSearchScope::CurrentFolder,
        None,
    ))
    .unwrap();

    assert!(!repo.rename("A", "b").unwrap());

    let all = repo.get_all().unwrap();
    assert_eq!(2, all.len());
    assert_eq!("ext:a", all.iter().find(|s| s.name == "A").unwrap().query);
    assert_eq!("ext:b", all.iter().find(|s| s.name == "B").unwrap().query);
}

#[test]
fn rename_that_only_changes_case_succeeds() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "big videos",
        "ext:mp4",
        SavedSearchScope::CurrentFolder,
        None,
    ))
    .unwrap();

    assert!(repo.rename("big videos", "Big Videos").unwrap());

    assert_eq!("Big Videos", single(&repo).name);
}

#[test]
fn rename_of_a_missing_row_returns_false() {
    let (_t, repo) = setup();
    assert!(!repo.rename("Nope", "Still nope").unwrap());
}

#[test]
fn get_all_is_ordered_by_name_ignoring_case() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "zebra",
        "ext:z",
        SavedSearchScope::CurrentFolder,
        None,
    ))
    .unwrap();
    repo.save(&SavedSearch::new(
        "Apple",
        "ext:a",
        SavedSearchScope::CurrentFolder,
        None,
    ))
    .unwrap();
    repo.save(&SavedSearch::new(
        "mango",
        "ext:m",
        SavedSearchScope::CurrentFolder,
        None,
    ))
    .unwrap();

    let names: Vec<String> = repo
        .get_all()
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(vec!["Apple", "mango", "zebra"], names);
}

#[test]
fn remove_deletes_the_row() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "Gone",
        "ext:g",
        SavedSearchScope::CurrentFolder,
        None,
    ))
    .unwrap();

    repo.remove("gone").unwrap();

    assert!(!repo.exists("Gone").unwrap());
    assert!(repo.get_all().unwrap().is_empty());
}

#[test]
fn a_new_search_has_a_creation_time_and_has_never_been_run() {
    let (_t, repo) = setup();
    let before = FileTime::now().add_secs(-1);
    repo.save(&SavedSearch::new(
        "Docs",
        "ext:docx",
        SavedSearchScope::ThisPc,
        None,
    ))
    .unwrap();

    let s = single(&repo);
    let created = s.created_utc.expect("created");
    assert!(created >= before && created <= FileTime::now().add_secs(1));
    assert_eq!(None, s.last_used_utc);
}

#[test]
fn mark_used_survives_an_edit_and_a_rename() {
    let (_t, repo) = setup();
    repo.save(&SavedSearch::new(
        "Docs",
        "ext:docx",
        SavedSearchScope::ThisPc,
        None,
    ))
    .unwrap();
    let created = single(&repo).created_utc;
    let used = FileTime::from_ymd_hms(2026, 9, 1, 12, 30, 0);

    repo.mark_used("docs", used).unwrap();
    repo.save(&SavedSearch::new(
        "Docs",
        "ext:pdf",
        SavedSearchScope::ThisPc,
        None,
    ))
    .unwrap();
    repo.rename("Docs", "Papers").unwrap();

    let s = single(&repo);
    assert_eq!(Some(used), s.last_used_utc);
    assert_eq!(created, s.created_utc);
}
