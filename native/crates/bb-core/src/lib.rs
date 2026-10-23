//! Everything that was BertBrowser.Core plus the settings model. Windows APIs sit behind probe traits so
//! the planners stay testable without a disk. Spec: src/BertBrowser.Core/**.

pub mod archives;
pub mod attributes;
pub mod benchmarking;
pub mod cancel;
pub mod changes;
pub mod cli;
pub mod columns;
pub mod commandbars;
pub mod commands;
pub mod custom_commands;
pub mod data;
pub mod diskusage;
pub mod format;
pub mod fs;
pub mod ipc;
pub mod launch;
pub mod layout;
pub mod mft;
pub mod models;
pub mod rules;
pub mod search;
pub mod settings;
pub mod shell_integration;
pub mod shellmenu;
pub mod theming;
pub mod time;

pub use bb_paths as paths;
pub use time::FileTime;
