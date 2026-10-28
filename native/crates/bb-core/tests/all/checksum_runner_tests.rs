// Port of tests/BertBrowser.Core.Tests/ChecksumRunnerTests.cs
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use bb_core::cancel::{CancelToken, Cancelled};
use bb_core::checksums::{
    ByteProgress, ChecksumAlgorithm, ChecksumOutcome, ChecksumProgress, ChecksumProgressSink,
    ChecksumRunner, FileDigester, FileDigests,
};
use bb_paths::fold_upper;

fn run(
    digester: &FakeDigester,
    paths: &[String],
    progress: ChecksumProgressSink<'_>,
    cancel: &CancelToken,
) -> ChecksumOutcome {
    ChecksumRunner::new(digester).run(paths, &[ChecksumAlgorithm::Sha256], None, progress, cancel)
}

fn paths(count: usize, width: usize) -> Vec<String> {
    (0..count)
        .map(|i| format!(r"C:\file{i:0width$}.bin"))
        .collect()
}

#[test]
fn every_file_gets_a_row() {
    let digester = FakeDigester::new()
        .with(r"C:\a.bin", "AA")
        .with(r"C:\b.bin", "BB");

    let outcome = run(
        &digester,
        &[r"C:\a.bin".to_string(), r"C:\b.bin".to_string()],
        None,
        &CancelToken::none(),
    );

    assert_eq!(2, outcome.rows.len());
    assert!(!outcome.cancelled);
    assert!(!outcome.incomplete());
}

/// Four threads finish in whatever order they like; the window must not. A selection shown once
/// and hashed twice has to come back the same way round both times.
#[test]
fn rows_come_back_in_input_order() {
    let paths = paths(200, 3);
    let mut digester = FakeDigester::new();
    for path in &paths {
        digester = digester.with(path, "AA");
    }

    let outcome = run(&digester, &paths, None, &CancelToken::none());

    let got: Vec<String> = outcome.rows.iter().map(|r| r.path.clone()).collect();
    assert_eq!(paths, got);
}

/// One item's failure never affects the others — the rule every multi-item operation in this app
/// follows.
#[test]
fn an_unreadable_file_is_one_failed_row_and_the_rest_survive() {
    let digester = FakeDigester::new()
        .with(r"C:\a.bin", "AA")
        .with(r"C:\c.bin", "CC");

    let outcome = run(
        &digester,
        &[
            r"C:\a.bin".to_string(),
            r"C:\b.bin".to_string(),
            r"C:\c.bin".to_string(),
        ],
        None,
        &CancelToken::none(),
    );

    assert_eq!(3, outcome.rows.len());
    assert_eq!(1, outcome.failures);
    assert!(outcome.incomplete());

    let b = outcome.rows.iter().find(|r| r.path == r"C:\b.bin").unwrap();
    assert!(b.failed());
    let a = outcome.rows.iter().find(|r| r.path == r"C:\a.bin").unwrap();
    assert_eq!(Some("AA"), a.digest(ChecksumAlgorithm::Sha256));
}

#[test]
fn a_cancel_keeps_what_was_finished() {
    let paths = paths(500, 3);
    let cancel = CancelToken::new();

    let after = {
        let cancel = cancel.clone();
        move |done: usize| {
            if done >= 20 {
                cancel.cancel();
            }
        }
    };
    let mut digester = FakeDigester::new().after_each_file(after);
    for path in &paths {
        digester = digester.with(path, "AA");
    }

    let outcome = run(&digester, &paths, None, &cancel);

    assert!(outcome.cancelled);
    assert!(!outcome.rows.is_empty());
    assert!(
        outcome.rows.len() < paths.len(),
        "a cancel should stop short of the whole list"
    );
}

/// The coalescing. The digester reports every chunk from four threads; if each of those became a
/// progress callback the dispatcher would stop repainting.
#[test]
fn progress_is_coalesced_far_below_one_report_per_chunk() {
    let paths = paths(50, 2);
    let mut digester = FakeDigester::new().chunks_per_file(40);
    for path in &paths {
        digester = digester.with(path, "AA");
    }

    let reports = AtomicUsize::new(0);
    let sink = |_: ChecksumProgress| {
        reports.fetch_add(1, Ordering::SeqCst);
    };
    run(&digester, &paths, Some(&sink), &CancelToken::none());

    // 50 files x 40 chunks = 2,000 opportunities. A 100 ms beat over a run this fast should
    // produce a small handful; anything approaching the chunk count means the throttle is gone.
    let reports = reports.into_inner();
    assert!(
        reports < 200,
        "expected coalesced progress, got {reports} reports"
    );
}

#[test]
fn the_algorithms_are_normalised_once() {
    let digester = FakeDigester::new().with(r"C:\a.bin", "AA");

    let outcome = ChecksumRunner::new(&digester).run(
        &[r"C:\a.bin".to_string()],
        &[
            ChecksumAlgorithm::Sha512,
            ChecksumAlgorithm::Md5,
            ChecksumAlgorithm::Sha512,
        ],
        None,
        None,
        &CancelToken::none(),
    );

    assert_eq!(
        vec![ChecksumAlgorithm::Md5, ChecksumAlgorithm::Sha512],
        outcome.algorithms
    );
}

#[test]
fn an_empty_selection_is_an_empty_outcome() {
    let outcome = run(&FakeDigester::new(), &[], None, &CancelToken::none());

    assert!(outcome.rows.is_empty());
    assert!(!outcome.cancelled);
    assert!(!outcome.incomplete());
}

/// In-memory `FileDigester`. A path it does not hold is an unreadable file — None, not an error —
/// which is the contract the real one keeps.
struct FakeDigester {
    digests: Mutex<HashMap<String, String>>,
    done: AtomicUsize,
    chunks_per_file: usize,
    after_each_file: Option<Box<dyn Fn(usize) + Sync + Send>>,
}

impl FakeDigester {
    fn new() -> FakeDigester {
        FakeDigester {
            digests: Mutex::new(HashMap::new()),
            done: AtomicUsize::new(0),
            chunks_per_file: 1,
            after_each_file: None,
        }
    }

    fn with(self, path: &str, digest: &str) -> FakeDigester {
        self.digests
            .lock()
            .unwrap()
            .insert(fold_upper(path), digest.to_string());
        self
    }

    fn chunks_per_file(mut self, chunks: usize) -> FakeDigester {
        self.chunks_per_file = chunks;
        self
    }

    fn after_each_file(mut self, f: impl Fn(usize) + Sync + Send + 'static) -> FakeDigester {
        self.after_each_file = Some(Box::new(f));
        self
    }
}

impl FileDigester for FakeDigester {
    fn digest(
        &self,
        path: &str,
        algorithms: &[ChecksumAlgorithm],
        progress: ByteProgress<'_>,
        cancel: &CancelToken,
    ) -> Result<Option<FileDigests>, Cancelled> {
        cancel.check()?;

        let digest = match self.digests.lock().unwrap().get(&fold_upper(path)) {
            Some(d) => d.clone(),
            None => return Ok(None),
        };

        for _ in 0..self.chunks_per_file {
            cancel.check()?;
            if let Some(report) = progress {
                report(1024);
            }
        }

        if let Some(after) = &self.after_each_file {
            after(self.done.fetch_add(1, Ordering::SeqCst) + 1);
        }

        let by_algorithm = algorithms.iter().map(|&a| (a, digest.clone())).collect();

        Ok(Some(FileDigests {
            by_algorithm,
            bytes_read: self.chunks_per_file as i64 * 1024,
        }))
    }
}
