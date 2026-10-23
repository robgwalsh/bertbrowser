//! One entry in a directory listing. Spec: Core/Models/FileEntry.cs.

use crate::FileTime;

/// One entry in a directory listing.
///
/// `created_utc` and `accessed_utc` come out of the same find data as the rest, so the Created and
/// Accessed columns cost no extra I/O. They default to **unknown** (zero), which renders blank and
/// never as a date that looks like data: an archive entry and a search hit have neither.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub full_path: String,
    pub is_directory: bool,
    /// -1 for a directory in a listing: its contents are not measured here.
    pub size_bytes: i64,
    pub modified_utc: FileTime,
    /// The entry's own Win32 attribute mask.
    pub attributes: u32,
    pub created_utc: FileTime,
    pub accessed_utc: FileTime,
}

impl FileEntry {
    /// An entry whose creation and access times are unknown.
    pub fn new(
        name: &str,
        full_path: &str,
        is_directory: bool,
        size_bytes: i64,
        modified_utc: FileTime,
        attributes: u32,
    ) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            full_path: full_path.to_string(),
            is_directory,
            size_bytes,
            modified_utc,
            attributes,
            created_utc: FileTime::UNKNOWN,
            accessed_utc: FileTime::UNKNOWN,
        }
    }
}
