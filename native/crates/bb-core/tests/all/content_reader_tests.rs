// Port of tests/BertBrowser.Core.Tests/ContentReaderTests.cs. The one class in the content search
// that opens a file, tested against real ones: the sharing flags, the attributes that mean "do not
// read this", and the difference between a file that failed and a run that stopped.
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;

use bb_core::cancel::{CancelToken, Cancelled};
use bb_core::search::{
    ContentReader, ContentSearchRules, ContentText, FileSystemContentReader, FileText,
};
use bb_test_support::unique_temp_path;

const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;
const FILE_SHARE_DELETE: u32 = 4;

struct Fixture {
    root: PathBuf,
    reader: FileSystemContentReader,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bb-content", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            root,
            reader: FileSystemContentReader,
        }
    }

    fn write(&self, name: &str, content: &str) -> String {
        self.write_bytes(name, content.as_bytes())
    }

    fn write_bytes(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.root.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn read(&self, path: &str) -> Result<FileText, Cancelled> {
        self.read_max(path, ContentSearchRules::MAX_BYTES_PER_FILE)
    }

    fn read_max(&self, path: &str, max: u64) -> Result<FileText, Cancelled> {
        self.reader.read(path, max, &CancelToken::none())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn text(outcome: Result<FileText, Cancelled>) -> ContentText {
    match outcome {
        Ok(FileText::Text(t)) => t,
        other => panic!("expected text, got {other:?}"),
    }
}

// --- the ordinary cases ---

#[test]
fn a_plain_text_file_comes_back_whole() {
    let f = Fixture::new();
    let t = text(f.read(&f.write("a.txt", "hello TODO world")));
    assert_eq!(t.text, "hello TODO world");
    assert!(!t.truncated);
    assert!(t.index_of("todo").is_some());
}

#[test]
fn line_endings_are_normalised_so_line_numbers_mean_one_thing() {
    let f = Fixture::new();
    let t = text(f.read(&f.write("crlf.txt", "a\r\nb\r\nTODO")));
    assert!(!t.text.contains('\r'));
}

#[test]
fn a_file_longer_than_the_budget_is_read_to_it_and_says_so() {
    // The honesty that matters: a miss here is "not in the first N bytes", and the outcome reports
    // it rather than passing it off as "not in the file".
    let f = Fixture::new();
    let path = f.write("big.txt", &format!("{}TODO", "x".repeat(4_000)));
    let t = text(f.read_max(&path, 1_000));

    assert!(t.truncated);
    assert_eq!(t.text.chars().count(), 1_000);
    assert!(
        t.index_of("TODO").is_none(),
        "the needle was past the budget and must not be found"
    );
}

#[test]
fn a_file_exactly_the_budget_is_not_reported_truncated() {
    let f = Fixture::new();
    let t = text(f.read_max(&f.write("exact.txt", &"x".repeat(500)), 500));
    assert!(!t.truncated);
}

#[test]
fn an_empty_file_is_empty_text_rather_than_a_failure() {
    let f = Fixture::new();
    let t = text(f.read(&f.write("empty.txt", "")));
    assert_eq!(t.text, "");
}

#[test]
fn a_utf16_file_with_no_bom_is_still_read() {
    // The bomless-UTF-16 rung of the ladder, reached through the head sample. Decide the encoding
    // wrongly here and the whole file decodes as mojibake and matches nothing.
    let f = Fixture::new();
    let bytes: Vec<u8> = "hello TODO world"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let t = text(f.read(&f.write_bytes("utf16.txt", &bytes)));
    assert!(t.index_of("TODO").is_some());
}

#[test]
fn a_bom_is_not_part_of_the_text() {
    let f = Fixture::new();
    let t = text(f.read(&f.write_bytes("bom.txt", b"\xEF\xBB\xBFTODO")));
    assert_eq!(t.text, "TODO");
}

// --- not text ---

#[test]
fn a_binary_file_is_nothing_to_search_rather_than_a_failure() {
    // The distinction the outcome rests on. A folder of images must not report itself
    // "incomplete": nothing went wrong, those files simply have no text.
    let f = Fixture::new();
    let bytes: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
    let outcome = f.read(&f.write_bytes("opaque.bin", &bytes));

    assert!(matches!(outcome, Ok(FileText::NotText)), "{outcome:?}");
}

#[test]
fn a_binary_file_does_not_match_even_when_its_bytes_spell_the_needle() {
    // A .exe with "TODO" in its string table is not a text hit, and reporting it as one would fill
    // a source-tree search with noise.
    let f = Fixture::new();
    let mut bytes = vec![0u8; 4096];
    bytes[100..104].copy_from_slice(b"TODO");
    let outcome = f.read(&f.write_bytes("withtext.bin", &bytes));

    assert!(matches!(outcome, Ok(FileText::NotText)), "{outcome:?}");
}

// --- refusals and failures ---

#[test]
fn a_missing_file_is_a_failure_rather_than_empty_text() {
    let f = Fixture::new();
    let outcome = f.read(&f.root.join("not-here.txt").to_string_lossy());
    assert!(matches!(outcome, Ok(FileText::Unreadable)), "{outcome:?}");
}

#[test]
fn a_cancel_throws_rather_than_returning_null() {
    // Load-bearing: Unreadable means "this file had a problem, carry on", Err means "the run is
    // stopping". Return Unreadable here and a cancelled search looks like a disk full of broken
    // files, which is what the outcome's two separate flags exist to keep apart.
    let f = Fixture::new();
    let path = f.write("a.txt", "hello");
    let cancel = CancelToken::new();
    cancel.cancel();

    let outcome = f
        .reader
        .read(&path, ContentSearchRules::MAX_BYTES_PER_FILE, &cancel);
    assert_eq!(outcome.err(), Some(Cancelled));
}

#[test]
fn reading_never_blocks_a_rename_or_a_delete() {
    // The house rule, and the one thing no in-memory test could catch: a held handle would block
    // this app's own executors in the very folder being searched. Narrow the share flags to
    // FILE_SHARE_READ and this goes red.
    let f = Fixture::new();
    let path = f.write("held.txt", "TODO");

    let _holder = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&path)
        .unwrap();
    let t = f.read(&path);
    assert!(matches!(t, Ok(FileText::Text(_))), "{t:?}");

    // And the other direction: while we would be reading it, it can still be renamed.
    let moved = f.root.join("held-renamed.txt");
    std::fs::rename(&path, &moved).unwrap();
    assert!(moved.exists());
}

#[test]
fn a_file_opened_for_writing_elsewhere_is_still_searchable() {
    let f = Fixture::new();
    let path = f.write("log.txt", "TODO in a live log");

    let _writer = OpenOptions::new()
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&path)
        .unwrap();
    let t = text(f.read(&path));

    assert!(t.index_of("TODO").is_some());
}
