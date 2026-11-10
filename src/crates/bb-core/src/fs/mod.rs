//! The file-system services: listing a folder, walking a subtree, the crawler and the watcher that
//! keep `fs_entry` fresh, the search orchestration over them, and the small rules every executor
//! shares (opening a file read-only, swapping a rewrite in, erasing a tree, telling a refusal from
//! a failure, stamping an entry, diffing two listings). Spec: Core/Services/{FileSystemService,
//! FileSystemWalker,IndexCrawler,IndexWatcherService,SearchService,ReadOnlyFile,StagedReplace,
//! DirectoryRemoval,AccessDenied,EntryStamp,FileListDiff}.cs.

mod index_crawler;
mod index_watcher;
mod search_service;

pub use index_crawler::IndexCrawler;
pub use index_watcher::{ChangeApplier, FsChange, FsChangeKind, IndexWatcher, IndexWatcherService};
pub use search_service::{SearchProgress, SearchService, is_held_path};
// Listing, walking and the executors' shared rules are bb-files'; re-exported so this module stays the one door.
pub use bb_files::fs::*;
