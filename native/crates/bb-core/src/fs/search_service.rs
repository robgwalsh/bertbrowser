//! Search orchestration, lazy indexing with stale-while-revalidate: fresh index, instant DB query;
//! stale or unwatched index, instant DB query plus a background re-crawl; unindexed, a live
//! filesystem scan streaming hits while a single-flight background crawl indexes the subtree for
//! next time. Spec: Core/Services/SearchService.cs.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bb_paths::PathKey;

use super::index_crawler::IndexCrawler;
use super::index_watcher::IndexWatcher;
use super::walker::{FileSystemWalker, WalkError};
use crate::archives::{ArchiveBrowser, ArchiveFormats, ArchivePath};
use crate::cancel::CancelToken;
use crate::data::{self, FsIndexRepository, SearchHit};
use crate::mft::MftIndex;
use crate::models::{ContentScanReport, SearchOutcome, SearchResultSource};
use crate::search::{
    ContentProgressSink, ContentReader, ContentScanner, ContentSearchRules,
    FileSystemContentReader, LiveBatches, SearchCandidate, SearchQuery, SearchQueryParse,
};

/// The two streaming channels a search can report through. `live_batches` carries what was found
/// as it is found; `content_progress` carries how much of a `content:` pass is left to look at,
/// which is the only thing worth saying during the tens of seconds a whole-tree grep can take.
#[derive(Clone, Copy, Default)]
pub struct SearchProgress<'a> {
    pub live_batches: LiveBatches<'a>,
    pub content_progress: ContentProgressSink<'a>,
}

type RefreshListener = Box<dyn Fn(&str) + Send + Sync>;

/// What the background crawls share with the service: everything a crawl thread needs after the
/// service's own methods have returned.
struct Shared {
    repository: Arc<FsIndexRepository>,
    crawler: Arc<IndexCrawler>,
    watchers: Arc<dyn IndexWatcher>,
    mft: Arc<dyn MftIndex>,
    archives: Option<Arc<dyn ArchiveBrowser>>,
    content_reader: Box<dyn ContentReader + Send + Sync>,
    content_budget: Option<Box<dyn Fn() -> u64 + Send + Sync>>,
    active_crawls: Mutex<HashSet<String>>,
    crawl_threads: Mutex<Vec<JoinHandle<()>>>,
    lifetime: CancelToken,
    index_refreshed: Mutex<Vec<RefreshListener>>,
}

pub struct SearchService {
    shared: Arc<Shared>,
}

impl SearchService {
    const MAX_RESULTS: usize = 1000;

    /// How many rows one flat branch view shows. Its own number rather than `MAX_RESULTS`: a
    /// thousand results is plenty for a search, which is a question with an answer; a flat view of
    /// a photo library is not a question, and stopping it at a thousand would read as broken.
    /// Fifty thousand rows is the point where the list itself, not the walk, becomes the thing you
    /// are fighting.
    pub const MAX_FLAT_ENTRIES: usize = 50_000;

    /// How many containers one `in:archives` pass will open.
    const MAX_ARCHIVES_SCANNED: usize = 200;

    /// And how many bytes of them, whichever bound is reached first.
    const MAX_ARCHIVE_BYTES_SCANNED: i64 = 2 * 1024 * 1024 * 1024;

    /// The first batch a walk streams goes as soon as it has this many rows, so something is on
    /// screen at once; each batch after it may be twice the last, up to `MAX_LIVE_BATCH_SIZE`, or
    /// goes when `LIVE_BATCH_INTERVAL` has passed. Growing rather than fixed: each batch is a UI
    /// post with a layout pass after it, so fifty at a time cost a fast walk of 20,000 files 400
    /// posts, but one batch at the end puts all the row adding after the walk instead of beside it
    /// (measured: 486 ms became 575). Doubling keeps both.
    pub const FIRST_LIVE_BATCH_SIZE: usize = 50;
    pub const MAX_LIVE_BATCH_SIZE: usize = 4096;
    pub const LIVE_BATCH_INTERVAL: Duration = Duration::from_millis(250);

