//! Iterative pre-order walk of a directory subtree. Spec: Core/Services/FileSystemWalker.cs.
//!
//! An explicit stack, so deep trees cannot overflow. The root itself is not emitted. Reparse-point
//! directories (junctions, symlinks) are emitted but never descended into, so cycles are
//! impossible. Inaccessible directories are skipped rather than fatal. Sizes and timestamps come
//! straight from the OS find data: no extra stat calls.

use std::os::windows::fs::MetadataExt;

use bb_paths::{PathError, PathKey, fold_upper};

use crate::FileTime;
use crate::attributes::{DIRECTORY, HIDDEN, REPARSE_POINT};
use crate::cancel::CancelToken;

/// One filesystem entry produced by the walk. `hidden` is *effective* (the entry's own Hidden
/// attribute or that of any ancestor within the walked subtree); `attributes` is the entry's own
/// mask, uninherited. `name_key` is carried rather than recomputed by consumers: the walker already
/// folds it on its way to building `path_key`, so it is free here and one allocation per entry
/// saved in the search matcher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkEntry {
    pub display_path: String,
    pub path_key: String,
    pub name: String,
    pub name_key: String,
    pub is_directory: bool,
    /// 0 for a directory.
    pub size_bytes: i64,
    pub modified_utc: FileTime,
    pub hidden: bool,
    pub attributes: u32,
    pub created_utc: FileTime,
}

/// Why a walk ended before the tree did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkError {
    Cancelled,
    /// The root could not be made into a path key.
    Path(PathError),
}

impl From<PathError> for WalkError {
    fn from(e: PathError) -> Self {
        WalkError::Path(e)
    }
}

impl std::fmt::Display for WalkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalkError::Cancelled => f.write_str("cancelled"),
            WalkError::Path(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for WalkError {}

pub struct FileSystemWalker;

impl FileSystemWalker {
    /// Walks `root`, calling `on_entry` for every entry; the callback returns false to stop.
    /// `include_hidden` false skips hidden entries and the subtrees under hidden directories
    /// entirely; true emits every entry with its effective `hidden` flag set. `root_hidden` seeds
    /// the root's hidden state, for a walked root that itself lives inside a hidden subtree
    /// (incremental mini-crawls). Stopping through the callback is `Ok`; a cancel is `Err`.
    pub fn walk(
        root: &str,
        include_hidden: bool,
        root_hidden: bool,
        cancel: &CancelToken,
        mut on_entry: impl FnMut(&WalkEntry) -> bool,
    ) -> Result<(), WalkError> {
        let mut stack: Vec<(String, String, bool)> = vec![(
            PathKey::normalize_display(root)?,
            PathKey::canonicalize(root)?,
            root_hidden,
        )];

        while let Some((dir_path, dir_key, dir_hidden)) = stack.pop() {
            if cancel.is_cancelled() {
                return Err(WalkError::Cancelled);
            }

            // Drive roots canonicalize to "C:\": avoid a doubled separator.
            let key_prefix = if dir_key.ends_with('\\') {
                dir_key.clone()
            } else {
                format!("{dir_key}\\")
            };
            let path_prefix = if dir_path.ends_with('\\') {
                dir_path.clone()
            } else {
                format!("{dir_path}\\")
            };

            // An unreadable directory is skipped, not fatal: search is best-effort like the size
            // scanner. A failure part-way through a listing abandons the rest of that folder, as
            // the enumerator's exception did.
            let Ok(entries) = std::fs::read_dir(&dir_path) else {
                continue;
            };
            for entry in entries {
                let Ok(entry) = entry else {
                    break;
                };
                let Ok(meta) = entry.metadata() else {
                    break;
                };
                let attributes = meta.file_attributes();
                // The attribute bit, as the enumerator's `IsDirectory` was: a junction is a
                // directory, where `FileType::is_dir` would call it a symlink.
                let is_dir = attributes & DIRECTORY != 0;
                let name = entry.file_name().to_string_lossy().into_owned();
                let name_key = fold_upper(&name);
                // Effective hidden: this entry, or any ancestor within the subtree.
                let hidden = dir_hidden || attributes & HIDDEN != 0;
                let walk_entry = WalkEntry {
                    display_path: format!("{path_prefix}{name}"),
                    path_key: format!("{key_prefix}{name_key}"),
                    name,
                    name_key,
                    is_directory: is_dir,
                    size_bytes: if is_dir { 0 } else { meta.file_size() as i64 },
                    modified_utc: FileTime(meta.last_write_time() as i64),
                    hidden,
                    attributes,
                    created_utc: FileTime(meta.creation_time() as i64),
                };
                let descend = is_dir && attributes & REPARSE_POINT == 0;

                if !include_hidden && walk_entry.hidden {
                    continue; // skip hidden entries and everything beneath them
                }
                if !on_entry(&walk_entry) {
                    return Ok(());
                }
                if descend {
                    stack.push((
                        walk_entry.display_path,
                        walk_entry.path_key,
                        walk_entry.hidden,
                    ));
                }
            }
        }
        Ok(())
    }
}
