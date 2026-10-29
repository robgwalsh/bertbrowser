//! Finding files that are byte-for-byte the same, in three passes that each cost far less than
//! the one after it. Spec: Core/Services/Duplicates/DuplicateScanner.cs.
//!
//! **The shortlist is free.** Two files of different lengths cannot be duplicates, and the MFT
//! index already holds a length for every file on every fixed volume — so the pass that would
//! otherwise mean walking millions of directory entries is a query. Only the collisions are ever
//! opened, and only the survivors of a 64 KB sample are ever read in full.
//!
//! **Cancelling gives back a floor, not nothing.** Whatever grouped before the stop is really
//! grouped — those files were read and their hashes really do match — so it is reported, with
//! `cancelled` saying the list is short. The one thing that must not happen is emitting a group
//! whose members only agreed on their *first 64 KB*, so a cancel during sampling reports only the
//! files small enough to have been hashed in full by it.
//!
//! Synchronous, like the planners and executors: `DuplicateFinder` is the facade that keeps view
//! models off it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::{
    DuplicateCandidateSource, DuplicateFile, DuplicateGroup, DuplicateRules,
    DuplicateScanAvailability, DuplicateScanOutcome, DuplicateScanPhase, DuplicateScanProgress,
    DuplicateScanRequest, FileHasher, FileIdentity,
};
use crate::cancel::CancelToken;
use crate::data::{self, Error, SearchHit};

/// A sink for progress, called from whichever thread reports.
pub type DuplicateProgressSink<'a> = Option<&'a (dyn Fn(DuplicateScanProgress) + Sync)>;

pub struct DuplicateScanner<'a> {
    candidates: &'a dyn DuplicateCandidateSource,
    hasher: &'a dyn FileHasher,
}

impl<'a> DuplicateScanner<'a> {
    /// How many files are read at once. SHA-256 is hardware-accelerated at a gigabyte or two a
    /// second per core, which is slower than an NVMe disk can feed one thread — so this is worth
    /// having. It stays small because the other case is a spinning disk, where too many concurrent
    /// readers turn one sequential pass into seek thrashing. Candidates arrive in path order (the
    /// index is clustered on it) and are consumed in that order, which keeps each worker's reads
    /// local.
    pub const MAX_PARALLELISM: usize = 4;

