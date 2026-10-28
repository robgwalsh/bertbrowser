// Port of tests/BertBrowser.Core.Tests/FileContentComparerTests.cs
use std::cell::Cell;
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;

use bb_core::cancel::{CancelToken, Cancelled};
use bb_core::compare::{ContentComparer, ContentComparison, ContentVerdict, FileContentComparer};
use bb_test_support::unique_temp_path;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

struct Fixture {
    root: PathBuf,
    comparer: FileContentComparer,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-content", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            root,
            comparer: FileContentComparer,
        }
    }

    fn file(&self, content: &[u8], name: &str) -> String {
        let path = self.root.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn compare(&self, a: &str, b: &str) -> ContentComparison {
        self.comparer
            .compare(a, b, None, &CancelToken::none())
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A file someone else still holds open is not this test's problem.
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn bytes(count: usize) -> Vec<u8> {
    (0..count).map(|i| (i % 251) as u8).collect()
}

// --- the verdicts ---

#[test]
fn two_identical_files_are_identical() {
    let f = Fixture::new();
    let content = bytes(5000);
    let result = f.compare(&f.file(&content, "a.bin"), &f.file(&content, "b.bin"));

    assert_eq!(ContentVerdict::Identical, result.verdict);
    assert_eq!(None, result.first_difference_offset);
    assert_eq!(5000, result.left_bytes);
}

#[test]
fn two_empty_files_are_identical() {
    let f = Fixture::new();
    assert_eq!(
        ContentVerdict::Identical,
        f.compare(&f.file(&[], "a.bin"), &f.file(&[], "b.bin"))
            .verdict
    );
}

#[test]
fn a_one_byte_change_reports_that_exact_offset() {
    let f = Fixture::new();
    let left = bytes(5000);
    let mut right = bytes(5000);
    right[1234] ^= 0xFF;

    let result = f.compare(&f.file(&left, "a.bin"), &f.file(&right, "b.bin"));

    assert_eq!(ContentVerdict::Differs, result.verdict);
    assert_eq!(Some(1234), result.first_difference_offset);
}

/// The offset must be exact across a buffer boundary too: narrowing to the byte happens inside a
/// chunk, and an off-by-one there would point at the wrong place in a large file.
#[test]
fn a_difference_past_the_first_buffer_reports_its_absolute_offset() {
    let f = Fixture::new();
    let left = bytes(3_000_000);
    let mut right = left.clone();
    right[2_500_000] ^= 0xFF;

    assert_eq!(
        Some(2_500_000),
        f.compare(&f.file(&left, "a.bin"), &f.file(&right, "b.bin"))
            .first_difference_offset
    );
}

/// "Differs at byte 4,096" is a more useful answer than "the sizes differ": it says the shorter
/// one is a prefix, which is what a truncated download looks like.
#[test]
fn a_prefix_reports_where_the_shorter_one_ended() {
    let f = Fixture::new();
    let longer = bytes(6000);
    let shorter = &longer[..4096];

    let result = f.compare(&f.file(&longer, "a.bin"), &f.file(shorter, "b.bin"));

    assert_eq!(ContentVerdict::Differs, result.verdict);
    assert_eq!(Some(4096), result.first_difference_offset);
    assert_eq!(6000, result.left_bytes);
    assert_eq!(4096, result.right_bytes);
}

#[test]
fn an_empty_file_against_a_non_empty_one_differs_at_zero() {
    let f = Fixture::new();
    let result = f.compare(&f.file(&[], "a.bin"), &f.file(&bytes(10), "b.bin"));

    assert_eq!(ContentVerdict::Differs, result.verdict);
    assert_eq!(Some(0), result.first_difference_offset);
}

#[test]
fn files_differing_only_in_their_last_byte_are_caught() {
    let f = Fixture::new();
    let left = bytes(1000);
    let mut right = left.clone();
    *right.last_mut().unwrap() ^= 0xFF;

    assert_eq!(
        Some(999),
        f.compare(&f.file(&left, "a.bin"), &f.file(&right, "b.bin"))
            .first_difference_offset
    );
}

// --- the rule that matters ---

/// The whole safety argument. A comparison's "same" is what lets a sync delete something, so a
/// side that could not be read must never come back as a match.
#[test]
fn an_unreadable_side_is_never_identical() {
    let f = Fixture::new();
    let real = f.file(&bytes(10), "a.bin");
    let missing = f.root.join("nope.bin").to_string_lossy().into_owned();

    assert_eq!(
        ContentVerdict::Unreadable,
        f.compare(&missing, &real).verdict
    );
    assert_eq!(
        ContentVerdict::Unreadable,
        f.compare(&real, &missing).verdict
    );
}

#[test]
fn a_directory_is_unreadable_not_a_throw() {
    let f = Fixture::new();
    assert_eq!(
        ContentVerdict::Unreadable,
        f.compare(&f.root(), &f.file(&bytes(10), "a.bin")).verdict
    );
}

/// The sharing rule again: this app must be able to compare a file it is itself writing, or a
/// comparison would block the very operations it exists to check.
#[test]
fn a_file_someone_else_has_open_for_writing_is_still_comparable() {
    let f = Fixture::new();
    let content = bytes(500);
    let a = f.file(&content, "a.bin");
    let b = f.file(&content, "b.bin");

    let _holder = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&a)
        .unwrap();

    assert_eq!(ContentVerdict::Identical, f.compare(&a, &b).verdict);
}

// --- the short circuit ---

/// The same path twice is the same file, which a handle already knows. Reading both sides to
/// confirm it would be minutes spent learning what Windows said immediately, and the progress
/// callback never firing is how this test can tell the difference.
#[test]
fn the_path_compared_with_itself_is_identical_without_reading() {
    let f = Fixture::new();
    let path = f.file(&bytes(2_000_000), "a.bin");

    let read = Cell::new(0i64);
    let result = f
        .comparer
        .compare(
            &path,
            &path,
            Some(&|delta| read.set(read.get() + delta)),
            &CancelToken::none(),
        )
        .unwrap();

    assert_eq!(ContentVerdict::Identical, result.verdict);
    assert_eq!(0, read.get());
}

// --- progress and cancellation ---

#[test]
fn progress_adds_up_to_what_was_read() {
    let f = Fixture::new();
    let content = bytes(2_500_000);
    let a = f.file(&content, "a.bin");
    let b = f.file(&content, "b.bin");

    let read = Cell::new(0i64);
    f.comparer
        .compare(
            &a,
            &b,
            Some(&|delta| read.set(read.get() + delta)),
            &CancelToken::none(),
        )
        .unwrap();

    assert_eq!(content.len() as i64, read.get());
}

#[test]
fn a_cancelled_token_throws() {
    let f = Fixture::new();
    let content = bytes(100);
    let a = f.file(&content, "a.bin");
    let b = f.file(&content, "b.bin");

    let cancel = CancelToken::new();
    cancel.cancel();

    assert_eq!(Err(Cancelled), f.comparer.compare(&a, &b, None, &cancel));
}
