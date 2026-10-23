//! The second pass of a `content:` search: takes the shortlist the name and metadata produced and
//! decides it by reading files. Spec: Core/Services/Search/ContentScanner.cs.
//!
//! **Only undecided candidates are ever opened.** That is the whole point of the verdict being
//! three-valued: in `content:a OR ext:md` every `.md` is already a hit and goes straight to the
//! results, and in `is:dir content:x` every directory is already refused. **A cancel gives back a
//! floor, not nothing**: whatever matched before the stop really did match, and a cancelled search
//! that returned an empty list would blank the results the user was watching fill up.

use std::borrow::Cow;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bb_paths::{PathKey, fold_upper};

use super::{
    ContentReader, ContentSearchRules, ContentSnippet, ContentText, FileText, SearchCandidate,
    SearchMatch, SearchQuery,
};
use crate::cancel::CancelToken;
use crate::data::SearchHit;
use crate::models::{ContentScanLimit, ContentScanProgress, ContentScanReport};

/// A sink for hits as they are found, when the caller wants them streamed.
pub type LiveBatches<'a> = Option<&'a (dyn Fn(&[SearchHit]) + Sync)>;

/// A sink for how far a reading pass has got.
pub type ContentProgressSink<'a> = Option<&'a (dyn Fn(ContentScanProgress) + Sync)>;

/// What the reading pass produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentScanOutcome {
    /// The survivors, capped.
    pub hits: Vec<SearchHit>,
    /// The result cap was reached.
    pub truncated: bool,
    /// The user stopped it; `hits` is a floor.
    pub cancelled: bool,
    /// What was actually read, for saying so out loud.
    pub report: ContentScanReport,
}

pub struct ContentScanner<'r> {
    reader: &'r dyn ContentReader,
}

/// `AtomicU8` encoding of the ceiling that stopped the workers; zero is none.
const STOP_FILES: u8 = 2;
const STOP_BYTES: u8 = 3;

