//! One entry a query is judged against. The fields are exactly what both consumers can supply: the
//! walker's entry carries every one of them, and the index selects every one of them from `fs_entry`.
//! `name_key` and `path_key` are **already upper-cased**, so matching allocates nothing per
//! candidate. Spec: Core/Services/Search/SearchCandidate.cs.

use super::ContentText;
use crate::FileTime;

#[derive(Clone, Copy)]
pub struct SearchCandidate<'a> {
    /// The entry's name, upper-cased.
    pub name_key: &'a str,
    /// The entry's full canonical path, upper-cased.
    pub path_key: &'a str,
    pub is_directory: bool,
    /// Length in bytes; 0 for a directory and for an unmeasured row.
    pub size_bytes: i64,
    /// Last write time in UTC; unknown (zero) for a row the sizeless build wrote.
    pub modified_utc: FileTime,
    /// Effective hidden state (the entry's own, or an ancestor's).
    pub hidden: bool,
    /// The entry's own Win32 attribute mask, uninherited. Zero means nothing was recorded and reads
    /// as "no" for every attribute term rather than as an answer.
    pub attributes: u32,
    /// Creation time in UTC; unknown (zero) when never recorded.
    pub created_utc: FileTime,
    /// The file's decoded text, when it has been read. **None means "not read yet"**, which is what
    /// makes a content term answer `NeedsContent` rather than guessing. `ContentText::none()` is the
    /// different thing: read, and there was nothing to search.
    pub content: Option<&'a ContentText>,
}
