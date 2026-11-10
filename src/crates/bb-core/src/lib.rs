//! Everything that was BertBrowser.Core plus the settings model. Windows APIs sit behind probe traits so
//! the planners stay testable without a disk. Spec: src/BertBrowser.Core/**.

pub mod fs;
pub mod models;
pub mod updates;

// The shared vocabulary lives in bb-base; re-exported at its old paths so nothing above notices.
pub use bb_base::{attributes, cancel, cli, format, time};
// The crates below this one, re-exported at their old paths: files on disk one operation at a time
// (bb-files), archives browsed as folders (bb-archives), the search language (bb-search).
pub use bb_archives as archives;
// The index (bb-index): the database, the MFT reader and the helper's pipe, the change log, layouts.
pub use bb_files::{delete, newitem, rename, shortcuts, timestamps, transfer};
pub use bb_index::{changes, columns, data, ipc, layout, mft, rules};
pub use bb_metadata as metadata;
pub use bb_paths as paths;
pub use bb_preview as preview;
pub use bb_search as search;
pub use time::FileTime;
