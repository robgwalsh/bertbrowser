//! Digesting a selection of files. Spec: Core/Services/Checksums/ChecksumRunner.cs.
//!
//! Shaped after `DuplicateScanner`, deliberately and in the same three places: four threads, a
//! progress callback bound once outside the loop, and a 100 ms coalescing throttle. It is the same
//! problem — many files, one hasher, a UI watching — so a second answer to it would be a second
//! thing to get wrong. One difference: results are placed by input index rather than collected
//! into a bag, so the window shows the files in the order the user selected them and two runs of
//! the same selection cannot come back in different orders.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::{ChecksumAlgorithm, ChecksumAlgorithms, FileDigester, FileDigests};
use crate::cancel::CancelToken;

/// One file's place in a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksumRow {
    pub path: String,
    pub name: String,
    /// None when the file could not be read — which is that file's problem alone.
    pub digests: Option<FileDigests>,
}

impl ChecksumRow {
    pub fn failed(&self) -> bool {
        self.digests.is_none()
    }

    pub fn digest(&self, algorithm: ChecksumAlgorithm) -> Option<&str> {
        self.digests
            .as_ref()
            .and_then(|d| d.by_algorithm.get(&algorithm))
            .map(String::as_str)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksumOutcome {
    pub rows: Vec<ChecksumRow>,
    pub algorithms: Vec<ChecksumAlgorithm>,
    /// The run stopped early. The rows it did finish are still here.
    pub cancelled: bool,
    /// Files that could not be read.
    pub failures: usize,
}

impl ChecksumOutcome {
    pub fn incomplete(&self) -> bool {
        self.failures > 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksumProgress {
    /// Files finished, of `files_total`.
    pub files_done: usize,
    pub files_total: usize,
    /// Bytes read, of `bytes_total`.
    pub bytes_done: i64,
    pub bytes_total: i64,
    /// The most recently finished file, for the detail line.
    pub current_name: Option<String>,
}

/// A sink for progress, called from whichever thread reports.
pub type ChecksumProgressSink<'a> = Option<&'a (dyn Fn(ChecksumProgress) + Sync)>;

pub struct ChecksumRunner<'d> {
    digester: &'d dyn FileDigester,
}

impl<'d> ChecksumRunner<'d> {
    /// Four, for `DuplicateScanner::MAX_PARALLELISM`'s reasons: hashing is slower than an NVMe can
    /// feed one thread, so more than one helps, and a spinning disk turns many readers into seek
    /// thrash, so not many more.
    pub const MAX_PARALLELISM: usize = 4;

    pub fn new(digester: &'d dyn FileDigester) -> ChecksumRunner<'d> {
        ChecksumRunner { digester }
    }

    /// `sizes` is what the listing knew per path, for the byte total of the bar; a path missing
    /// from it simply contributes nothing to the total.
    pub fn run(
        &self,
        paths: &[String],
        algorithms: &[ChecksumAlgorithm],
        sizes: Option<&HashMap<String, i64>>,
        progress: ChecksumProgressSink<'_>,
        cancel: &CancelToken,
    ) -> ChecksumOutcome {
        let wanted = ChecksumAlgorithms::normalise(algorithms.iter().copied());
        let rows: Vec<OnceLock<ChecksumRow>> = (0..paths.len()).map(|_| OnceLock::new()).collect();

        let bytes_total = sizes.map_or(0, |sizes| {
            paths
                .iter()
                .filter_map(|p| sizes.get(p))
                .fold(0i64, |acc, s| acc.saturating_add(*s))
        });

        let run = Beat::new(progress, paths.len(), bytes_total);
        let failures = AtomicUsize::new(0);
        let cancelled = AtomicBool::new(false);
        let next = AtomicUsize::new(0);

        // Bound once: the digester takes a callback and building one per file would allocate a
        // closure for every one of them.
        let add_bytes = |delta: i64| run.add_bytes(delta);

        let worker = || {
            loop {
                if cancelled.load(Ordering::SeqCst) {
                    return;
                }
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(path) = paths.get(i) else {
                    return;
                };
                // The loop checks the token before each item, as Parallel.For did.
                if cancel.is_cancelled() {
                    cancelled.store(true, Ordering::SeqCst);
                    return;
                }

                let digests = match self
                    .digester
                    .digest(path, &wanted, Some(&add_bytes), cancel)
                {
                    Ok(d) => d,
                    Err(_) => {
                        cancelled.store(true, Ordering::SeqCst);
                        return;
                    }
                };

                if digests.is_none() {
                    failures.fetch_add(1, Ordering::SeqCst);
                }

                let name = match bb_paths::file_name(path) {
                    "" => path.clone(),
                    n => n.to_string(),
                };
                let _ = rows[i].set(ChecksumRow {
                    path: path.clone(),
                    name: name.clone(),
                    digests,
                });
                run.file_done(name);
            }
        };

        std::thread::scope(|s| {
            for _ in 0..Self::MAX_PARALLELISM.min(paths.len()) {
                s.spawn(worker);
            }
        });

        // Whatever finished is still worth showing. A cancel is the user changing their mind, not
        // an error, and throwing away four minutes of completed digests to say so would be the
        // wrong answer to it.
        let cancelled = cancelled.load(Ordering::SeqCst);

        run.finished();

        let done: Vec<ChecksumRow> = rows.into_iter().filter_map(OnceLock::into_inner).collect();

        ChecksumOutcome {
            rows: done,
            algorithms: wanted,
            cancelled,
            failures: failures.into_inner(),
        }
    }
}

/// Coalesces progress onto a 100 ms beat.
///
/// Not optional. The digester reports every megabyte, from four threads at once; forwarding each
/// of those to a UI-bound sink posts thousands of callbacks a second to the dispatcher and the
/// window stops repainting. Counters are atomic because all four threads write them; the throttle
/// is behind a lock because only one of them should report.
struct Beat<'p> {
    progress: ChecksumProgressSink<'p>,
    files_total: usize,
    bytes_total: i64,
    bytes_done: AtomicI64,
    files_done: AtomicUsize,
    gate: Mutex<BeatState>,
}

struct BeatState {
    current: Option<String>,
    last_report: Option<Instant>,
}

impl<'p> Beat<'p> {
    const REPORT_INTERVAL: Duration = Duration::from_millis(100);

    fn new(progress: ChecksumProgressSink<'p>, files_total: usize, bytes_total: i64) -> Beat<'p> {
        Beat {
            progress,
            files_total,
            bytes_total,
            bytes_done: AtomicI64::new(0),
            files_done: AtomicUsize::new(0),
            gate: Mutex::new(BeatState {
                current: None,
                last_report: None,
            }),
        }
    }

    fn add_bytes(&self, delta: i64) {
        self.bytes_done.fetch_add(delta, Ordering::SeqCst);
        self.report(false);
    }

    fn file_done(&self, name: String) {
        self.files_done.fetch_add(1, Ordering::SeqCst);
        self.gate.lock().unwrap_or_else(|p| p.into_inner()).current = Some(name);
        self.report(false);
    }

    fn finished(&self) {
        self.report(true);
    }

    fn report(&self, force: bool) {
        let Some(progress) = self.progress else {
            return;
        };

        let mut state = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        if !force
            && let Some(last) = state.last_report
            && now.duration_since(last) < Self::REPORT_INTERVAL
        {
            return;
        }

        state.last_report = Some(now);
        progress(ChecksumProgress {
            files_done: self.files_done.load(Ordering::SeqCst),
            files_total: self.files_total,
            bytes_done: self.bytes_done.load(Ordering::SeqCst),
            bytes_total: self.bytes_total,
            current_name: state.current.clone(),
        });
    }
}
