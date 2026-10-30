//! Archives browsed as folders: the suffix table, the virtual-path parser, the readers, the tree
//! an index builds, the cache, searching inside one, and the extract/create/edit planners and
//! executors. Spec: Core/Services/Archives/* and Core/Services/Preview/ArchiveListing.cs.

mod archive_path;
mod aware;
mod browser;
mod cache;
mod creator;
mod edit_executor;
mod edit_models;
mod edit_planner;
mod extract_executor;
mod extract_models;
mod extract_planner;
mod extract_rules;
mod formats;
mod index_builder;
mod listing;
mod models;
mod probes;
mod reader;
mod search_scanner;
mod source_walk;
mod write_rules;
mod writer;

pub use archive_path::ArchivePath;
pub use aware::{
    ArchiveAwareFileSystemService, ArchiveLockedError, ArchivePasswords, NoArchivePasswords,
};
pub use browser::ArchiveBrowser;
pub use cache::ArchiveCache;
pub use creator::{ArchiveCreator, CreateArchiveOutcome};
pub use edit_executor::ArchiveEditExecutor;
pub use edit_models::{
    AddFile, ArchiveEdit, ArchiveEditOutcome, ArchiveEditPlan, ArchiveEditRejection,
    ArchiveEditUndo, RejectedArchiveEdit,
};
pub use edit_planner::ArchiveEditPlanner;
pub use extract_executor::ExtractExecutor;
pub use extract_models::{
    ExtractConflict, ExtractOutcome, ExtractPlan, ExtractRejection, FailedExtraction,
    PlannedExtraction, RejectedExtraction,
};
pub use extract_planner::{ExtractPlanner, ExtractProbe, FileSystemExtractProbe};
pub use extract_rules::ExtractRules;
pub use formats::{ArchiveContainer, ArchiveFormat, ArchiveFormats};
pub use index_builder::ArchiveIndexBuilder;
pub use listing::{ArchiveContents, ArchiveEntry, ArchiveListing};
pub use models::{ArchiveCapabilities, ArchiveFailure, ArchiveIndex, ArchiveNode, RawArchiveEntry};
pub use probes::ArchiveRenameProbe;
pub use reader::{ArchiveReadError, ArchiveReader, EntrySink, LibraryArchiveReader};
pub use search_scanner::ArchiveSearchScanner;
pub use source_walk::{ArchiveSource, ArchiveSourceWalk};
pub use write_rules::{ArchiveWriteFormat, ArchiveWriteRules, CompressionLevel, WriteFormatInfo};
pub use writer::{ArchiveType, ArchiveWriter, CompressionType, WriterOptions};
