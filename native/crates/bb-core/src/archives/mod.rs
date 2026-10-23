//! Archives browsed as folders: the suffix table and the virtual-path parser. The readers, the
//! extract/create/edit planners and executors arrive with M4. Spec: Core/Services/Archives/*.

mod archive_path;
mod browser;
mod formats;

pub use archive_path::ArchivePath;
pub use browser::ArchiveBrowser;
pub use formats::{ArchiveContainer, ArchiveFormat, ArchiveFormats};
