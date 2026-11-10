//! The small rules every executor shares: opening a file read-only, swapping a rewrite in, telling a
//! refusal from a failure, erasing a tree, stamping an entry. `bb_core::fs` re-exports them beside the services.

mod access_denied;
mod directory_removal;
mod entry_stamp;
mod read_only_file;
mod staged_replace;

pub use access_denied::AccessDenied;
pub use directory_removal::DirectoryRemoval;
pub use entry_stamp::EntryStamp;
pub use read_only_file::ReadOnlyFile;
pub use staged_replace::StagedReplace;
