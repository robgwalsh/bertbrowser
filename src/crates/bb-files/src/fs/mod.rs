//! Listing a folder, walking a subtree, diffing two listings, the known folders - plus bb-base's
//! small rules every executor shares, re-exported so `crate::fs::AccessDenied` reads as it always did.
//! The index-backed services (crawler, watcher, search) are bb-core's `fs`, which re-exports all of
//! this. Spec: Core/Services/{FileSystemService,FileSystemWalker,FileListDiff}.cs.

mod file_list_diff;
mod file_system_service;
mod known_folders;
mod walker;

pub use bb_base::fs::{AccessDenied, DirectoryRemoval, EntryStamp, ReadOnlyFile, StagedReplace};
pub use file_list_diff::{FileListChanges, FileListDiff};
pub use file_system_service::{
    DriveInfo, DriveType, FileSystemService, LocalFileSystem, SubdirectoryPresence,
};
pub use known_folders::KnownFolders;
pub use walker::{FileSystemWalker, WalkEntry, WalkError};
