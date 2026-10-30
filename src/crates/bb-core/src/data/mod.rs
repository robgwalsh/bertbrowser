//! SQLite persistence: the connection factory and migrations, and one repository per table.
//! Every path column stores `PathKey::canonicalize` output; subtree queries use `prefix_bounds`.

mod bookmark;
mod bookmark_service;
mod change_log;
mod db;
mod dir_size;
mod fs_index;
mod saved_search;
mod saved_workspace;

pub use bookmark::{Bookmark, BookmarkRepository};
pub use bookmark_service::BookmarkService;
pub use change_log::ChangeLogRepository;
pub use db::{Conn, Db, EXPECTED_SCHEMA_VERSION};
pub use dir_size::{DirSizeRepository, DirSizeResult};
pub use fs_index::{
    DuplicateShortlist, FsEntryRow, FsIndexRepository, FsIndexRoot, FsSubtreeRow, SearchHit,
};
pub use saved_search::{SavedSearch, SavedSearchRepository, SavedSearchScope};
pub use saved_workspace::{SavedWorkspace, SavedWorkspaceRepository};

/// What a repository call can fail with.
#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Cancelled,
    Path(bb_paths::PathError),
    Json(serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Sqlite(e) => write!(f, "database: {e}"),
            Error::Cancelled => write!(f, "cancelled"),
            Error::Path(e) => write!(f, "{e}"),
            Error::Json(e) => write!(f, "json: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sqlite(e)
    }
}

impl From<bb_paths::PathError> for Error {
    fn from(e: bb_paths::PathError) -> Self {
        Error::Path(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Placeholders `?1, ?2, ...` for an `IN (...)` list.
pub(crate) fn placeholders(count: usize) -> String {
    (1..=count)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Chunk size for `IN (...)` lists, well under SQLite's bound-parameter limit.
pub(crate) const IN_CHUNK: usize = 500;
