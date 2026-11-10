//! The index: the SQLite database and its repositories, the MFT reader and the elevated helper's
//! side of it, the pipe between app and helper, the change log, saved layouts and their rules.
//! Needs nothing of the file operations or archives, so the elevated bb-indexer links this and
//! bb-base alone, and an edit to a transfer or an archive reader leaves it alone. bb-core
//! re-exports each module at its old path (`bb_core::data`).

pub mod changes;
pub mod columns;
pub mod data;
pub mod ipc;
pub mod layout;
pub mod mft;
pub mod rules;
