//! Everything that was BertBrowser.Core plus the settings model. Windows APIs sit behind probe traits so
//! the planners stay testable without a disk. Spec: src/BertBrowser.Core/**.

pub mod archives;
pub mod changes;
pub mod checksums;
pub mod columns;
pub mod commandbars;
pub mod commands;
pub mod compare;
pub mod data;
pub mod delete;
pub mod diff;
pub mod diskusage;
pub mod duplicates;
pub mod elevation;
pub mod fs;
pub mod ipc;
pub mod layout;
pub mod mft;
pub mod models;
pub mod newitem;
pub mod rename;
pub mod rules;
pub mod search;
pub mod settings;
pub mod shortcuts;
pub mod timestamps;
pub mod transfer;
pub mod undo;
pub mod updates;

// The shared vocabulary lives in bb-base; re-exported at its old paths so nothing above notices.
pub use bb_base::{attributes, cancel, cli, format, time};
pub use bb_metadata as metadata;
pub use bb_paths as paths;
pub use bb_preview as preview;
pub use time::FileTime;
