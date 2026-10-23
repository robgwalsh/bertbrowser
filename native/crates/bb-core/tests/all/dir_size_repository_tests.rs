// Port of tests/BertBrowser.Core.Tests/DirSizeRepositoryTests.cs (which also holds ByteSizeFormatterTests).
use bb_core::FileTime;
use bb_core::data::{DirSizeRepository, DirSizeResult};
use bb_core::format::byte_size;
use bb_paths::PathKey;
use bb_test_support::TempDb;

fn result(key: &str, size: i64, files: i32, dirs: i32, incomplete: bool) -> DirSizeResult {
    DirSizeResult {
        path_key: key.to_string(),
        size_bytes: size,
        file_count: files,
        dir_count: dirs,
        incomplete,
        computed_utc: FileTime::now(),
    }
}

#[test]
fn upsert_many_overwrites_existing() {
    let t = TempDb::new();
    let repo = DirSizeRepository::new(t.db());
    let key = PathKey::canonicalize(r"C:\some\dir").unwrap();
    repo.upsert_many(&[result(&key, 100, 1, 0, false)]).unwrap();
    repo.upsert_many(&[result(&key, 250, 2, 1, true)]).unwrap();

    let row = repo.get(r"C:\some\dir").unwrap().unwrap();
    assert_eq!(250, row.size_bytes);
    assert_eq!(2, row.file_count);
    assert!(row.incomplete);
}

#[test]
fn get_unknown_directory_is_null() {
    let t = TempDb::new();
    let repo = DirSizeRepository::new(t.db());
    assert!(repo.get(r"C:\never\indexed").unwrap().is_none());
}

#[test]
fn get_many_keys_are_canonicalized_paths() {
    let t = TempDb::new();
    let repo = DirSizeRepository::new(t.db());
    let key = PathKey::canonicalize(r"C:\some\dir").unwrap();
    repo.upsert_many(&[result(&key, 100, 1, 0, false)]).unwrap();

    // Queried with different casing and a trailing separator; both canonicalize to the same key.
    let rows = repo.get_many(&[r"c:\SOME\dir\"]).unwrap();

    assert_eq!(100, rows[&key].size_bytes);
}

#[test]
fn format_human_readable() {
    for (bytes, expected) in [
        (0, "0 B"),
        (999, "999 B"),
        (1024, "1 KB"),
        (1536, "1.5 KB"),
        (1_048_576, "1 MB"),
        (1_469_006, "1.4 MB"),
        (1_073_741_824, "1.000 GB"),
    ] {
        assert_eq!(expected, byte_size(bytes), "bytes {bytes}");
    }
}