impl<'r> ContentScanner<'r> {
    pub fn new(reader: &'r dyn ContentReader) -> ContentScanner<'r> {
        ContentScanner { reader }
    }

    /// `candidates`: the first pass's hits, name and metadata already applied. `max_results` is the
    /// result cap, separate from the file ceiling. `max_bytes_per_file` is the one bound a person
    /// can set.
    #[allow(clippy::too_many_arguments)]
    pub fn scan(
        &self,
        query: &SearchQuery,
        candidates: &[SearchHit],
        max_results: usize,
        candidates_truncated: bool,
        max_bytes_per_file: u64,
        live_batches: LiveBatches<'_>,
        progress: ContentProgressSink<'_>,
        cancel: &CancelToken,
    ) -> ContentScanOutcome {
        let needles = query.content_needles();

        // One cheap pass with no I/O at all, to separate what the name already settled from what
        // has to be read. Emitting the settled ones first is also why a query like
        // "content:todo OR ext:md" puts rows on screen instantly.
        let mut settled: Vec<SearchHit> = Vec::new();
        let mut to_read: Vec<SearchHit> = Vec::new();
        for hit in candidates {
            let keys = Keys::of(hit);
            match query.evaluate(&keys.candidate(hit, None)) {
                SearchMatch::Yes => settled.push(hit.clone()),
                SearchMatch::NeedsContent => to_read.push(hit.clone()),
                SearchMatch::No => {}
            }
        }

        let mut hits: Vec<SearchHit> = Vec::new();
        let run = Run::new(progress, to_read.len());

        if !settled.is_empty() {
            let take = &settled[..settled.len().min(max_results)];
            hits.extend_from_slice(take);
            if let Some(report) = live_batches {
                report(take);
            }
        }

        let mut limit = if candidates_truncated {
            ContentScanLimit::Candidates
        } else {
            ContentScanLimit::None
        };
        let mut cancelled = false;
        let unreadable = AtomicUsize::new(0);
        let truncated_files = AtomicUsize::new(0);
        let mut last_examined = candidates.last().map(|h| h.display_path.clone());

        if hits.len() < max_results && !to_read.is_empty() {
            let found: Mutex<Vec<SearchHit>> = Mutex::new(Vec::new());
            let stop = AtomicU8::new(0);
            let stopped_by_cancel = AtomicBool::new(false);
            let next = AtomicUsize::new(0);

            let worker = || {
                loop {
                    if stop.load(Ordering::SeqCst) != 0 || stopped_by_cancel.load(Ordering::SeqCst)
                    {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(hit) = to_read.get(i) else {
                        return;
                    };
                    // The loop checks the token before each item, as Parallel.ForEach did.
                    if cancel.is_cancelled() {
                        stopped_by_cancel.store(true, Ordering::SeqCst);
                        return;
                    }

                    // Both ceilings are claimed before the open, not after, so the count cannot
                    // overshoot by the number of threads.
                    if !run.try_claim_file() {
                        let _ = stop.compare_exchange(
                            0,
                            STOP_FILES,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        );
                        return;
                    }
                    if run.bytes_read() >= ContentSearchRules::MAX_BYTES_READ {
                        let _ = stop.compare_exchange(
                            0,
                            STOP_BYTES,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        );
                        return;
                    }

                    let text: Cow<'_, ContentText> =
                        match self
                            .reader
                            .read(&hit.display_path, max_bytes_per_file, cancel)
                        {
                            Err(_) => {
                                stopped_by_cancel.store(true, Ordering::SeqCst);
                                return;
                            }
                            Ok(FileText::Unreadable) => {
                                // This file had a problem; the rest are unaffected, which is the
                                // contract every batch operation in this app keeps.
                                unreadable.fetch_add(1, Ordering::SeqCst);
                                run.file_done(&hit.display_path, 0);
                                continue;
                            }
                            Ok(FileText::NotText) => Cow::Borrowed(ContentText::none()),
                            Ok(FileText::Text(t)) => Cow::Owned(t),
                        };

                    run.file_done(&hit.display_path, text.text.len() as u64);
                    if text.truncated {
                        truncated_files.fetch_add(1, Ordering::SeqCst);
                    }

                    let keys = Keys::of(hit);
                    if query.evaluate(&keys.candidate(hit, Some(&text))) != SearchMatch::Yes {
                        continue;
                    }

                    let mut matched = hit.clone();
                    matched.content_match = ContentSnippet::for_content(&text, &needles);
                    found
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(matched);

                    // The text goes out of scope here and must: at the file ceiling, holding one
                    // per hit would be tens of thousands of megabyte strings alive at once.
                }
            };

            std::thread::scope(|s| {
                for _ in 0..ContentSearchRules::MAX_PARALLELISM.min(to_read.len()) {
                    s.spawn(worker);
                }
            });

            if stopped_by_cancel.load(Ordering::SeqCst) || cancel.is_cancelled() {
                cancelled = true;
            }
            if limit == ContentScanLimit::None {
                limit = match stop.load(Ordering::SeqCst) {
                    STOP_FILES => ContentScanLimit::Files,
                    STOP_BYTES => ContentScanLimit::Bytes,
                    _ => ContentScanLimit::None,
                };
            }

            // Ordered by path so a run is repeatable: the workers complete in whatever order the
            // disk answers, and a result list that reshuffled between two identical searches would
            // be impossible to trust.
            let mut ordered = found.into_inner().unwrap_or_else(|p| p.into_inner());
            ordered.sort_by_cached_key(|h| Keys::of(h).path_key);

            for hit in &ordered {
                if hits.len() >= max_results {
                    break;
                }
                hits.push(hit.clone());
            }

            if !ordered.is_empty()
                && let Some(report) = live_batches
            {
                let take = ordered.len().min(max_results.saturating_sub(settled.len()));
                report(&ordered[..take]);
            }

            last_examined = run.last_path().or(last_examined);
        }

        run.finished();

        let report = ContentScanReport {
            files_read: run.files_read(),
            bytes_read: run.bytes_read(),
            limit,
            unreadable: unreadable.into_inner(),
            truncated: truncated_files.into_inner(),
            last_path_examined: last_examined,
        };

        ContentScanOutcome {
            truncated: hits.len() >= max_results,
            hits,
            cancelled,
            report,
        }
    }
}

/// The upper-cased keys a hit is judged by, owned so a borrowed candidate can be built over them.
struct Keys {
    name_key: String,
    path_key: String,
}

impl Keys {
    fn of(hit: &SearchHit) -> Keys {
        Keys {
            name_key: fold_upper(&hit.name),
            path_key: PathKey::canonicalize(&hit.display_path)
                .unwrap_or_else(|_| fold_upper(&hit.display_path)),
        }
    }

    /// Rebuilds the candidate for the reading pass, now that the file's text is in hand. Every
    /// field has to survive the round trip through `SearchHit`, because this is the tree's
    /// *second* evaluation and it is the one that decides: drop the attributes here and
    /// `is:readonly content:todo` would shortlist correctly and then let every file through.
    fn candidate<'a>(
        &'a self,
        hit: &'a SearchHit,
        content: Option<&'a ContentText>,
    ) -> SearchCandidate<'a> {
        SearchCandidate {
            name_key: &self.name_key,
            path_key: &self.path_key,
            is_directory: hit.is_directory,
            size_bytes: hit.size_bytes,
            modified_utc: hit.modified_utc,
            hidden: hit.hidden,
            attributes: hit.attributes,
            created_utc: hit.created_utc,
            content,
        }
    }
}

