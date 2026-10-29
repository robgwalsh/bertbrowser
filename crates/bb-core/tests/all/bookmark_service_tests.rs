// Not a port: BookmarkService has no C# tests. Pins the one rule it adds over the repository — a
// path inside an archive is refused here, not only in the menu.
use bb_core::data::{BookmarkRepository, BookmarkService};
use bb_test_support::{TempDb, unique_temp_path};

#[test]
fn a_path_inside_an_archive_is_refused() {
    // The bookmark table is keyed by PathKey, and a virtual path canonicalizes perfectly happily —
    // one such row would make every subtree range scan over its folder return archive interiors.
    let t = TempDb::new();
    let service = BookmarkService::new(BookmarkRepository::new(t.db()));
    let dir = unique_temp_path("bertbrowser-bookmark", "");
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("bundle.zip");
    std::fs::write(&archive, b"PK").unwrap();
    let inside = format!("{}\\src\\main.rs", archive.display());

    assert!(!service.add(&inside, false).unwrap());
    assert!(!service.exists(&inside).unwrap());
    assert!(service.get_all().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ordinary_path_goes_through_to_the_repository() {
    let t = TempDb::new();
    let service = BookmarkService::new(BookmarkRepository::new(t.db()));

    assert!(service.add(r"C:\data\project", true).unwrap());
    assert!(!service.add(r"C:\data\project", true).unwrap());
    assert!(service.exists(r"c:\DATA\project").unwrap());
    assert_eq!(1, service.get_all().unwrap().len());

    service.remove(r"C:\data\project").unwrap();
    assert!(service.get_all().unwrap().is_empty());
}
