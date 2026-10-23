//! The listing and search models that cross between the file-system services and the UI: a
//! directory entry, a search's outcome and what a content scan did. Spec: Core/Models/{FileEntry,
//! ContentScanModels,SearchOutcome,DirSizeResult}.cs. (The index rows and `SearchHit` live in
//! `crate::data`, beside the repository that reads them.)

mod content_scan;
mod dir_size_result;
mod file_entry;
mod search_outcome;

pub use content_scan::{ContentScanLimit, ContentScanProgress, ContentScanReport};
pub use dir_size_result::DirSizeResult;
pub use file_entry::FileEntry;
pub use search_outcome::{SearchOutcome, SearchResultSource};

pub use crate::data::SearchHit;
pub use crate::search::ContentMatch;
