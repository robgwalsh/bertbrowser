// Port of tests/BertBrowser.Core.Tests/FileDigesterTests.cs
//
// The multi-algorithm half of `FileSystemFileHasher`. Its sibling, `file_system_file_hasher_tests`,
// is deliberately unchanged: between them they hold down the claim that both seams run one read
// loop.
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};

use bb_core::cancel::{CancelToken, Cancelled};
use bb_core::checksums::{
    ChecksumAlgorithm, ChecksumAlgorithms, FileDigester, FileDigests, FileSystemFileHasher,
    to_hex_upper,
};
use bb_core::fs::DirectoryRemoval;
use bb_test_support::unique_temp_path;
use sha2::Digest;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

struct Fixture {
    root: PathBuf,
    digester: FileSystemFileHasher,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-digest", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            root,
            digester: FileSystemFileHasher::new(),
        }
    }

    fn file(&self, content: &[u8]) -> String {
        self.file_named(content, "file.bin")
    }

    fn file_named(&self, content: &[u8], name: &str) -> String {
        let path = self.root.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn digest(&self, path: &str, algorithms: &[ChecksumAlgorithm]) -> FileDigests {
        self.digester
            .digest(path, algorithms, None, &CancelToken::none())
            .expect("not cancelled")
            .expect("expected a digest")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A file someone else still holds open is not this test's problem.
        let _ = DirectoryRemoval::remove_tree(&self.root());
    }
}

fn bytes(count: usize) -> Vec<u8> {
    (0..count).map(|i| (i % 251) as u8).collect()
}

// --- the digests themselves ---

/// CRC-32 of "123456789" is CBF43926 — the check value every CRC-32 specification publishes.
///
/// This is the single most important fact in the checksum feature. The C# library wrote the
/// checksum little-endian and the bytes had to be reversed; here it is a `u32` that has to be
/// rendered big-endian. Get that wrong and every digest is still eight plausible hex characters,
/// every other test still passes, and every .sfv file this app writes is silently unverifiable
/// anywhere else.
#[test]
fn crc32_matches_the_known_vector() {
    let f = Fixture::new();
    let path = f.file(b"123456789");

    assert_eq!(
        "CBF43926",
        f.digest(&path, &[ChecksumAlgorithm::Crc32]).by_algorithm[&ChecksumAlgorithm::Crc32]
    );
}

#[test]
fn each_algorithm_agrees_with_the_bcl() {
    let f = Fixture::new();
    let content = bytes(5000);
    let path = f.file(&content);

    let digests = f
        .digest(
            &path,
            &[
                ChecksumAlgorithm::Md5,
                ChecksumAlgorithm::Sha1,
                ChecksumAlgorithm::Sha256,
                ChecksumAlgorithm::Sha512,
            ],
        )
        .by_algorithm;

    assert_eq!(
        to_hex_upper(&md5::Md5::digest(&content)),
        digests[&ChecksumAlgorithm::Md5]
    );
    assert_eq!(
        to_hex_upper(&sha1::Sha1::digest(&content)),
        digests[&ChecksumAlgorithm::Sha1]
    );
    assert_eq!(
        to_hex_upper(&sha2::Sha256::digest(&content)),
        digests[&ChecksumAlgorithm::Sha256]
    );
    assert_eq!(
        to_hex_upper(&sha2::Sha512::digest(&content)),
        digests[&ChecksumAlgorithm::Sha512]
    );
}

/// The one-pass claim: five at once must equal five separately.
#[test]
fn several_algorithms_in_one_pass_agree_with_one_each() {
    let f = Fixture::new();
    let path = f.file(&bytes(3_000_000));

    let together = f.digest(&path, &ChecksumAlgorithms::ALL).by_algorithm;

    for algorithm in ChecksumAlgorithms::ALL {
        assert_eq!(
            f.digest(&path, &[algorithm]).by_algorithm[&algorithm],
            together[&algorithm],
            "{algorithm:?}"
        );
    }
}

#[test]
fn every_digest_is_uppercase_hex_of_the_catalogue_length() {
    let f = Fixture::new();
    let path = f.file(&bytes(64));

    for (algorithm, digest) in f.digest(&path, &ChecksumAlgorithms::ALL).by_algorithm {
        assert_eq!(
            ChecksumAlgorithms::hex_length(algorithm),
            digest.len(),
            "{algorithm:?}"
        );
        assert_eq!(digest.to_uppercase(), digest, "{algorithm:?}");
        assert!(ChecksumAlgorithms::is_hex(&digest), "{algorithm:?}");
    }
}

