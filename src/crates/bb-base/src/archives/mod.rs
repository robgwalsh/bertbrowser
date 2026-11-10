//! The archive suffix table and the virtual-path syntax (`C:\x\a.zip\src`), which half the crates
//! above need without needing an archive reader. `bb_core::archives` re-exports both.

mod archive_path;
mod formats;

pub use archive_path::ArchivePath;
pub use formats::{ArchiveContainer, ArchiveFormat, ArchiveFormats};
