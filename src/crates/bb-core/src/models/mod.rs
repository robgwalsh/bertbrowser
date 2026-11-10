//! The listing and search models that cross between the file-system services and the UI: a
//! directory entry, a search's outcome and what a content scan did. Spec: Core/Models/{FileEntry,
//! ContentScanModels,SearchOutcome,DirSizeResult}.cs. (The index rows and `SearchHit` live in
//! `crate::data`, beside the repository that reads them.)

mod dir_size_result;

pub use bb_files::models::FileEntry;
pub use bb_search::{ContentScanLimit, ContentScanProgress, ContentScanReport};
pub use bb_search::{SearchOutcome, SearchResultSource};
pub use dir_size_result::DirSizeResult;

pub use crate::data::SearchHit;
pub use crate::search::ContentMatch;
