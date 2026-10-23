// Port of tests/BertBrowser.Core.Tests/BookmarkRepositoryTests.cs.
use bb_core::data::BookmarkRepository;
use bb_test_support::TempDb;

#[test]
fn add_then_exists_and_get_all() {
    let t = TempDb::new();
    let repo = BookmarkRepository::new(t.db());
    assert!(repo.add(r"C:\data\project", true).unwrap());

    assert!(repo.exists(r"C:\data\project").unwrap());
    let all = repo.get_all().unwrap();
    assert_eq!(1, all.len());
    assert_eq!(r"C:\data\project", all[0].display_path);
    assert!(all[0].is_directory);
}

#[test]
fn exists_is_case_insensitive() {
    let t = TempDb::new();
    let repo = BookmarkRepository::new(t.db());
    repo.add(r"C:\Data\Notes.txt", false).unwrap();
    assert!(repo.exists(r"c:\data\notes.txt").unwrap());
}

#[test]
fn add_same_path_twice_is_no_op() {
    let t = TempDb::new();
    let repo = BookmarkRepository::new(t.db());
    assert!(repo.add(r"C:\data\a", true).unwrap());
    assert!(!repo.add(r"C:\DATA\A", true).unwrap()); // same canonical key
    assert_eq!(1, repo.get_all().unwrap().len());
}

#[test]
fn remove_deletes_bookmark() {
    let t = TempDb::new();
    let repo = BookmarkRepository::new(t.db());
    repo.add(r"C:\data\a", true).unwrap();
    repo.remove(r"c:\data\a").unwrap();
    assert!(!repo.exists(r"C:\data\a").unwrap());
    assert!(repo.get_all().unwrap().is_empty());
}

#[test]
fn get_all_orders_directories_before_files() {
    let t = TempDb::new();
    let repo = BookmarkRepository::new(t.db());
    repo.add(r"C:\z\file.txt", false).unwrap();
    repo.add(r"C:\a\folder", true).unwrap();

    let all = repo.get_all().unwrap();
    assert!(all[0].is_directory);
    assert!(!all[1].is_directory);
}