    pub fn new(
        candidates: &'a dyn DuplicateCandidateSource,
        hasher: &'a dyn FileHasher,
    ) -> DuplicateScanner<'a> {
        DuplicateScanner { candidates, hasher }
    }

    /// `is_building`: whether any volume's initial enumeration is still running; `is_indexed`:
    /// whether this scope's volume reports a complete live index. The two index facts are passed
    /// in rather than resolved here, exactly as the disk-usage service passes them to its rules:
    /// it keeps this type free of the index service and lets every availability case be tested by
    /// handing it a pair of booleans. `Err` only for a shortlist the index could not produce (a
    /// database failure); a cancel is an outcome, not an error.
    pub fn scan(
        &self,
        request: &DuplicateScanRequest,
        is_building: bool,
        is_indexed: bool,
        cancel: &CancelToken,
        progress: DuplicateProgressSink<'_>,
    ) -> data::Result<DuplicateScanOutcome> {
        let run = Run::new(progress);

        run.begin_phase(DuplicateScanPhase::Shortlisting, 0, 0);

        let shortlist = match self.candidates.shortlist(request, cancel) {
            Ok(s) => s,
            Err(Error::Cancelled) => {
                return Ok(DuplicateScanOutcome {
                    cancelled: true,
                    ..DuplicateScanOutcome::empty(DuplicateScanAvailability::Ready)
                });
            }
            Err(e) => return Err(e),
        };

        let availability = DuplicateRules::classify(
            shortlist.files_in_scope,
            shortlist.sized_files_in_scope,
            is_building,
            is_indexed,
        );

        // Rows that cannot be believed are not a result set, they are the absence of one. Handing
        // them back would leave the caller re-deciding whether to hash a whole disk for nothing.
        if matches!(
            availability,
            DuplicateScanAvailability::NotIndexed | DuplicateScanAvailability::NoSizeData
        ) {
            return Ok(DuplicateScanOutcome::empty(availability));
        }

        if shortlist.files.is_empty() {
            return Ok(DuplicateScanOutcome::empty(availability));
        }

        let mut incomplete = 0usize;
        let mut cancelled = false;

        // --- pass two: the first 64 KB of every candidate ---
        let sample_bytes: i64 = shortlist
            .files
            .iter()
            .map(|f| f.size_bytes.min(DuplicateRules::HEAD_SAMPLE_BYTES))
            .sum();
        run.begin_phase(
            DuplicateScanPhase::Sampling,
            shortlist.files.len(),
            sample_bytes,
        );

        let samples = self.hash_all(
            &shortlist.files,
            DuplicateRules::HEAD_SAMPLE_BYTES,
            &run,
            &mut incomplete,
            cancel,
        );
        cancelled |= samples.cancelled;

        // A file the sample read to its end was hashed in full, so its verdict is already final
        // and survives a cancel. One exactly the sample's length is not: there may be more after
        // it.
        let (settled, provisional): (Vec<Sample>, Vec<Sample>) = samples
            .samples
            .into_iter()
            .partition(|s| s.bytes_read < DuplicateRules::HEAD_SAMPLE_BYTES);

        let mut groups: Vec<DuplicateGroup> = Self::confirm(settled);

        // --- pass three: the survivors, in full ---
        if !cancelled {
            // Only files whose head still collides with another's are worth reading whole.
            let mut by_head: HashMap<(i64, &str), Vec<&Sample>> = HashMap::new();
            for sample in &provisional {
                by_head
                    .entry((sample.file.size_bytes, sample.hash.as_str()))
                    .or_default()
                    .push(sample);
            }
            let mut contenders: Vec<SearchHit> = by_head
                .into_values()
                .filter(|g| g.len() > 1)
                .flatten()
                .map(|s| s.file.clone())
                .collect();
            contenders.sort_by(|a, b| a.display_path.cmp(&b.display_path));

            if !contenders.is_empty() {
                let total: i64 = contenders.iter().map(|f| f.size_bytes).sum();
                run.begin_phase(DuplicateScanPhase::Hashing, contenders.len(), total);

                let full = self.hash_all(&contenders, 0, &run, &mut incomplete, cancel);
                cancelled |= full.cancelled;
                groups.extend(Self::confirm(full.samples));
            }
        }

        run.finished();

        // Biggest saving first; the tie-break on the first path keeps two runs identical.
        groups.sort_by(|a, b| {
            b.wasted_bytes().cmp(&a.wasted_bytes()).then_with(|| {
                a.files
                    .first()
                    .map(|f| f.display_path.as_str())
                    .cmp(&b.files.first().map(|f| f.display_path.as_str()))
            })
        });

        Ok(DuplicateScanOutcome {
            groups,
            availability,
            cancelled,
            incomplete: incomplete > 0,
            files_hashed: run.files_hashed(),
            bytes_hashed: run.total_bytes_hashed(),
        })
    }

    /// Hashes every file, bounded-parallel. `cancelled` is true when the run was cancelled
    /// part-way — whatever landed in `samples` before that is still good.
    fn hash_all(
        &self,
        files: &[SearchHit],
        max_bytes: i64,
        run: &Run<'_>,
        incomplete: &mut usize,
        cancel: &CancelToken,
    ) -> HashedBatch {
        let failures = AtomicUsize::new(0);
        let into: Mutex<Vec<Sample>> = Mutex::new(Vec::new());
        let stopped = AtomicBool::new(false);
        let next = AtomicUsize::new(0);

        // Bound once rather than per file: the hasher takes a callback and would otherwise
        // allocate a closure for every candidate, of which there can be hundreds of thousands.
        let add_bytes = |delta: i64| run.add_bytes(delta);

        let worker = || {
            loop {
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(file) = files.get(i) else {
                    return;
                };
                // The loop checks the token before each item, as Parallel.ForEach did.
                if cancel.is_cancelled() {
                    stopped.store(true, Ordering::SeqCst);
                    return;
                }

                match self
                    .hasher
                    .hash(&file.display_path, max_bytes, Some(&add_bytes), cancel)
                {
                    Err(_) => {
                        stopped.store(true, Ordering::SeqCst);
                        return;
                    }
                    Ok(None) => {
                        failures.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(Some(fingerprint)) => {
                        into.lock().unwrap_or_else(|p| p.into_inner()).push(Sample {
                            file: file.clone(),
                            hash: fingerprint.hash,
                            bytes_read: fingerprint.bytes_read,
                            identity: fingerprint.identity,
                        });
                    }
                }

                run.file_done(&file.name);
            }
        };

        std::thread::scope(|s| {
            for _ in 0..Self::MAX_PARALLELISM.min(files.len()) {
                s.spawn(worker);
            }
        });

        *incomplete += failures.into_inner();
        HashedBatch {
            samples: into.into_inner().unwrap_or_else(|p| p.into_inner()),
            cancelled: stopped.into_inner(),
        }
    }

    /// Turns fully-hashed files into groups: same real byte count, same digest, at least two of
    /// them once hardlinks have been folded together.
    fn confirm(samples: Vec<Sample>) -> Vec<DuplicateGroup> {
        // bytes_read rather than the indexed size: the row may be stale, and a file that shrank
        // since it was indexed must not be compared against one that did not.
        let mut by_content: HashMap<(i64, String), Vec<Sample>> = HashMap::new();
        for sample in samples {
            by_content
                .entry((sample.bytes_read, sample.hash.clone()))
                .or_default()
                .push(sample);
        }

        by_content
            .into_iter()
            .filter(|(_, members)| members.len() > 1)
            .filter_map(|((bytes_read, hash), members)| Self::build(bytes_read, hash, members))
            .collect()
    }

    /// One group, with every set of names belonging to a single file on disk collapsed into one
    /// entry. None when folding leaves fewer than two entries — the "copies" were all one file
    /// wearing several names. Deleting any of them would free nothing, so this is not a result,
    /// and it is what stops a hardlink-heavy tree burying everything else.
    fn build(size_bytes: i64, hash: String, mut members: Vec<Sample>) -> Option<DuplicateGroup> {
        members.sort_by(|a, b| a.file.display_path.cmp(&b.file.display_path));

        let mut entries: Vec<DuplicateFile> = Vec::new();
        let mut claimed: HashMap<FileIdentity, usize> = HashMap::new();

        for member in members {
            if let Some(identity) = member.identity {
                if let Some(&at) = claimed.get(&identity) {
                    // Another name for a file already listed. It travels with that entry.
                    entries[at].hardlink_paths.push(member.file.display_path);
                    continue;
                }
                claimed.insert(identity, entries.len());
            }

            entries.push(DuplicateFile {
                display_path: member.file.display_path,
                relative_dir_display: member.file.relative_dir_display,
                name: member.file.name,
                size_bytes,
                modified_utc: member.file.modified_utc,
                hidden: member.file.hidden,
                hardlink_paths: Vec::new(),
            });
        }

        if entries.len() > 1 {
            Some(DuplicateGroup {
                size_bytes,
                hash,
                files: entries,
            })
        } else {
            None
        }
    }
}

