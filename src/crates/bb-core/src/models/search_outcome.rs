//! The final result of a search. Spec: Core/Models/SearchOutcome.cs.

use super::{ContentScanReport, SearchHit};

/// Where a search's results came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchResultSource {
    /// Streamed from a live filesystem scan (subtree not indexed yet).
    LiveScan,
    /// Served from a fresh index.
    Index,
    /// Served from an index that is being re-crawled in the background.
    StaleIndex,
}

/// Final result of a search. When `refresh_pending` is true a background (re)crawl was started and
/// the service's index-refreshed event will fire for the covering root once it completes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchOutcome {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
    pub source: SearchResultSource,
    pub refresh_pending: bool,
    /// Why the query could not be used, in words for the user. An outcome carrying one has no hits
    /// and is *not* a search that found nothing: the caller shows the message and leaves the
    /// previous results on screen, rather than reporting "no results" for a query that never ran.
    pub problem: Option<String>,
    /// The query filtered on size or modified time, found nothing, and the index has no lengths in
    /// scope to filter on (the sizeless names-only build). Unanswerable rather than unmatched.
    pub scope_lacks_metadata: bool,
    /// The user stopped the search; `hits` is then a floor. Kept apart from the content report's
    /// own `incomplete`, or a cancelled run looks like a disk full of broken files.
    pub cancelled: bool,
    /// What the `content:` reading pass did, or None when the query had no content term, which is
    /// also how a caller knows whether to show the Match column at all.
    pub content_scan: Option<ContentScanReport>,
}

impl SearchOutcome {
    pub fn new(
        hits: Vec<SearchHit>,
        truncated: bool,
        source: SearchResultSource,
        refresh_pending: bool,
    ) -> SearchOutcome {
        SearchOutcome {
            hits,
            truncated,
            source,
            refresh_pending,
            problem: None,
            scope_lacks_metadata: false,
            cancelled: false,
            content_scan: None,
        }
    }

    /// An outcome with no hits and a message: the query never ran.
    pub fn refused(problem: &str, source: SearchResultSource) -> SearchOutcome {
        let mut outcome = SearchOutcome::new(Vec::new(), false, source, false);
        outcome.problem = Some(problem.to_string());
        outcome
    }
}