/// The counters, and the throttle in front of the progress callback. The coalescing is not
/// optional: a report per file at thousands of files a second floods the UI with work whose only
/// job is to move a number. At most one report per 100 ms in between, and the finish always
/// reports.
struct Run<'p> {
    progress: Option<&'p (dyn Fn(ContentScanProgress) + Sync)>,
    total: usize,
    claimed: AtomicUsize,
    files_read: AtomicUsize,
    bytes_read: AtomicU64,
    gate: Mutex<Gate>,
}

struct Gate {
    since_report: Instant,
    last_path: Option<String>,
}

impl<'p> Run<'p> {
    const REPORT_INTERVAL: Duration = Duration::from_millis(100);

    fn new(progress: Option<&'p (dyn Fn(ContentScanProgress) + Sync)>, total: usize) -> Run<'p> {
        Run {
            progress,
            total,
            claimed: AtomicUsize::new(0),
            files_read: AtomicUsize::new(0),
            bytes_read: AtomicU64::new(0),
            gate: Mutex::new(Gate {
                since_report: Instant::now(),
                last_path: None,
            }),
        }
    }

    fn files_read(&self) -> usize {
        self.files_read.load(Ordering::SeqCst)
    }

    fn bytes_read(&self) -> u64 {
        self.bytes_read.load(Ordering::SeqCst)
    }

    fn last_path(&self) -> Option<String> {
        self.gate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .last_path
            .clone()
    }

    /// Takes one from the file ceiling, or refuses when it is spent.
    fn try_claim_file(&self) -> bool {
        self.claimed.fetch_add(1, Ordering::SeqCst) < ContentSearchRules::MAX_FILES_OPENED
    }

    fn file_done(&self, path: &str, bytes: u64) {
        self.files_read.fetch_add(1, Ordering::SeqCst);
        self.bytes_read.fetch_add(bytes, Ordering::SeqCst);
        self.gate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .last_path = Some(path.to_string());
        self.report(false);
    }

    fn finished(&self) {
        self.report(true);
    }

    fn report(&self, force: bool) {
        let Some(progress) = self.progress else {
            return;
        };
        let snapshot = {
            let mut gate = self.gate.lock().unwrap_or_else(|p| p.into_inner());
            if !force && gate.since_report.elapsed() < Self::REPORT_INTERVAL {
                return;
            }
            gate.since_report = Instant::now();
            ContentScanProgress {
                files_read: self.files_read(),
                files_to_read: self.total,
                bytes_read: self.bytes_read(),
            }
        };
        progress(snapshot);
    }
}
