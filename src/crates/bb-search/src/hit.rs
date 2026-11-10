//! One search result row, as both the index and a live scan produce it. Spec: Core/Models/SearchHit.cs.

use bb_base::FileTime;

use crate::ContentMatch;

/// One search result. `relative_dir_display` is the parent directory relative to the search root
/// ("" for direct children), display-cased; for a whole-PC search it is the full parent path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub display_path: String,
    pub relative_dir_display: String,
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: i64,
    pub modified_utc: FileTime,
    pub hidden: bool,
    /// Zero means not recorded rather than none.
    pub attributes: u32,
    pub created_utc: FileTime,
    /// Where a `content:` term found its needle, when one did.
    pub content_match: Option<ContentMatch>,
}
