//! Opening a file to read its bytes, on this app's terms. Spec: Core/Services/ReadOnlyFile.cs.
//!
//! One place, because the terms are a cross-cutting rule rather than a preference. Every read
//! shares `ReadWrite | Delete`, or this app's own rename, move and delete block themselves on a
//! file it is merely looking at. Cloud placeholders are refused rather than silently hydrated,
//! because reading one is a multi-gigabyte download nobody asked for. A reparse point is the entry
//! it is, not the file it points at. Three copies of this rule would be three chances for one to
//! drift, and it fails silently in both directions: too narrow and the app fights itself, too wide
//! and a preview starts a download.

use std::fs::{File, OpenOptions};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use crate::attributes::{DIRECTORY, OFFLINE, RECALL_ON_DATA_ACCESS, RECALL_ON_OPEN, REPARSE_POINT};

pub struct ReadOnlyFile;

impl ReadOnlyFile {
    /// A cloud file whose bytes are not on this machine.
    pub const PLACEHOLDER: u32 = OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS;

    /// Opens `path` for a sequential read, or returns None when the file cannot be read, or must
    /// not be. That is never an error on its own: one unreadable file is that file's problem and
    /// the caller carries on with the rest, as every other multi-item operation in this app does.
    /// `follow_reparse_points` false (every current caller) refuses a link, since anything
    /// counting or comparing files would read one file twice under two names.
    pub fn try_open(path: &str, follow_reparse_points: bool) -> Option<File> {
        // The entry's own attributes, not its target's.
        let meta = std::fs::symlink_metadata(path).ok()?;
        let attributes = meta.file_attributes();
        // A folder is not a file to read (`FileInfo.Exists` was false for one), junction or not.
        if attributes & DIRECTORY != 0 {
            return None;
        }
        if !follow_reparse_points && attributes & REPARSE_POINT != 0 {
            return None;
        }
        if attributes & Self::PLACEHOLDER != 0 {
            return None;
        }

        // Callers read in large blocks of their own; `File` adds no buffering underneath.
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
            .open(path)
            .ok()
    }
}
