//! The questions about archives that the listing seam cannot express. Spec: `IArchiveBrowser` in
//! Core/Services/Archives/ArchiveAwareFileSystemService.cs, plus the `in:archives` second pass of
//! `ArchiveSearchScanner.Search` that the search service takes.
//!
//! Separate from `FileSystemService` on purpose: that trait is about directories and gains
//! nothing by learning what an archive is, and the callers of it must stay unable to tell the
//! difference. This is what navigation, the preview pane and the context-menu guards ask instead,
//! implemented by the same object, so the two views cannot disagree.

use std::sync::Arc;

use super::{ArchiveIndex, ArchivePath};
use crate::cancel::CancelToken;
use crate::data::SearchHit;
use crate::search::SearchQuery;

pub trait ArchiveBrowser: Send + Sync {
    /// A real directory, or somewhere inside a readable archive. Opens the container, so it
    /// belongs on a worker thread.
    fn can_list(&self, path: &str) -> bool;

    /// Whether this path is inside an archive, and which one.
    fn resolve(&self, path: &str) -> Option<ArchivePath>;

    /// The container's index, cached.
    fn read_archive(&self, archive_file: &str) -> Arc<ArchiveIndex>;

    /// Reads the container's index (cached) and searches its entry names with `query`, returning at
    /// most `limit` hits whose display paths are virtual paths inside `archive_file`. A container
    /// that cannot be read yields nothing.
    fn search_archive(
        &self,
        archive_file: &str,
        query: &SearchQuery,
        limit: usize,
        cancel: &CancelToken,
    ) -> Vec<SearchHit>;
}
