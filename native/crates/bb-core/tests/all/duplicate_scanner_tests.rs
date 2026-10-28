// Port of tests/BertBrowser.Core.Tests/DuplicateScannerTests.cs
//
// The three passes, against a fake index and a fake hasher. Both seams exist for this: contriving
// two files that share their first 64 KB and differ after it — or three names for one file on
// disk — takes a line here and a privileged filesystem layout otherwise.
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use bb_core::FileTime;
use bb_core::cancel::{CancelToken, Cancelled};
use bb_core::checksums::{ByteProgress, to_hex_upper};
use bb_core::data::{self, SearchHit};
use bb_core::duplicates::{
    DuplicateCandidateSource, DuplicateProgressSink, DuplicateRules, DuplicateScanAvailability,
    DuplicateScanOutcome, DuplicateScanPhase, DuplicateScanProgress, DuplicateScanRequest,
    DuplicateScanner, DuplicateShortlist, FileFingerprint, FileHasher, FileIdentity,
};
use bb_paths::fold_upper;
use sha2::Digest;

const HEAD: i64 = DuplicateRules::HEAD_SAMPLE_BYTES;

fn anywhere() -> DuplicateScanRequest {
    DuplicateScanRequest {
        root_path: None,
        min_size_bytes: 1,
        include_hidden: true,
        skip_system_folders: false,
    }
}

/// Content of `length` bytes filled from `seed`, so two files agree exactly as far as their seeds
/// do.
fn content(length: usize, seed: &str) -> Vec<u8> {
    let pattern = seed.as_bytes();
    (0..length).map(|i| pattern[i % pattern.len()]).collect()
}

/// Content that agrees with `seed` for the whole sample and then diverges — the case the second
/// hashing pass exists for.
fn same_head(length: usize, seed: &str, tail: &str) -> Vec<u8> {
    let mut bytes = content(length, seed);
    for (i, &b) in tail.as_bytes().iter().enumerate() {
        bytes[HEAD as usize + i] = b;
    }
    bytes
}

