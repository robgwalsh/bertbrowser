// Port of tests/BertBrowser.Core.Tests/FileSystemFileHasherTests.cs
//
// The real hasher against real files. The grouping rules are covered with a fake; what needs a
// disk is the part that touches one — the digest being what it claims, the sample stopping where
// it should, and above all the sharing flags, which no unit test of the scanner could catch.
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};

use bb_core::cancel::{CancelToken, Cancelled};
use bb_core::checksums::{FileSystemFileHasher, to_hex_upper};
use bb_core::duplicates::{FileFingerprint, FileHasher};
use bb_core::fs::DirectoryRemoval;
use bb_test_support::unique_temp_path;
use sha2::Digest;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

struct Fixture {
    root: PathBuf,
    hasher: FileSystemFileHasher,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-hash", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            root,
            hasher: FileSystemFileHasher::new(),
        }
    }

    fn file(&self, content: &[u8], name: &str) -> String {
        let path = self.root.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn hash(&self, path: &str, max_bytes: i64) -> Option<FileFingerprint> {
        self.hasher
            .hash(path, max_bytes, None, &CancelToken::none())
            .expect("not cancelled")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = DirectoryRemoval::remove_tree(&self.root.to_string_lossy());
    }
}

fn bytes(length: usize) -> Vec<u8> {
    (0..length).map(|i| (i % 251) as u8).collect()
}

fn sha256(content: &[u8]) -> String {
    to_hex_upper(&sha2::Sha256::digest(content))
}

// --- the digest ---

#[test]
fn known_content_gives_the_known_digest() {
    let f = Fixture::new();
    let content = bytes(10_000);
    let path = f.file(&content, "a.bin");

    let fingerprint = f.hash(&path, 0).expect("a fingerprint");

    assert_eq!(sha256(&content), fingerprint.hash);
    assert_eq!(10_000, fingerprint.bytes_read);
}

/// The sampling pass depends on this exactly: read the bound, hash only that, and report how much
/// was taken so the caller can tell "the whole file" from "the first part of it".
#[test]
fn the_sample_stops_at_its_bound() {
    let f = Fixture::new();
    let content = bytes(10_000);
    let path = f.file(&content, "a.bin");

    let fingerprint = f.hash(&path, 4096).expect("a fingerprint");

    assert_eq!(sha256(&content[..4096]), fingerprint.hash);
    assert_eq!(4096, fingerprint.bytes_read);
}

/// A bound past the end reads to the end and says so — which is how a file smaller than the
/// sample is recognised as having been hashed in full.
#[test]
fn a_bound_past_the_end_reads_to_the_end() {
    let f = Fixture::new();
    let content = bytes(100);
    let path = f.file(&content, "a.bin");

    let fingerprint = f.hash(&path, 4096).expect("a fingerprint");

    assert_eq!(100, fingerprint.bytes_read);
    assert_eq!(sha256(&content), fingerprint.hash);
}

#[test]
fn progress_reports_chunk_deltas_adding_up_to_what_was_read() {
    let f = Fixture::new();
    let path = f.file(&bytes(10_000), "a.bin");

    let total = AtomicI64::new(0);
    let add = |delta: i64| {
        total.fetch_add(delta, Ordering::SeqCst);
    };
    let fingerprint = f
        .hasher
        .hash(&path, 0, Some(&add), &CancelToken::none())
        .unwrap()
        .expect("a fingerprint");

    assert_eq!(fingerprint.bytes_read, total.into_inner());
}

// --- the sharing rule ---

/// **The rule the preview pane already follows, and the reason this type opens the way it does.**
/// Hashing must never block, or be blocked by, someone else holding the file — in this app that
/// someone is usually its own rename, move or delete executor working in the folder the user is
/// standing in. Narrow the share flags to read-only sharing and this goes red with a sharing
/// violation.
#[test]
fn a_file_someone_else_has_open_for_writing_is_still_hashable() {
    let f = Fixture::new();
    let content = bytes(4096);
    let path = f.file(&content, "busy.bin");

    let _writer = OpenOptions::new()
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&path)
        .unwrap();

    let fingerprint = f.hash(&path, 0).expect("a fingerprint");

    assert_eq!(sha256(&content), fingerprint.hash);
}

// --- refusals ---

/// A file that has gone is not an error worth throwing over: one candidate's failure marks the
/// scan incomplete and the rest carry on, which is the rule every executor in this app holds to.
#[test]
fn a_missing_file_is_null_not_a_throw() {
    let f = Fixture::new();
    let missing = f
        .root
        .join("never-existed.bin")
        .to_string_lossy()
        .into_owned();

    assert_eq!(None, f.hash(&missing, 0));
}

#[test]
fn a_directory_is_null_not_a_throw() {
    let f = Fixture::new();
    let directory = f.root.join("folder");
    std::fs::create_dir_all(&directory).unwrap();

    assert_eq!(None, f.hash(&directory.to_string_lossy(), 0));
}

/// Cancellation is not a per-file failure and must not come back as one: None means "this file
/// had a problem", and conflating the two would let a cancelled run look like a disk full of
/// unreadable files.
#[test]
fn a_cancelled_token_throws_rather_than_returning_null() {
    let f = Fixture::new();
    let path = f.file(&bytes(4096), "a.bin");
    let cancel = CancelToken::new();
    cancel.cancel();

    assert_eq!(Err(Cancelled), f.hasher.hash(&path, 0, None, &cancel));
}

// --- identity ---

/// An ordinary file carries one name, and the scanner then does no identity bookkeeping for it at
/// all. Reporting an identity here would put every file on the shortlist through the
/// hardlink-folding path for nothing.
#[test]
fn an_ordinary_file_has_no_identity() {
    let f = Fixture::new();
    let path = f.file(&bytes(4096), "a.bin");

    let fingerprint = f.hash(&path, 0).expect("a fingerprint");

    assert_eq!(None, fingerprint.identity);
}