struct Sample {
    file: SearchHit,
    hash: String,
    bytes_read: i64,
    identity: Option<FileIdentity>,
}

struct HashedBatch {
    samples: Vec<Sample>,
    cancelled: bool,
}

/// One `scan` call's running counters and the rate at which it is willing to talk about them.
///
/// **The coalescing is not optional**: the hasher reports every megabyte, and forwarding each one
/// to a sink bound to the UI floods the dispatcher with work whose only job is to move a bar a
/// pixel. Phase boundaries always report; in between, at most one report per `REPORT_INTERVAL`.
/// Several threads write at once, so the counters are atomic and the throttle is behind a lock.
/// The lock is entered roughly once per megabyte read, which is nothing beside reading the
/// megabyte.
struct Run<'p> {
    progress: DuplicateProgressSink<'p>,
    gate: Mutex<RunState>,
    done: AtomicUsize,
    bytes_done: AtomicI64,
    total_bytes_hashed: AtomicI64,
    files_hashed: AtomicUsize,
}

struct RunState {
    phase: DuplicateScanPhase,
    total: usize,
    bytes_total: i64,
    name: String,
    since_report: Instant,
}

impl<'p> Run<'p> {
    const REPORT_INTERVAL: Duration = Duration::from_millis(100);

    fn new(progress: DuplicateProgressSink<'p>) -> Run<'p> {
        Run {
            progress,
            gate: Mutex::new(RunState {
                phase: DuplicateScanPhase::Shortlisting,
                total: 0,
                bytes_total: 0,
                name: String::new(),
                since_report: Instant::now(),
            }),
            done: AtomicUsize::new(0),
            bytes_done: AtomicI64::new(0),
            total_bytes_hashed: AtomicI64::new(0),
            files_hashed: AtomicUsize::new(0),
        }
    }

    /// Across every phase, for the outcome — `bytes_done` resets each time.
    fn total_bytes_hashed(&self) -> i64 {
        self.total_bytes_hashed.load(Ordering::SeqCst)
    }

    fn files_hashed(&self) -> usize {
        self.files_hashed.load(Ordering::SeqCst)
    }

    /// Each phase carries its own totals and starts from zero. Sampling's byte total is known
    /// exactly; the full pass's cannot be, until sampling has said who survived it. Two
    /// determinate bars in sequence is the honest shape — a single one would have to invent a
    /// denominator and then revise it downwards.
    fn begin_phase(&self, phase: DuplicateScanPhase, total: usize, bytes_total: i64) {
        {
            let mut state = self.gate.lock().unwrap_or_else(|p| p.into_inner());
            state.phase = phase;
            state.total = total;
            state.bytes_total = bytes_total;
            state.name.clear();
            self.done.store(0, Ordering::SeqCst);
            self.bytes_done.store(0, Ordering::SeqCst);
        }
        self.report(true);
    }

    /// Chunk deltas from the hasher, from any of the reading threads.
    fn add_bytes(&self, delta: i64) {
        self.bytes_done.fetch_add(delta, Ordering::SeqCst);
        self.total_bytes_hashed.fetch_add(delta, Ordering::SeqCst);
        self.report(false);
    }

    fn file_done(&self, name: &str) {
        self.done.fetch_add(1, Ordering::SeqCst);
        self.files_hashed.fetch_add(1, Ordering::SeqCst);
        self.gate.lock().unwrap_or_else(|p| p.into_inner()).name = name.to_string();
        self.report(false);
    }

    fn finished(&self) {
        self.gate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .name
            .clear();
        self.report(true);
    }

    fn report(&self, force: bool) {
        let Some(progress) = self.progress else {
            return;
        };

        let snapshot = {
            let mut state = self.gate.lock().unwrap_or_else(|p| p.into_inner());
            if !force && state.since_report.elapsed() < Self::REPORT_INTERVAL {
                return;
            }
            state.since_report = Instant::now();

            DuplicateScanProgress {
                phase: state.phase,
                done: self.done.load(Ordering::SeqCst),
                total: state.total,
                current_name: state.name.clone(),
                bytes_done: self.bytes_done.load(Ordering::SeqCst),
                bytes_total: state.bytes_total,
            }
        };

        progress(snapshot);
    }
}
