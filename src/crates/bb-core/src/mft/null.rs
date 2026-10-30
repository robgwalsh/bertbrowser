//! An index that indexes nothing and reports nothing. The UI harness must never raise an elevation
//! prompt (a UAC dialog is the one thing offscreen cannot fix), and tests want the crawl and
//! live-scan fallbacks exercised without a volume to index. Reporting nothing indexed is the honest
//! answer: it is exactly what a non-NTFS machine, a still-indexing one and a declined prompt all
//! look like, so every caller downstream already handles it. Spec:
//! Core/Services/Mft/NullMftIndexService.cs.

use std::sync::Arc;

use super::{IndexListener, IndexStartMode, IndexerPresence, MftIndex};
use crate::changes::ChangeLogPolicy;

/// A unit struct, so `Arc::new(NullMftIndex)` reads as the harness's one line. The policy it is
/// handed goes nowhere, since no tail reads it.
#[derive(Default)]
pub struct NullMftIndex;

impl MftIndex for NullMftIndex {
    fn start_with(&self, _mode: IndexStartMode) -> Result<(), String> {
        Ok(())
    }

    fn stop(&self) {}

    fn any_indexed(&self) -> bool {
        false
    }

    fn is_building(&self) -> bool {
        false
    }

    fn building_drives(&self) -> Vec<String> {
        Vec::new()
    }

    fn completed_roots(&self) -> Vec<String> {
        Vec::new()
    }

    /// Not `NotRunning`: this host was never going to have a helper, and saying one is missing
    /// would raise the "start the indexer" banner over every harness run.
    fn presence(&self) -> IndexerPresence {
        IndexerPresence::NotApplicable
    }

    fn is_indexed(&self, _path_key: &str) -> bool {
        false
    }

    fn status_text(&self) -> String {
        String::new()
    }

    fn can_retry(&self) -> bool {
        false
    }

    fn can_start(&self) -> bool {
        false
    }

    fn retry(&self) {}

    fn change_log(&self) -> ChangeLogPolicy {
        ChangeLogPolicy::OFF
    }

    fn set_change_log(&self, _policy: ChangeLogPolicy) {}

    fn subscribe(&self, _listener: Arc<dyn IndexListener>) -> u64 {
        0
    }

    fn unsubscribe(&self, _id: u64) {}
}
