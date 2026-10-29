//! The filesystem questions the planner and the merge expander ask, behind traits so the
//! containment rules can be tested against link layouts that are impractical to create on a real
//! disk. Spec: Core/Services/Transfer/ITransferProbe.cs.

use std::os::windows::fs::MetadataExt;

use windows_sys::Win32::Storage::FileSystem::{
    GetFileAttributesExW, GetFileAttributesW, GetFileExInfoStandard, INVALID_FILE_ATTRIBUTES,
    WIN32_FILE_ATTRIBUTE_DATA,
};

use crate::FileTime;
use crate::attributes::{DIRECTORY, REPARSE_POINT};

/// One immediate child of a directory, as the enumeration that found it already described it.
/// `is_reparse_point` is carried because it is the one attribute that decides whether a merge may
/// descend; `size_bytes` is 0 for a directory, whose size is never read from here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferEntry {
    pub name: String,
    pub full_path: String,
    pub is_directory: bool,
    pub is_reparse_point: bool,
    pub size_bytes: i64,
    pub modified_utc: FileTime,
}

/// What is at a path, as the planner and executor care: nothing, a file, or a directory (a junction
/// is a directory whether or not its target still exists).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Missing,
    File,
    Directory,
}

/// What `TransferPlanner` needs answered. `Send + Sync` because a run is driven from a worker
/// thread while the UI holds the same executor.
pub trait TransferProbe: Send + Sync {
    fn directory_exists(&self, path: &str) -> bool;

    fn file_exists(&self, path: &str) -> bool;

    /// Whether anything is there: a file or a directory. One question, so a probe that can answer it
    /// with one call to the disk does.
    fn exists(&self, path: &str) -> bool {
        self.file_exists(path) || self.directory_exists(path)
    }

    /// What is there, asked once rather than as `directory_exists` then `file_exists`.
    fn kind(&self, path: &str) -> EntryKind {
        if self.directory_exists(path) {
            EntryKind::Directory
        } else if self.file_exists(path) {
            EntryKind::File
        } else {
            EntryKind::Missing
        }
    }

    /// The physical path with every symlink/junction resolved, including ones part-way along the
    /// path (`C:\link\sub` where `link` is a junction). Returns the fully-qualified input when
    /// nothing resolves, or when resolution fails: callers must treat the result as a best effort
    /// and keep their literal-path checks as well.
    fn resolve_final_path(&self, path: &str) -> String;
}

/// A probe that can also list a directory's immediate children. **Separate from `TransferProbe`
/// on purpose**: the planner keeps taking the narrower trait and therefore *cannot* enumerate,
/// which matters because a plan is built on every drag-over. The merge expander takes this one,
/// and runs only at drop time.
pub trait TransferEntrySource: TransferProbe {
    /// The directory's immediate children, **one level, never recursive**. Empty when the
    /// directory cannot be read: an unreadable folder is not a merge opportunity, and a plan that
    /// fails part-way through building is worse than one that declines to expand.
    fn entries(&self, directory: &str) -> Vec<TransferEntry>;
}

/// Real-filesystem probe.
#[derive(Default)]
pub struct FileSystemTransferProbe;

/// The attributes of the entry itself (a link is not followed), or None when nothing is there.
/// `GetFileAttributesW` rather than `symlink_metadata`: the latter opens and closes a handle, which on
/// a machine with a file-system filter (every one with Defender) was the larger part of a probe, and a
/// copy asks five of them per file.
fn attributes_of(path: &str) -> Option<u32> {
    let wide: Vec<u16> = super::copier::extended(path)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: NUL-terminated and alive for the call.
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    (attributes != INVALID_FILE_ATTRIBUTES).then_some(attributes)
}

/// The length of the file at `path`, or None for a folder or nothing. From the directory entry, like
/// `attributes_of`, with no handle opened.
pub(crate) fn file_length(path: &str) -> Option<i64> {
    let wide: Vec<u16> = super::copier::extended(path)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: an all-zero attribute record is a valid value for the call to fill in.
    let mut data: WIN32_FILE_ATTRIBUTE_DATA = unsafe { std::mem::zeroed() };
    // SAFETY: NUL-terminated path and a record of the type the info level names, both alive for the call.
    let ok = unsafe {
        GetFileAttributesExW(
            wide.as_ptr(),
            GetFileExInfoStandard,
            (&mut data as *mut WIN32_FILE_ATTRIBUTE_DATA).cast(),
        )
    };
    if ok == 0 || data.dwFileAttributes & DIRECTORY != 0 {
        return None;
    }
    Some(((i64::from(data.nFileSizeHigh)) << 32) | i64::from(data.nFileSizeLow))
}