#[test]
fn the_algorithms_asked_for_are_the_ones_returned() {
    let f = Fixture::new();
    let path = f.file(&bytes(10));

    let digests = f
        .digest(&path, &[ChecksumAlgorithm::Sha512, ChecksumAlgorithm::Md5])
        .by_algorithm;

    assert_eq!(2, digests.len());
    assert!(digests.contains_key(&ChecksumAlgorithm::Md5));
    assert!(digests.contains_key(&ChecksumAlgorithm::Sha512));
}

#[test]
fn asking_twice_for_one_algorithm_digests_it_once() {
    let f = Fixture::new();
    let path = f.file(&bytes(10));

    assert_eq!(
        1,
        f.digest(&path, &[ChecksumAlgorithm::Md5, ChecksumAlgorithm::Md5])
            .by_algorithm
            .len()
    );
}

/// An empty request is an empty answer, not a failure. A window with every box unticked has
/// nothing to say about a file, which is a different thing from being unable to read it.
#[test]
fn asking_for_no_algorithms_is_an_empty_result_not_null() {
    let f = Fixture::new();
    let path = f.file(&bytes(10));

    let result = f
        .digester
        .digest(&path, &[], None, &CancelToken::none())
        .unwrap();

    let result = result.expect("not None");
    assert!(result.by_algorithm.is_empty());
}

// --- progress ---

/// Progress counts bytes off the disk, not bytes hashed. Five algorithms over one megabyte is one
/// megabyte of progress — report it per sink and every multi-algorithm run shows a bar that runs
/// to five hundred percent.
#[test]
fn progress_reports_each_chunk_once_however_many_algorithms() {
    let f = Fixture::new();
    let content = bytes(2_500_000);
    let path = f.file(&content);

    let reported = AtomicI64::new(0);
    let add = |delta: i64| {
        reported.fetch_add(delta, Ordering::SeqCst);
    };
    f.digester
        .digest(
            &path,
            &ChecksumAlgorithms::ALL,
            Some(&add),
            &CancelToken::none(),
        )
        .unwrap();

    assert_eq!(content.len() as i64, reported.into_inner());
}

// --- the contract, restated because the loop is now shared ---

#[test]
fn a_missing_file_is_null_not_a_throw() {
    let f = Fixture::new();
    let missing = f.root.join("nope.bin").to_string_lossy().into_owned();

    assert_eq!(
        Ok(None),
        f.digester.digest(
            &missing,
            &[ChecksumAlgorithm::Sha256],
            None,
            &CancelToken::none()
        )
    );
}

#[test]
fn a_directory_is_null_not_a_throw() {
    let f = Fixture::new();

    assert_eq!(
        Ok(None),
        f.digester.digest(
            &f.root(),
            &[ChecksumAlgorithm::Sha256],
            None,
            &CancelToken::none()
        )
    );
}

#[test]
fn a_cancelled_token_throws_rather_than_returning_null() {
    let f = Fixture::new();
    let path = f.file(&bytes(64));
    let cancel = CancelToken::new();
    cancel.cancel();

    assert_eq!(
        Err(Cancelled),
        f.digester
            .digest(&path, &[ChecksumAlgorithm::Sha256], None, &cancel)
    );
}

/// The sharing rule, restated here because the read loop is now shared with the hasher: narrow
/// the flags to read-only sharing and this goes red alongside its sibling, which is exactly what
/// should happen.
#[test]
fn a_file_someone_else_has_open_for_writing_is_still_digestible() {
    let f = Fixture::new();
    let path = f.file(&bytes(128));

    let _holder = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&path)
        .unwrap();

    assert!(
        f.digester
            .digest(
                &path,
                &[ChecksumAlgorithm::Sha256],
                None,
                &CancelToken::none()
            )
            .unwrap()
            .is_some()
    );
}

#[test]
fn an_empty_file_digests_to_the_empty_digest() {
    let f = Fixture::new();
    let path = f.file(&[]);

    let digests = f.digest(
        &path,
        &[ChecksumAlgorithm::Sha256, ChecksumAlgorithm::Crc32],
    );

    assert_eq!(0, digests.bytes_read);
    assert_eq!(
        to_hex_upper(&sha2::Sha256::digest([])),
        digests.by_algorithm[&ChecksumAlgorithm::Sha256]
    );
    assert_eq!("00000000", digests.by_algorithm[&ChecksumAlgorithm::Crc32]);
}