type Files = Vec<(&'static str, Vec<u8>)>;
type AfterChunk = Box<dyn Fn(&str, i64) + Sync + Send>;

struct Build {
    source: FakeCandidateSource,
    hasher: FakeHasher,
}

fn build(
    files: Files,
    identities: Option<Vec<(&'static str, FileIdentity)>>,
    unreadable: Option<Vec<&'static str>>,
    after_chunk: Option<AfterChunk>,
) -> Build {
    let map: HashMap<String, Vec<u8>> = files
        .iter()
        .map(|(p, c)| (p.to_string(), c.clone()))
        .collect();
    Build {
        source: FakeCandidateSource::new(&map),
        hasher: FakeHasher::new(map, identities, unreadable, after_chunk),
    }
}

fn scan(b: &Build, is_building: bool, is_indexed: bool) -> DuplicateScanOutcome {
    DuplicateScanner::new(&b.source, &b.hasher)
        .scan(
            &anywhere(),
            is_building,
            is_indexed,
            &CancelToken::none(),
            None,
        )
        .unwrap()
}

fn single<T>(items: &[T]) -> &T {
    assert_eq!(1, items.len(), "expected exactly one item");
    &items[0]
}

// --- the ordinary cases ---

#[test]
fn identical_files_are_a_group() {
    let b = build(
        vec![
            (r"C:\a\report.pdf", content(4096, "same")),
            (r"C:\b\report.pdf", content(4096, "same")),
        ],
        None,
        None,
        None,
    );

    let outcome = scan(&b, false, true);

    let group = single(&outcome.groups);
    assert_eq!(2, group.files.len());
    assert_eq!(4096, group.size_bytes);
    assert_eq!(4096, group.wasted_bytes());
}

/// The shortlist is a size collision and nothing more. Make the sampling pass accept on size
/// alone and this goes red — which is the bug that would offer to delete two unrelated files that
/// happen to be the same length.
#[test]
fn same_size_different_content_is_not_a_group() {
    let b = build(
        vec![
            (r"C:\a\one.bin", content(4096, "aaaa")),
            (r"C:\b\two.bin", content(4096, "bbbb")),
        ],
        None,
        None,
        None,
    );

    let outcome = scan(&b, false, true);

    assert!(outcome.groups.is_empty());
}

/// The whole reason there is a third pass. These two agree for every byte the sample reads and
/// differ immediately after it — a disk image, a container with a shared header, a VM file. Drop
/// the full hash and this goes red.
#[test]
fn same_head_different_tail_is_not_a_group() {
    let length = HEAD as usize + 4096;
    let b = build(
        vec![
            (r"C:\a\disk.img", same_head(length, "same", "LEFT")),
            (r"C:\b\disk.img", same_head(length, "same", "RIGHT")),
        ],
        None,
        None,
        None,
    );

    let outcome = scan(&b, false, true);

    assert!(outcome.groups.is_empty());

    // And it really did have to read them whole to find out.
    assert_eq!(2, b.hasher.calls().iter().filter(|c| c.1 == 0).count());
}

/// A file the sample read to its end was hashed in full by it, so asking again would read every
/// small file on the shortlist twice for nothing.
#[test]
fn files_smaller_than_the_sample_are_never_read_twice() {
    let b = build(
        vec![
            (r"C:\a\note.txt", content(2048, "same")),
            (r"C:\b\note.txt", content(2048, "same")),
        ],
        None,
        None,
        None,
    );

    let outcome = scan(&b, false, true);

    assert_eq!(1, outcome.groups.len());
    assert!(b.hasher.calls().iter().all(|c| c.1 != 0));
}

/// A row can be stale: the file shrank after it was indexed. Grouping on the indexed length would
/// compare it against one that did not, so the real byte count read is what counts.
#[test]
fn groups_carry_the_length_actually_read_not_the_indexed_one() {
    let b = build(
        vec![
            (r"C:\a\x.bin", content(1024, "same")),
            (r"C:\b\x.bin", content(1024, "same")),
        ],
        None,
        None,
        None,
    );

    // The index still thinks both are 9999 bytes.
    let outcome = scan(&b, false, true);

    let group = single(&outcome.groups);
    assert_eq!(1024, group.size_bytes);
    assert!(group.files.iter().all(|f| f.size_bytes == 1024));
}

// --- hardlinks ---

/// Two names for one file are not two copies: deleting one frees nothing. They fold into a single
/// entry that carries the other name.
#[test]
fn hardlinked_copies_are_one_entry() {
    let identity = FileIdentity {
        volume_serial_number: 1,
        file_index: 42,
    };
    let b = build(
        vec![
            (r"C:\a\x.bin", content(4096, "same")),
            (r"C:\b\x.bin", content(4096, "same")),
            (r"C:\c\x.bin", content(4096, "same")),
        ],
        Some(vec![(r"C:\a\x.bin", identity), (r"C:\b\x.bin", identity)]),
        None,
        None,
    );

    let outcome = scan(&b, false, true);

    let group = single(&outcome.groups);
    assert_eq!(2, group.files.len());

    let folded: Vec<_> = group
        .files
        .iter()
        .filter(|f| !f.hardlink_paths.is_empty())
        .collect();
    let folded = single(&folded);
    assert_eq!(r"C:\a\x.bin", folded.display_path);
    assert_eq!(vec![r"C:\b\x.bin".to_string()], folded.hardlink_paths);

    // And the waste is one redundant copy, not two.
    assert_eq!(4096, group.wasted_bytes());
}

/// C:\Windows\WinSxS is built almost entirely this way. Every "copy" being one file means there
/// is nothing to reclaim and nothing to show — reporting the group would bury every real result
/// under millions of rows that free nothing.
#[test]
fn when_every_copy_is_one_file_there_is_no_group() {
    let identity = FileIdentity {
        volume_serial_number: 1,
        file_index: 7,
    };
    let b = build(
        vec![
            (r"C:\w\a.dll", content(4096, "same")),
            (r"C:\w\b.dll", content(4096, "same")),
            (r"C:\w\c.dll", content(4096, "same")),
        ],
        Some(vec![
            (r"C:\w\a.dll", identity),
            (r"C:\w\b.dll", identity),
            (r"C:\w\c.dll", identity),
        ]),
        None,
        None,
    );

    let outcome = scan(&b, false, true);

    assert!(outcome.groups.is_empty());
}

// --- availability ---

/// The sizeless build path writes every row with size_bytes = 0, so every file collides with
/// every other. Trusting that would mean reading an entire disk to discover nothing. Make
/// classify return Ready for this shape and the assertion on the hasher goes red.
#[test]
fn no_size_data_hashes_nothing_at_all() {
    let hasher = FakeHasher::new(HashMap::new(), None, None, None);
    let mut source = FakeCandidateSource::new(&HashMap::new());
    source.files_in_scope = 5000;
    source.sized_files_in_scope = 0;

    let outcome = DuplicateScanner::new(&source, &hasher)
        .scan(&anywhere(), false, true, &CancelToken::none(), None)
        .unwrap();

    assert_eq!(DuplicateScanAvailability::NoSizeData, outcome.availability);
    assert!(outcome.groups.is_empty());
    assert!(hasher.calls().is_empty());
}

#[test]
fn not_indexed_hashes_nothing_at_all() {
    let hasher = FakeHasher::new(HashMap::new(), None, None, None);
    let source = FakeCandidateSource::new(&HashMap::new());

    let outcome = DuplicateScanner::new(&source, &hasher)
        .scan(&anywhere(), false, false, &CancelToken::none(), None)
        .unwrap();

    assert_eq!(DuplicateScanAvailability::NotIndexed, outcome.availability);
    assert!(hasher.calls().is_empty());
}

// --- failures ---

/// One file's failure never costs the others — the rule every executor in this app holds to. The
/// result is a floor and says so.
#[test]
fn an_unreadable_candidate_is_skipped_and_said_so() {
    let b = build(
        vec![
            (r"C:\a\x.bin", content(4096, "same")),
            (r"C:\b\x.bin", content(4096, "same")),
            (r"C:\c\x.bin", content(4096, "same")),
        ],
        None,
        Some(vec![r"C:\c\x.bin"]),
        None,
    );

    let outcome = scan(&b, false, true);

    assert!(outcome.incomplete);
    let group = single(&outcome.groups);
    assert_eq!(2, group.files.len());
}

// --- cancellation ---

/// A cancel during sampling must not leave a group behind whose members only ever agreed on their
/// first 64 KB. These two would be exactly that: same head, different tail.
#[test]
fn cancel_during_sampling_reports_nothing_it_had_not_confirmed() {
    let length = HEAD as usize + 4096;
    let cancel = CancelToken::new();

    let after = {
        let cancel = cancel.clone();
        Box::new(move |_: &str, _: i64| cancel.cancel()) as AfterChunk
    };
    let b = build(
        vec![
            (r"C:\a\disk.img", same_head(length, "same", "LEFT")),
            (r"C:\b\disk.img", same_head(length, "same", "RIGHT")),
        ],
        None,
        None,
        Some(after),
    );

    let outcome = DuplicateScanner::new(&b.source, &b.hasher)
        .scan(&anywhere(), false, true, &cancel, None)
        .unwrap();

    assert!(outcome.cancelled);
    assert!(outcome.groups.is_empty());
}

/// A cancel during the full pass keeps what the sample had already settled: those files were read
/// end to end, so their verdict is not provisional. Handing back nothing would throw away work
/// that is genuinely finished.
#[test]
fn cancel_during_the_full_pass_keeps_what_the_sample_settled() {
    let large = HEAD as usize + 8192;
    let cancel = CancelToken::new();

    // Only a full read ever gets past the sample's length.
    let after = {
        let cancel = cancel.clone();
        Box::new(move |_: &str, so_far: i64| {
            if so_far > HEAD {
                cancel.cancel();
            }
        }) as AfterChunk
    };
    let b = build(
        vec![
            // Settled by sampling: read to the end, so already final.
            (r"C:\a\note.txt", content(2048, "small")),
            (r"C:\b\note.txt", content(2048, "small")),
            // Only ever sampled; the full pass is what gets cancelled.
            (r"C:\a\disk.img", same_head(large, "same", "LEFT")),
            (r"C:\b\disk.img", same_head(large, "same", "RIGHT")),
        ],
        None,
        None,
        Some(after),
    );

    let outcome = DuplicateScanner::new(&b.source, &b.hasher)
        .scan(&anywhere(), false, true, &cancel, None)
        .unwrap();

    assert!(outcome.cancelled);
    let group = single(&outcome.groups);
    assert_eq!(2048, group.size_bytes);
}

// --- progress ---

#[test]
fn progress_walks_through_the_phases() {
    let length = HEAD as usize + 4096;
    let reports: Mutex<Vec<DuplicateScanProgress>> = Mutex::new(Vec::new());
    let sink = |p: DuplicateScanProgress| reports.lock().unwrap().push(p);
    let progress: DuplicateProgressSink<'_> = Some(&sink);

    let b = build(
        vec![
            (r"C:\a\disk.img", content(length, "same")),
            (r"C:\b\disk.img", content(length, "same")),
        ],
        None,
        None,
        None,
    );

    DuplicateScanner::new(&b.source, &b.hasher)
        .scan(&anywhere(), false, true, &CancelToken::none(), progress)
        .unwrap();

    let reports = reports.into_inner().unwrap();
    let phases: HashSet<DuplicateScanPhase> = reports.iter().map(|r| r.phase).collect();
    assert!(phases.contains(&DuplicateScanPhase::Shortlisting));
    assert!(phases.contains(&DuplicateScanPhase::Sampling));
    assert!(phases.contains(&DuplicateScanPhase::Hashing));

    // Sampling's total is exact and reads only the head of each file; the full pass reads both
    // whole. A bar with no denominator is what an indeterminate one is for, and neither of these
    // needs to be one.
    let sampling = reports
        .iter()
        .rfind(|r| r.phase == DuplicateScanPhase::Sampling)
        .unwrap();
    assert_eq!(HEAD * 2, sampling.bytes_total);

    let hashing = reports
        .iter()
        .rfind(|r| r.phase == DuplicateScanPhase::Hashing)
        .unwrap();
    assert_eq!(length as i64 * 2, hashing.bytes_total);
}

// --- ordering ---

/// Biggest saving first: the list is read top-down and the point of it is space.
#[test]
fn groups_come_back_worst_first() {
    let b = build(
        vec![
            (r"C:\a\small.bin", content(1024, "s")),
            (r"C:\b\small.bin", content(1024, "s")),
            (r"C:\a\big.bin", content(8192, "b")),
            (r"C:\b\big.bin", content(8192, "b")),
        ],
        None,
        None,
        None,
    );

    let outcome = scan(&b, false, true);

    assert_eq!(2, outcome.groups.len());
    assert_eq!(8192, outcome.groups[0].size_bytes);
    assert_eq!(9216, outcome.wasted_bytes());
}

// --- meta ---

/// Guards the guard. If the fake hasher were to answer the same digest for different bytes, every
/// separation test above would pass for the wrong reason.
#[test]
fn the_content_check_actually_separates_files_differing_by_one_byte() {
    let left = content(4096, "same");
    let mut right = content(4096, "same");
    right[4095] ^= 0xFF;

    let b = build(
        vec![(r"C:\a\x.bin", left), (r"C:\b\x.bin", right)],
        None,
        None,
        None,
    );

    assert!(scan(&b, false, true).groups.is_empty());
}

/// An in-memory index: the files it knows about, and — like the real repository — only the ones
/// sharing a byte length with another. The two scope counts default to describing a healthy index
/// and are settable for the availability cases.
struct FakeCandidateSource {
    files: Vec<(String, usize)>,
    files_in_scope: i64,
    sized_files_in_scope: i64,
}

impl FakeCandidateSource {
    fn new(files: &HashMap<String, Vec<u8>>) -> FakeCandidateSource {
        FakeCandidateSource {
            files: files.iter().map(|(p, c)| (p.clone(), c.len())).collect(),
            files_in_scope: files.len() as i64,
            sized_files_in_scope: files.len() as i64,
        }
    }
}

impl DuplicateCandidateSource for FakeCandidateSource {
    fn shortlist(
        &self,
        _request: &DuplicateScanRequest,
        _cancel: &CancelToken,
    ) -> data::Result<DuplicateShortlist> {
        let mut by_length: HashMap<usize, usize> = HashMap::new();
        for (_, len) in &self.files {
            *by_length.entry(*len).or_default() += 1;
        }

        let mut candidates: Vec<SearchHit> = self
            .files
            .iter()
            .filter(|(_, len)| by_length[len] > 1)
            .map(|(path, len)| {
                let (dir, name) = bb_paths::split_last(path);
                SearchHit {
                    display_path: path.clone(),
                    relative_dir_display: dir.to_string(),
                    name: name.to_string(),
                    is_directory: false,
                    size_bytes: *len as i64,
                    modified_utc: FileTime::from_ymd_hms(2024, 3, 14, 9, 26, 53),
                    hidden: false,
                    attributes: 0,
                    created_utc: FileTime::UNKNOWN,
                    content_match: None,
                }
            })
            .collect();
        candidates.sort_by(|a, b| a.display_path.cmp(&b.display_path));

        Ok(DuplicateShortlist {
            files: candidates,
            files_in_scope: self.files_in_scope,
            sized_files_in_scope: self.sized_files_in_scope,
        })
    }
}

/// A hasher that really hashes, but of bytes held in memory and in a fixed number of chunks with
/// a hook between them — so a cancel can be made to land in the middle of a named file every
/// time, which is what a real multi-gigabyte fixture would otherwise be needed for.
struct FakeHasher {
    files: HashMap<String, Vec<u8>>,
    identities: HashMap<String, FileIdentity>,
    unreadable: HashSet<String>,
    after_chunk: Option<AfterChunk>,
    calls: Mutex<Vec<(String, i64)>>,
}

impl FakeHasher {
    const CHUNK_BYTES: i64 = 4096;

    fn new(
        files: HashMap<String, Vec<u8>>,
        identities: Option<Vec<(&'static str, FileIdentity)>>,
        unreadable: Option<Vec<&'static str>>,
        after_chunk: Option<AfterChunk>,
    ) -> FakeHasher {
        FakeHasher {
            files,
            identities: identities
                .unwrap_or_default()
                .into_iter()
                .map(|(p, id)| (p.to_string(), id))
                .collect(),
            unreadable: unreadable
                .unwrap_or_default()
                .into_iter()
                .map(fold_upper)
                .collect(),
            after_chunk,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<(String, i64)> {
        self.calls.lock().unwrap().clone()
    }
}

impl FileHasher for FakeHasher {
    fn hash(
        &self,
        path: &str,
        max_bytes: i64,
        progress: ByteProgress<'_>,
        cancel: &CancelToken,
    ) -> Result<Option<FileFingerprint>, Cancelled> {
        cancel.check()?;
        self.calls
            .lock()
            .unwrap()
            .push((path.to_string(), max_bytes));

        if self.unreadable.contains(&fold_upper(path)) {
            return Ok(None);
        }
        let Some(content) = self.files.get(path) else {
            return Ok(None);
        };

        let limit = if max_bytes > 0 {
            max_bytes.min(content.len() as i64)
        } else {
            content.len() as i64
        };

        let mut read = 0i64;
        while read < limit {
            cancel.check()?;

            let chunk = Self::CHUNK_BYTES.min(limit - read);
            read += chunk;
            if let Some(report) = progress {
                report(chunk);
            }
            if let Some(after) = &self.after_chunk {
                after(path, read);
            }
        }

        let hash = to_hex_upper(&sha2::Sha256::digest(&content[..limit as usize]));
        Ok(Some(FileFingerprint {
            hash,
            bytes_read: limit,
            identity: self.identities.get(path).copied(),
        }))
    }
}
