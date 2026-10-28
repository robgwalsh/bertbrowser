//! Disk usage: the verdict rules, the queries over the index and `dir_size_cache`, the squarified
//! treemap geometry and the treemap's palette. Every number comes from what the MFT pass wrote;
//! an unmeasured folder is `None`, never zero, all the way to the view.
//! Spec: src/BertBrowser.Core/Services/DiskUsage/*, Core/Models/DiskUsageModels.cs.

mod models;
mod rules;
mod service;
mod treemap_layout;
mod treemap_palette;

pub use models::{DiskUsageAvailability, DiskUsageBreakdown, DiskUsageNode, LargestFilesOutcome};
pub use rules::DiskUsageRules;
pub use service::{
    ArchiveEntries, ArchiveFileSize, DiskUsageArchives, DiskUsageError, DiskUsageIndex,
    DiskUsageService,
};
pub use treemap_layout::{TreemapLayout, TreemapRect};
pub use treemap_palette::TreemapPalette;