    pub fn new(
        repository: Arc<FsIndexRepository>,
        crawler: Arc<IndexCrawler>,
        watchers: Arc<dyn IndexWatcher>,
        mft: Arc<dyn MftIndex>,
    ) -> SearchService {
        SearchService {
            shared: Arc::new(Shared {
                repository,
                crawler,
                watchers,
                mft,
                archives: None,
                content_reader: Box::new(FileSystemContentReader),
                content_budget: None,
                active_crawls: Mutex::new(HashSet::new()),
                crawl_threads: Mutex::new(Vec::new()),
                lifetime: CancelToken::new(),
                index_refreshed: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Only consulted when a query says `in:archives`; without one that is a no-op.
    pub fn with_archives(mut self, archives: Arc<dyn ArchiveBrowser>) -> SearchService {
        self.shared_mut().archives = Some(archives);
        self
    }

    /// The `content:` reading pass's reader. Injectable for the reason the hasher is.
    pub fn with_content_reader(
        mut self,
        reader: Box<dyn ContentReader + Send + Sync>,
    ) -> SearchService {
        self.shared_mut().content_reader = reader;
        self
    }

    /// How much of each file to read, asked fresh each search: a closure rather than a value
    /// because this is a singleton and the setting can change while it lives. Core never sees the
    /// settings; the composition root closes over them, as `include_hidden` travels as a parameter.
    pub fn with_content_budget(
        mut self,
        budget: Box<dyn Fn() -> u64 + Send + Sync>,
    ) -> SearchService {
        self.shared_mut().content_budget = Some(budget);
        self
    }

    fn shared_mut(&mut self) -> &mut Shared {
        Arc::get_mut(&mut self.shared).expect("configured before any crawl starts")
    }

    /// Called (on a worker thread) with the canonical root key whose (re)crawl just completed.
    pub fn on_index_refreshed(&self, listener: RefreshListener) {
        self.shared
            .index_refreshed
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(listener);
    }

    /// Searches for `query_text` under `root_path`. `Ok(None)` when the query is too short to run.
    /// When the subtree is not indexed yet, live matches stream through `progress.live_batches`
    /// while a background crawl builds the index; the returned outcome contains the full (possibly
    /// capped) hit list either way. A cancel during the walk or the index query is `Err(Cancelled)`;
    /// one during the content pass is reported in the outcome.
    pub fn search(
        &self,
        root_path: &str,
        query_text: &str,
        include_hidden: bool,
        progress: SearchProgress<'_>,
        cancel: &CancelToken,
    ) -> data::Result<Option<SearchOutcome>> {
        let parse = SearchQuery::parse(query_text);
        let Some(query) = parse.query.as_ref() else {
            return Ok(Self::refused(&parse));
        };

        // A root inside an archive is refused outright, and this is the hard invariant rather than
        // a missing feature. A virtual path canonicalizes happily, find_covering_root then misses,
        // and ensure_indexed would crawl a path that does not exist straight into fs_entry, after
        // which every subtree range scan over the archive's own containing folder returns archive
        // interiors. Searching inside a container is answered from its already loaded index.
        if ArchivePath::parse(root_path, is_file).is_some() {
            return Ok(Some(SearchOutcome::refused(
                "Searching inside an archive is not indexed. Extract it to search its contents.",
                SearchResultSource::Index,
            )));
        }

        let show_hidden = Self::show_hidden(query, include_hidden);
        let root_key = PathKey::canonicalize(root_path)?;
        let shared = &self.shared;
        let covering = shared.repository.find_covering_root(&root_key)?;

        if let Some(covering) = covering {
            // Fresh = crawl completed, nothing flagged stale, and something is patching changes
            // live: either the MFT/USN tail (NTFS volumes) or a directory watcher (the crawl
            // fallback for other roots). Watchers are in-memory, so the first crawl-backed search
            // each session deliberately lands on the stale path: instant cached results plus one
            // background re-crawl that re-attaches the watcher. MFT-covered roots are kept live by
            // the indexer, so they never fall to the crawler.
            let fresh = !covering.stale
                && (shared.mft.is_indexed(&covering.path_key)
                    || shared.watchers.is_watching(&covering.path_key));
            if !fresh {
                self.ensure_indexed(&covering.path_key, &covering.display_path);
            }

            cancel.check().map_err(|_| data::Error::Cancelled)?;
            let (hits, truncated) =
                shared
                    .repository
                    .search(root_path, query, Self::cap(query), show_hidden)?;

            let scoped = self.unanswered_for_want_of_metadata(query, &hits, Some(root_path))?;

            let mut visible = Self::visible(hits);
            visible.extend(self.scan_archives(query, root_path, progress.live_batches, cancel)?);

            let (hits, capped, cancelled, report) =
                self.apply_content(query, visible, truncated, progress, cancel);

            let mut outcome = SearchOutcome::new(
                hits,
                capped,
                if fresh {
                    SearchResultSource::Index
                } else {
                    SearchResultSource::StaleIndex
                },
                !fresh,
            );
            outcome.scope_lacks_metadata = scoped;
            outcome.cancelled = cancelled;
            outcome.content_scan = report;
            return Ok(Some(outcome));
        }

        self.ensure_indexed(&root_key, &PathKey::normalize_display(root_path)?);

        // A content query must not stream the *walk*: those rows are candidates, not results, and
        // most are about to be discarded. Putting them on screen and taking them away again would
        // be worse than a moment of nothing; the reading pass streams what actually matched. It
        // walks for candidates rather than results, so it stops at the larger ceiling and the
        // reading pass applies the 1,000-result cap afterwards.
        let mut outcome = self.live_scan(
            root_path,
            Some(query),
            Self::cap(query),
            if query.needs_content() {
                None
            } else {
                progress.live_batches
            },
            show_hidden,
            true,
            cancel,
        )?;

        let extra = self.scan_archives(query, root_path, progress.live_batches, cancel)?;
        let mut combined = std::mem::take(&mut outcome.hits);
        combined.extend(extra);

        let (hits, capped, cancelled, report) =
            self.apply_content(query, combined, outcome.truncated, progress, cancel);
        outcome.hits = hits;
        outcome.truncated = capped;
        outcome.cancelled = cancelled;
        outcome.content_scan = report;
        Ok(Some(outcome))
    }

    /// Everything under `root_path`, as list rows: what the flat branch view shows. Not a search,
    /// and here rather than in a service of its own because it is the same walk. **It never reads
    /// `fs_entry` and never starts a crawl.** A browse surface must be true rather than fast, and
    /// index rows can be a rebuild behind; and pressing Ctrl+B must not quietly enrol a subtree in
    /// the index. Files only: the walk still descends into every folder it does not show. `cap` is
    /// passed in rather than taken from `MAX_FLAT_ENTRIES` so a test can reach the truncated case
    /// without writing fifty thousand files.
    pub fn list_subtree(
        &self,
        root_path: &str,
        cap: usize,
        include_hidden: bool,
        progress: SearchProgress<'_>,
        cancel: &CancelToken,
    ) -> data::Result<SearchOutcome> {
        // Refused here rather than trusted to the caller, for the reason `search` gives at length.
        if ArchivePath::parse(root_path, is_file).is_some() {
            return Ok(SearchOutcome::refused(
                "Listing an archive's contents flat is done from the container itself.",
                SearchResultSource::LiveScan,
            ));
        }

        let mut outcome = self.live_scan(
            root_path,
            None,
            cap,
            progress.live_batches,
            include_hidden,
            false,
            cancel,
        )?;
        // `refresh_pending` is the live scan's answer to "is a crawl going to improve on this?",
        // and here none was started, so nothing is coming.
        outcome.refresh_pending = false;
        Ok(outcome)
    }

    /// Whole-PC search across every MFT-indexed volume. `Ok(None)` when the query is too short.
    /// Results are served straight from the index; while the MFT build is still in flight they
    /// are partial and `refresh_pending` is set.
    pub fn search_all(
        &self,
        query_text: &str,
        include_hidden: bool,
        progress: SearchProgress<'_>,
        cancel: &CancelToken,
    ) -> data::Result<Option<SearchOutcome>> {
        let parse = SearchQuery::parse(query_text);
        let Some(query) = parse.query.as_ref() else {
            return Ok(Self::refused(&parse));
        };
        let shared = &self.shared;

        cancel.check().map_err(|_| data::Error::Cancelled)?;
        let (hits, truncated) = shared.repository.search_global(
            query,
            Self::cap(query),
            Self::show_hidden(query, include_hidden),
        )?;

        let scoped = self.unanswered_for_want_of_metadata(query, &hits, None)?;

        let (hits, capped, cancelled, report) =
            self.apply_content(query, Self::visible(hits), truncated, progress, cancel);

        // While volumes are still enumerating the results are partial; the caller re-queries when
        // the index reports a refresh.
        let mut outcome = SearchOutcome::new(
            hits,
            capped,
            SearchResultSource::Index,
            shared.mft.is_building(),
        );
        outcome.scope_lacks_metadata = scoped;
        outcome.cancelled = cancelled;
        outcome.content_scan = report;
        Ok(Some(outcome))
    }

    /// How many rows the first pass asks for. A content query wants *candidates*, not results:
    /// using the 1,000-result cap would mean never grepping more than a thousand files, most of
    /// which are about to be thrown away. The larger number costs nothing extra to fetch.
    fn cap(query: &SearchQuery) -> usize {
        if query.needs_content() {
            ContentSearchRules::MAX_CANDIDATES
        } else {
            Self::MAX_RESULTS
        }
    }

    /// Runs the reading pass over a first-pass result, or returns it untouched. Takes the
    /// *visible* hits, after `visible` has dropped anything in a delete's holding folder or the
    /// Recycle Bin: opening those would spend the file ceiling on things the user has already
    /// deleted.
    fn apply_content(
        &self,
        query: &SearchQuery,
        hits: Vec<SearchHit>,
        truncated: bool,
        progress: SearchProgress<'_>,
        cancel: &CancelToken,
    ) -> (Vec<SearchHit>, bool, bool, Option<ContentScanReport>) {
        if !query.needs_content() {
            return (hits, truncated, false, None);
        }
        let shared = &self.shared;
        let budget = shared
            .content_budget
            .as_ref()
            .map_or(ContentSearchRules::MAX_BYTES_PER_FILE, |b| b());
        let scan = ContentScanner::new(shared.content_reader.as_ref()).scan(
            query,
            &hits,
            Self::MAX_RESULTS,
            truncated,
            budget,
            progress.live_batches,
            progress.content_progress,
            cancel,
        );
        (scan.hits, scan.truncated, scan.cancelled, Some(scan.report))
    }

    /// Drops hits that live in a delete's holding folder. They are still on disk, so Ctrl+Z can
    /// put them back, but they have been deleted as far as the user is concerned, and search
    /// saying otherwise reads as a delete that silently failed.
    fn visible(hits: Vec<SearchHit>) -> Vec<SearchHit> {
        if hits.iter().any(|h| is_held_path(&h.display_path)) {
            hits.into_iter()
                .filter(|h| !is_held_path(&h.display_path))
                .collect()
        } else {
            hits
        }
    }

    /// What to return when the text did not yield a runnable query. A parse *problem* is an
    /// outcome with no hits and a message; "nothing to run" is None, which is what tells the
    /// caller to go back to showing the plain directory listing.
    fn refused(parse: &SearchQueryParse) -> Option<SearchOutcome> {
        parse
            .problem
            .as_ref()
            .map(|problem| SearchOutcome::refused(problem, SearchResultSource::Index))
    }

    /// Whether this run should surface hidden entries. Search normally excludes them whatever the
    /// browse setting says (hidden entries are index noise that buries the results a search is
    /// for), but a query that explicitly asks for them has to override that, or `is:hidden`
    /// returns nothing every time and reads as broken.
    fn show_hidden(query: &SearchQuery, include_hidden: bool) -> bool {
        include_hidden || query.wants_hidden()
    }

    /// Whether an empty result means "nothing matched" or "nothing could have matched": a size or
    /// date filter over an index built by the names-only fallback can never return a row. Asked
    /// only when the answer could change what the user is told.
    fn unanswered_for_want_of_metadata(
        &self,
        query: &SearchQuery,
        hits: &[SearchHit],
        root_path: Option<&str>,
    ) -> data::Result<bool> {
        if !hits.is_empty() || !query.needs_metadata() {
            return Ok(false);
        }
        Ok(!self.shared.repository.has_size_data(root_path)?)
    }

    /// The `in:archives` second pass: opens the containers the walk finds and searches their
    /// entry names. **Opt-in, and it has to be**: a bare word must never open an archive, and
    /// opening one is orders of magnitude more expensive than reading an index row. It finds the
    /// containers itself rather than reusing the first pass's hits, because the query describes
    /// what the user is looking for *inside* an archive and there is no reason it should also
    /// match the archive's own name. No schema change, and there must not be one: archive contents
    /// in a `PathKey`-keyed corpus would poison every subtree scan over the folders those
    /// containers live in. Capped both ways, per container and per byte.
    fn scan_archives(
        &self,
        query: &SearchQuery,
        root_path: &str,
        live_batches: LiveBatches<'_>,
        cancel: &CancelToken,
    ) -> data::Result<Vec<SearchHit>> {
        let Some(archives) = self.shared.archives.as_ref() else {
            return Ok(Vec::new());
        };
        if !query.wants_archives() {
            return Ok(Vec::new());
        }

        // One walk for the containers, matching on the name table rather than on the query.
        // Bounded as it goes, so a folder holding ten thousand zips stops at the cap rather than
        // collecting them all and then throwing most away.
        let mut containers: Vec<String> = Vec::new();
        let mut found = 0i64;
        let walked = FileSystemWalker::walk(root_path, false, false, cancel, |entry| {
            if entry.is_directory || !ArchiveFormats::is_archive_name(&entry.name) {
                return true;
            }
            containers.push(entry.display_path.clone());
            found += entry.size_bytes;
            containers.len() < Self::MAX_ARCHIVES_SCANNED && found < Self::MAX_ARCHIVE_BYTES_SCANNED
        });
        walked.map_err(walk_error)?;

        let mut hits: Vec<SearchHit> = Vec::new();
        for path in containers {
            cancel.check().map_err(|_| data::Error::Cancelled)?;
            if hits.len() >= Self::MAX_RESULTS {
                break;
            }
            let inside =
                archives.search_archive(&path, query, Self::MAX_RESULTS - hits.len(), cancel);
            if inside.is_empty() {
                continue;
            }
            // Straight onto the same stream the live scan uses, so results land in a list that is
            // already on screen.
            if let Some(report) = live_batches {
                report(&inside);
            }
            hits.extend(inside);
        }
        Ok(hits)
    }

    /// Walks a subtree, keeping what `query` matches. **None means every entry**: the flat branch
    /// view, which is a listing rather than a search. Deliberately not a query that matches
    /// everything, which the grammar refuses to build; manufacturing one would put a permanent
    /// member into the `matches`/`write_sql` agreement contract on behalf of a caller that is not
    /// asking a question. `cap` is passed in because the three callers stop at three different
    /// numbers; `include_directories` false drops folder rows while the walk still descends.
    #[allow(clippy::too_many_arguments)]
    fn live_scan(
        &self,
        root_path: &str,
        query: Option<&SearchQuery>,
        cap: usize,
        live_batches: LiveBatches<'_>,
        include_hidden: bool,
        include_directories: bool,
        cancel: &CancelToken,
    ) -> data::Result<SearchOutcome> {
        let root_display = PathKey::normalize_display(root_path)?;
        let root_trimmed = root_display.trim_end_matches('\\').to_string();
        let mut hits: Vec<SearchHit> = Vec::new();
        let mut batch: Vec<SearchHit> = Vec::new();
        let mut truncated = false;
        let mut batch_size = Self::FIRST_LIVE_BATCH_SIZE;
        let mut since_flush = Instant::now();

        // The walk hands over a folder's entries together, so the relative folder is worked out
        // once per folder rather than once per row.
        let mut last_parent: Option<String> = None;
        let mut last_rel_dir = String::new();

        let walked = FileSystemWalker::walk(root_path, include_hidden, false, cancel, |entry| {
            if !include_directories && entry.is_directory {
                return true; // not emitted, still descended into
            }
            if let Some(query) = query
                && !query.matches(&SearchCandidate {
                    name_key: &entry.name_key,
                    path_key: &entry.path_key,
                    is_directory: entry.is_directory,
                    size_bytes: entry.size_bytes,
                    modified_utc: entry.modified_utc,
                    hidden: entry.hidden,
                    attributes: entry.attributes,
                    created_utc: entry.created_utc,
                    content: None,
                })
            {
                return true;
            }
            if is_held_path(&entry.display_path) {
                return true; // deleted, just not committed yet
            }
            if hits.len() >= cap {
                truncated = true;
                return false; // stop scanning for hits; the background crawl keeps indexing
            }

            let (parent, _) = bb_paths::split_last(&entry.display_path);
            if last_parent.as_deref() != Some(parent) {
                last_parent = Some(parent.to_string());
                last_rel_dir = relative_dir(&root_trimmed, parent);
            }

            let hit = SearchHit {
                display_path: entry.display_path.clone(),
                relative_dir_display: last_rel_dir.clone(),
                name: entry.name.clone(),
                is_directory: entry.is_directory,
                size_bytes: entry.size_bytes,
                modified_utc: entry.modified_utc,
                hidden: entry.hidden,
                attributes: entry.attributes,
                created_utc: entry.created_utc,
                content_match: None,
            };
            hits.push(hit.clone());
            batch.push(hit);

            if batch.len() >= batch_size || since_flush.elapsed() >= Self::LIVE_BATCH_INTERVAL {
                if let Some(report) = live_batches {
                    report(&batch);
                }
                batch.clear();
                batch_size = (batch_size * 2).min(Self::MAX_LIVE_BATCH_SIZE);
                since_flush = Instant::now();
            }
            true
        });
        walked.map_err(walk_error)?;

        if !batch.is_empty()
            && let Some(report) = live_batches
        {
            report(&batch);
        }

        Ok(SearchOutcome::new(
            hits,
            truncated,
            SearchResultSource::LiveScan,
            true,
        ))
    }

    /// Kicks off a background crawl of `root_key` unless one is already in flight (single-flight
    /// per root: typing more characters never restarts the crawl). Runs on the service lifetime,
    /// not any search's token. The watcher attaches before crawling starts so no change event is
    /// missed; crawl_gen stamping makes concurrent watcher writes safe.
    fn ensure_indexed(&self, root_key: &str, display_path: &str) {
        let shared = &self.shared;
        {
            let mut active = shared
                .active_crawls
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if !active.insert(root_key.to_string()) {
                return;
            }
        }
        let worker = shared.clone();
        let key = root_key.to_string();
        let display = display_path.to_string();
        let thread = std::thread::Builder::new()
            .name("index-crawl".into())
            .spawn(move || {
                worker.watchers.watch(&key, &display);
                // Background index build is best-effort; searches fall back to live scans.
                let completed = worker
                    .crawler
                    .crawl(&display, &worker.lifetime, None)
                    .unwrap_or(false);
                if completed {
                    let listeners = worker
                        .index_refreshed
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    for listener in listeners.iter() {
                        listener(&key);
                    }
                }
                worker
                    .active_crawls
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&key);
            });
        if let Ok(handle) = thread {
            let mut threads = shared
                .crawl_threads
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            threads.retain(|t| !t.is_finished());
            threads.push(handle);
        }
    }
}

impl Drop for SearchService {
    /// Cancels the service lifetime, which every background crawl runs on, and waits for them:
    /// a crawl checks the token per directory and per chunk, so this is prompt.
    fn drop(&mut self) {
        self.shared.lifetime.cancel();
        let threads: Vec<JoinHandle<()>> = std::mem::take(
            &mut *self
                .shared
                .crawl_threads
                .lock()
                .unwrap_or_else(|p| p.into_inner()),
        );
        for thread in threads {
            let _ = thread.join();
        }
    }
}

/// The hit's parent relative to the search root ("" for a direct child), as `Path.GetRelativePath`
/// gave it: the walker builds every display path under the root's own display form, so this is a
/// prefix strip rather than a path resolution.
fn relative_dir(root_trimmed: &str, parent: &str) -> String {
    let parent = parent.trim_end_matches('\\');
    if parent.len() <= root_trimmed.len() {
        return String::new();
    }
    if parent.as_bytes()[root_trimmed.len()] == b'\\'
        && parent[..root_trimmed.len()].eq_ignore_ascii_case(root_trimmed)
    {
        return parent[root_trimmed.len() + 1..].to_string();
    }
    parent.to_string()
}

fn walk_error(e: WalkError) -> data::Error {
    match e {
        WalkError::Cancelled => data::Error::Cancelled,
        WalkError::Path(p) => data::Error::Path(p),
    }
}

/// `File.Exists`: a file, not a folder, is there.
fn is_file(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// Whether a path sits in a delete's holding folder or a Recycle Bin: deleted as far as the user is
/// concerned, however much it is still on disk. Asked once per row by a search and by the flat
/// view, so the segments are walked in place.
// TODO(port): this is `DeleteExecutor.IsHeldPath` and `ProtectedLocations.IsRecycleBinName`
// (Core/Services/Delete); when Delete is ported it moves there and the names come from one table.
pub fn is_held_path(path: &str) -> bool {
    const RECYCLE_BIN_FOLDERS: [&str; 3] = ["$Recycle.Bin", "$RECYCLE.BIN", "RECYCLER"];
    const TRASH_FOLDER_NAME: &str = ".bertbrowser-trash";
    const LOCAL_STAGING_PREFIX: &str = ".bertbrowser-deleted-";

    path.split(['\\', '/']).any(|segment| {
        !segment.is_empty()
            && (RECYCLE_BIN_FOLDERS
                .iter()
                .any(|f| segment.eq_ignore_ascii_case(f))
                || segment.eq_ignore_ascii_case(TRASH_FOLDER_NAME)
                || (segment.len() >= LOCAL_STAGING_PREFIX.len()
                    && segment.is_char_boundary(LOCAL_STAGING_PREFIX.len())
                    && segment[..LOCAL_STAGING_PREFIX.len()]
                        .eq_ignore_ascii_case(LOCAL_STAGING_PREFIX)))
    })
}
