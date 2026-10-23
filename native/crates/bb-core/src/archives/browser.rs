//! The archive browser as search sees it: the `in:archives` second pass opens the containers the
//! walk found and searches their entry names. Spec: `IArchiveBrowser` in
//! Core/Services/Archives/ArchiveAwareFileSystemService.cs and `ArchiveSearchScanner.Search`.
//!
//! Only the slice search needs is declared here, so that the search service can take
//! `Option<&dyn ArchiveBrowser>` without an `ArchiveIndex` type existing yet.
// TODO(port): implemented at M4, when the readers and `ArchiveIndex` arrive; the listing half of
// `IArchiveBrowser` (`CanList`, `Resolve`, `ReadArchive`) is added then.

use crate::cancel::CancelToken;
use crate::data::SearchHit;
use crate::search::SearchQuery;

pub trait ArchiveBrowser: Send + Sync {
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