/// Bounds the resolution loop so a cyclic junction layout cannot hang it.
const MAX_HOPS: usize = 32;

impl TransferProbe for FileSystemTransferProbe {
    /// The attribute bit of the entry itself, as `Directory.Exists` read it: a junction is a
    /// directory whether or not its target still exists.
    fn directory_exists(&self, path: &str) -> bool {
        attributes_of(path).is_some_and(|a| a & DIRECTORY != 0)
    }

    fn file_exists(&self, path: &str) -> bool {
        attributes_of(path).is_some_and(|a| a & DIRECTORY == 0)
    }

    fn exists(&self, path: &str) -> bool {
        attributes_of(path).is_some()
    }

    fn kind(&self, path: &str) -> EntryKind {
        match attributes_of(path) {
            None => EntryKind::Missing,
            Some(a) if a & DIRECTORY != 0 => EntryKind::Directory,
            Some(_) => EntryKind::File,
        }
    }

    fn resolve_final_path(&self, path: &str) -> String {
        let Ok(mut full) = bb_paths::PathKey::normalize_display(path) else {
            return path.to_string();
        };
        // A link is only resolved when it is the final component, so walk up the path looking for
        // the deepest ancestor that is one, splice in its target, and go round again: a target may
        // itself sit behind another link.
        for _ in 0..MAX_HOPS {
            match resolve_once(&full) {
                Some(resolved) => full = resolved,
                None => return full,
            }
        }
        full
    }
}

impl TransferEntrySource for FileSystemTransferProbe {
    /// One level, from the find data the enumeration already holds: name, attributes, length and
    /// last-write all come free, so listing a folder costs no stat per entry.
    fn entries(&self, directory: &str) -> Vec<TransferEntry> {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries {
            let (Ok(entry),) = (entry,) else {
                return Vec::new();
            };
            let Ok(meta) = entry.metadata() else {
                return Vec::new();
            };
            let attributes = meta.file_attributes();
            let is_directory = attributes & DIRECTORY != 0;
            out.push(TransferEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                full_path: entry.path().to_string_lossy().into_owned(),
                is_directory,
                is_reparse_point: attributes & REPARSE_POINT != 0,
                size_bytes: if is_directory {
                    0
                } else {
                    meta.file_size() as i64
                },
                modified_utc: FileTime(meta.last_write_time() as i64),
            });
        }
        out
    }
}

/// Replaces the deepest link on the path with its target. None when there is none.
fn resolve_once(full: &str) -> Option<String> {
    let mut suffix = String::new();
    let mut current = full.to_string();
    loop {
        if let Some(target) = link_target(&current)
            && !bb_paths::fold_upper(&target).eq(&bb_paths::fold_upper(&current))
        {
            return Some(if suffix.is_empty() {
                target
            } else {
                super::models::join(&target, &suffix)
            });
        }
        let name = bb_paths::file_name(&current).to_string();
        if name.is_empty() {
            return None; // reached the root
        }
        suffix = if suffix.is_empty() {
            name
        } else {
            format!("{name}\\{suffix}")
        };
        current = bb_paths::parent_key(&current)?;
    }
}

/// The final target of a link at `path`, or None when it is not one or cannot be read: an
/// unreadable link falls back to the literal path.
fn link_target(path: &str) -> Option<String> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_attributes() & REPARSE_POINT == 0 {
        return None;
    }
    // `canonicalize` is `GetFinalPathNameByHandle`: the final target, however many links deep.
    let resolved = std::fs::canonicalize(path).ok()?;
    Some(strip_extended_prefix(&resolved.to_string_lossy()))
}

/// `\\?\C:\x` back to `C:\x` and `\\?\UNC\s\x` back to `\\s\x`, as `FullName` reported them.
fn strip_extended_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}
