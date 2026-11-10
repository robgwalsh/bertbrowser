//! The vocabulary every other crate speaks: timestamps, attributes, cancellation, the command line,
//! the archive-path syntax, the few file-system rules every executor shares, and transfer-style
//! progress. Nothing here knows about a feature; `bb-core` re-exports each module at its old path, so
//! `bb_core::fs::EntryStamp` and `crate::cancel` inside it keep working.

pub mod archives;
pub mod attributes;
pub mod cancel;
pub mod cli;
pub mod format;
pub mod fs;
pub mod path_parts;
pub mod progress;
pub mod time;

pub use time::FileTime;
