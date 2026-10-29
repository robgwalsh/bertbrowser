//! The file-system services: listing a folder, walking a subtree, the crawler and the watcher that
//! keep `fs_entry` fresh, the search orchestration over them, and the small rules every executor
//! shares (opening a file read-only, swapping a rewrite in, erasing a tree, telling a refusal from
//! a failure, stamping an entry, diffing two listings). Spec: Core/Services/{FileSystemService,
//! FileSystemWalker,IndexCrawler,IndexWatcherService,SearchService,ReadOnlyFile,StagedReplace,
//! DirectoryRemoval,AccessDenied,EntryStamp,FileListDiff}.cs.

mod access_denied;
mod directory_removal;
mod entry_stamp;
mod file_list_diff;
mod file_system_service;
mod index_crawler;
mod index_watcher;
mod read_only_file;
mod search_service;
mod staged_replace;
mod walker;

pub use access_denied::AccessDenied;
pub use directory_removal::DirectoryRemoval;
pub use entry_stamp::EntryStamp;
pub use file_list_diff::{FileListChanges, FileListDiff};
pub use file_system_service::{
    DriveInfo, DriveType, FileSystemService, LocalFileSystem, SubdirectoryPresence,
};
pub use index_crawler::IndexCrawler;
pub use index_watcher::{ChangeApplier, FsChange, FsChangeKind, IndexWatcher, IndexWatcherService};
pub use read_only_file::ReadOnlyFile;
pub use search_service::{SearchProgress, SearchService, is_held_path};
pub use staged_replace::StagedReplace;
pub use walker::{FileSystemWalker, WalkEntry, WalkError};
