//! Enough about an entry on disk to tell whether it is still the one something wrote.
//! Spec: Core/Services/EntryStamp.cs.
//!
//! The undo history needs it where an undo would otherwise act on whatever happens to be at a path
//! now rather than on what it put there: removing a pasted file, and swapping an edited archive for
//! its original. **Creation time is part of it.** A copy keeps its source's last-write time, so an
//! original restored under a copy's name can match on length and last write; a creation time is set
//! when the entry is made. Not infallibly (NTFS tunnels a creation time onto a name reused within
//! seconds, which is exactly what an Overwrite does), so the stamp is a second guard, not the only
//! one.

use std::os::windows::fs::MetadataExt;

use crate::FileTime;
use crate::attributes::DIRECTORY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryStamp {
    /// -1 for a folder: its contents are not measured, only its identity.
    pub length: i64,
    pub write_utc: FileTime,
    pub created_utc: FileTime,
}

impl EntryStamp {
    /// The stamp of whatever is at `path`, or None when nothing is.
    pub fn of(path: &str) -> Option<EntryStamp> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        Some(EntryStamp {
            length: if meta.file_attributes() & DIRECTORY != 0 {
                -1
            } else {
                meta.file_size() as i64
            },
            write_utc: FileTime(meta.last_write_time() as i64),
            created_utc: FileTime(meta.creation_time() as i64),
        })
    }

    /// Whether what is at `path` is still the entry this stamp was taken of. A folder is compared by
    /// creation time alone, since writing inside it moves its last-write time without making it a
    /// different folder.
    pub fn matches(&self, path: &str) -> bool {
        match Self::of(path) {
            Some(now) => {
                now.length == self.length
                    && now.created_utc == self.created_utc
                    && (self.length < 0 || now.write_utc == self.write_utc)
            }
            None => false,
        }
    }
}
