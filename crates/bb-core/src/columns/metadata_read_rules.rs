//! Whether a metadata column may open a file at all.
//!
//! The most dangerous rule in the columns feature, so it lives here rather than inside the shell
//! layer that enforces it — pure over attribute bits, where tests can hold it still. The read
//! itself is `SHGetPropertyStoreFromParsingName` with `GPS_OPENSLOWITEM`, which genuinely opens the
//! file so that EXIF and ID3 handlers run.
//! Spec: Core/Services/Columns/MetadataReadRules.cs.

use crate::attributes;

use super::preview_metadata::is_cloud_placeholder;

pub struct MetadataReadRules;

impl MetadataReadRules {
    /// Whether the file's properties may be read.
    ///
    /// **A cloud placeholder is refused, and that is what this rule is for.** Opening one makes the
    /// sync provider fetch its content — so a Dimensions column over a synced photo folder would
    /// download the entire folder, one row at a time, as the user scrolled past it, with no
    /// progress and nothing asked. The preview pane refuses these for the same reason through the
    /// same predicate, so the two cannot drift apart.
    ///
    /// A reparse point is refused rather than followed. A directory is skipped because there is
    /// nothing there worth the open. Both render blank, which is what unknown looks like everywhere
    /// else in this app.
    pub fn may_read(attributes: u32) -> bool {
        attributes & attributes::DIRECTORY == 0
            && attributes & attributes::REPARSE_POINT == 0
            && !is_cloud_placeholder(attributes)
    }
}
