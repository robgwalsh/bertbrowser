// Port of tests/BertBrowser.Core.Tests/EncryptedArchiveTests.cs. Encrypted containers, against
// archives 7-Zip really produced. The two cases must look different to the user and so must look
// different here: a zip with its headers in the clear lists in full and only refuses to open its
// entries, while a 7z with its headers encrypted has nothing to list at all.
use std::sync::Arc;

use bb_core::archives::{
    ArchiveAwareFileSystemService, ArchiveCache, ArchiveFailure, ArchiveLockedError,
    ArchivePasswords, ArchiveReader, LibraryArchiveReader,
};
use bb_core::cancel::CancelToken;
use bb_core::fs::{FileSystemService, LocalFileSystem};
use bb_test_support::archive_fixtures;
use bb_test_support::archive_support::TempRoot;

struct Fixture {
    root: TempRoot,
    reader: LibraryArchiveReader,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            root: TempRoot::new("bertbrowser-enc"),
            reader: LibraryArchiveReader::new(),
        }
    }

    fn fixture(&self, name: &str, base64: &str) -> String {
        archive_fixtures::write_to(&self.root.root.join(name), base64)
    }

    fn read(&self, path: &str, password: Option<&str>) -> bb_core::archives::ArchiveIndex {
        self.reader.read(path, password, &CancelToken::none())
    }
}

fn names(index: &bb_core::archives::ArchiveIndex) -> Vec<String> {
    index
        .children("")
        .expect("root")
        .iter()
        .map(|n| n.name.clone())
        .collect()
}

struct FixedPassword(String);

impl ArchivePasswords for FixedPassword {
    fn for_archive(&self, _archive_file: &str) -> Option<String> {
        Some(self.0.clone())
    }
}

/// Names, sizes and dates are in the clear, so hiding them would be pretending. The lock is on
/// the entries, and that is what the banner says.
#[test]
fn a_zip_with_encrypted_contents_still_lists_in_full() {
    let f = Fixture::new();
    let path = f.fixture("enc.zip", archive_fixtures::ENCRYPTED_ZIP);

    let index = f.read(&path, None);

    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(2, index.file_count);
    assert_eq!(vec!["notes.txt", "secret.txt"], names(&index));
    assert!(index.capabilities.is_encrypted);
    assert!(index.children("").unwrap().iter().all(|n| n.is_encrypted));
}

#[test]
fn reading_an_encrypted_entry_without_the_password_fails_rather_than_returning_rubbish() {
    let f = Fixture::new();
    let path = f.fixture("enc.zip", archive_fixtures::ENCRYPTED_ZIP);

    assert!(
        f.reader
            .read_entry(&path, "secret.txt", 1024, None, &CancelToken::none())
            .is_none()
    );
}

#[test]
fn the_password_unlocks_the_contents() {
    let f = Fixture::new();
    let path = f.fixture("enc.zip", archive_fixtures::ENCRYPTED_ZIP);

    let bytes = f.reader.read_entry(
        &path,
        "secret.txt",
        1024,
        Some(archive_fixtures::ZIP_PASSWORD),
        &CancelToken::none(),
    );

    assert_eq!(Some(b"classified".to_vec()), bytes);
}

/// Nothing can be listed, so the banner is the whole content. This is the arm that must not be
/// reported as "damaged": there is something the user can do about it.
#[test]
fn a7z_with_encrypted_headers_reports_that_a_password_is_needed() {
    let f = Fixture::new();
    let path = f.fixture("enchdr.7z", archive_fixtures::HEADER_ENCRYPTED_SEVEN_ZIP);

    let index = f.read(&path, None);

    assert!(!index.ok());
    assert_eq!(ArchiveFailure::PasswordRequired, index.failure);
    assert!(
        index
            .error
            .as_deref()
            .unwrap()
            .to_lowercase()
            .contains("password")
    );
}

#[test]
fn the_right_password_lists_a_header_encrypted7z() {
    let f = Fixture::new();
    let path = f.fixture("enchdr.7z", archive_fixtures::HEADER_ENCRYPTED_SEVEN_ZIP);

    let index = f.read(&path, Some(archive_fixtures::SEVEN_ZIP_PASSWORD));

    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(vec!["notes.txt", "secret.txt"], names(&index));
}

/// A wrong password is its own message, and it must arrive as a message rather than escaping
/// from a worker thread.
#[test]
fn a_wrong_password_is_reported_rather_than_thrown() {
    let f = Fixture::new();
    let path = f.fixture("enchdr.7z", archive_fixtures::HEADER_ENCRYPTED_SEVEN_ZIP);

    let index = f.read(&path, Some("not-the-password"));

    assert!(!index.ok());
    assert_eq!(ArchiveFailure::PasswordRequired, index.failure);
    assert!(index.error.as_deref().unwrap().contains("did not work"));
}

/// 7z is read-only here, so an ordinary one is the only evidence the format is handled at all:
/// nothing in the dependency graph can write one to round-trip against.
#[test]
fn an_ordinary7z_reads_and_its_entries_open() {
    let f = Fixture::new();
    let path = f.fixture("plain.7z", archive_fixtures::PLAIN_SEVEN_ZIP);

    let index = f.read(&path, None);

    assert!(index.ok(), "{:?}", index.error);
    assert_eq!(2, index.file_count);

    let bytes = f
        .reader
        .read_entry(&path, "secret.txt", 1024, None, &CancelToken::none());
    assert_eq!(Some(b"classified".to_vec()), bytes);
}

/// The listing layer turns a locked container into its own error, so the one caller that can
/// offer to unlock can tell it apart, while everything handling an `io::Error` keeps working.
#[test]
fn listing_a_locked_archive_throws_something_the_banner_can_act_on() {
    let f = Fixture::new();
    let path = f.fixture("enchdr.7z", archive_fixtures::HEADER_ENCRYPTED_SEVEN_ZIP);
    let service = ArchiveAwareFileSystemService::new(
        Arc::new(LocalFileSystem),
        Arc::new(LibraryArchiveReader::new()),
    );

    let error = service.list_directory(&path).unwrap_err();

    let locked = ArchiveLockedError::in_error(&error).expect("a locked-archive error");
    assert_eq!(path, locked.archive_file);
}

/// Once a password is known, the same listing simply works.
#[test]
fn a_known_password_makes_the_listing_succeed() {
    let f = Fixture::new();
    let path = f.fixture("enchdr.7z", archive_fixtures::HEADER_ENCRYPTED_SEVEN_ZIP);
    let service = ArchiveAwareFileSystemService::with_passwords(
        Arc::new(LocalFileSystem),
        Arc::new(LibraryArchiveReader::new()),
        Arc::new(FixedPassword(
            archive_fixtures::SEVEN_ZIP_PASSWORD.to_string(),
        )),
        ArchiveCache::default(),
    );

    let rows = service.list_directory(&path).unwrap();

    let names: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
    assert_eq!(vec!["notes.txt", "secret.txt"], names);
}
