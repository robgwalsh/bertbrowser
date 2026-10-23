//! Comparing two listings of the same folder, so a refresh can change what actually changed.
//! Spec: Core/Services/FileListDiff.cs.
//!
//! This exists because of what the alternative costs. The file list replaces its whole collection
//! on a load, and the view focuses the list when that happens: fine for a navigation the user asked
//! for, ruinous for a refresh nobody asked for, which would drop the selection, jump the scroll
//! position back to the top and pull the caret out of another pane's search box every time a file
//! appeared on disk. Pure, and keyed on `PathKey::canonicalize` so the comparison agrees with
//! everything else in the app about what "the same path" means.

use std::collections::{HashMap, HashSet};

use bb_paths::{PathError, PathKey};

use crate::attributes::{
    ARCHIVE, COMPRESSED, DIRECTORY, ENCRYPTED, HIDDEN, READONLY, REPARSE_POINT, SYSTEM,
};
use crate::models::FileEntry;

/// What changed in a folder between two listings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileListChanges {
    /// Canonical path keys that are no longer there.
    pub removed: Vec<String>,
    /// Entries that were not there before.
    pub added: Vec<FileEntry>,
    /// Entries at the same path whose displayed details differ: a size, a timestamp, an attribute,
    /// or the casing of the name.
    pub updated: Vec<FileEntry>,
}

impl FileListChanges {
    pub fn any(&self) -> bool {
        !self.removed.is_empty() || !self.added.is_empty() || !self.updated.is_empty()
    }

    pub fn none() -> FileListChanges {
        FileListChanges::default()
    }
}

pub struct FileListDiff;

impl FileListDiff {
    pub fn compute(
        current: &[FileEntry],
        next: &[FileEntry],
    ) -> Result<FileListChanges, PathError> {
        let mut before: HashMap<String, &FileEntry> = HashMap::with_capacity(current.len());
        for entry in current {
            before.insert(PathKey::canonicalize(&entry.full_path)?, entry);
        }

        let mut added = Vec::new();
        let mut updated = Vec::new();
        let mut seen: HashSet<String> = HashSet::with_capacity(next.len());

        for entry in next {
            let key = PathKey::canonicalize(&entry.full_path)?;
            if !seen.insert(key.clone()) {
                continue; // a listing cannot hold one path twice; ignore if it does
            }
            match before.get(&key) {
                None => added.push(entry.clone()),
                Some(existing) => {
                    if differs(existing, entry) {
                        updated.push(entry.clone());
                    }
                }
            }
        }

        let removed: Vec<String> = before
            .into_keys()
            .filter(|key| !seen.contains(key))
            .collect();

        Ok(FileListChanges {
            removed,
            added,
            updated,
        })
    }
}

/// The attributes a row can render and that only change when something really happened to the
/// file. The cloud-provider bits move on files nobody touched, and `Normal` is only meaningful in
/// isolation, so a lister reporting it one pass and `Archive` the next would look like a change.
/// `Archive` itself stays in: a write that sets it moves the modified time too and it costs
/// nothing, while an `attrib +a` on its own is a real change to a real column.
const MEANINGFUL_ATTRIBUTES: u32 =
    READONLY | HIDDEN | SYSTEM | DIRECTORY | ARCHIVE | COMPRESSED | ENCRYPTED | REPARSE_POINT;

/// Whether anything the row shows has changed. The name is compared **ordinally**, and that is the
/// interesting case: path keys are uppercased, so a rename that only changes casing is the same key
/// and would otherwise register as no change at all. **The access time is deliberately not
/// compared**, though a column may render it: reading a file moves it, and this app reads files
/// constantly (the preview pane, content search, the shell property reads behind a metadata
/// column), so weighing it would mark every row the user merely looked at as changed.
fn differs(a: &FileEntry, b: &FileEntry) -> bool {
    a.name != b.name
        || a.size_bytes != b.size_bytes
        || a.modified_utc != b.modified_utc
        || a.created_utc != b.created_utc
        || a.is_directory != b.is_directory
        || (a.attributes & MEANINGFUL_ATTRIBUTES) != (b.attributes & MEANINGFUL_ATTRIBUTES)
}
