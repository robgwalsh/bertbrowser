//! How much a record holds, for the history's byte budget.
//! Spec: Core/Services/UndoHistory/HeldSizeEstimator.cs.
//!
//! A file is one stat, and exact. A folder comes from the size index first, by the path it had
//! before it was set aside; when the index does not know it, the held copy itself is walked (it is
//! this app's own hidden folder, so nothing else is racing the walk) but only so far. The walk has
//! an entry cap and a time cap, and a walk that hits either says so, so the total is a floor rather
//! than a guess.
//!
//! This only ever decides when an old entry is released, never what anybody is shown as a folder's
//! size, which is why a walk is acceptable here where it is not in a listing.

use std::os::windows::fs::MetadataExt;
use std::time::{Duration, Instant};

use super::models::{HeldEntry, HeldSize};
use super::record::UndoableRecord;
use crate::attributes::DIRECTORY;
use crate::fs::DirectoryRemoval;

/// Sizes of folders the index already knows, by their original path: one primary-key lookup, the
/// same kind of answer a transfer's estimate takes.
pub trait HeldFolderSizes {
    /// The folder's recursive size, or None when the index has no complete answer.
    fn of(&self, original_path: &str) -> Option<i64>;
}

pub struct HeldSizeEstimator<'a> {
    index: Option<&'a dyn HeldFolderSizes>,
}

impl<'a> HeldSizeEstimator<'a> {
    pub const WALK_ENTRY_CAP: usize = 50_000;
    pub const WALK_TIME_CAP: Duration = Duration::from_secs(1);

    pub fn new(index: Option<&'a dyn HeldFolderSizes>) -> HeldSizeEstimator<'a> {
        HeldSizeEstimator { index }
    }

    pub fn measure_record(&self, record: &dyn UndoableRecord) -> HeldSize {
        record
            .held()
            .iter()
            .fold(HeldSize::NONE, |sum, held| sum + self.measure(held))
    }

    pub fn measure(&self, held: &HeldEntry) -> HeldSize {
        if !held.is_directory {
            return match std::fs::metadata(&held.held_path) {
                Ok(meta) if meta.file_attributes() & DIRECTORY == 0 => {
                    HeldSize::new(meta.file_size() as i64, true)
                }
                Ok(_) => HeldSize::NONE,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => HeldSize::NONE,
                Err(_) => HeldSize::new(0, false),
            };
        }

        if let Some(known) = self.index.and_then(|index| index.of(&held.original_path)) {
            return HeldSize::new(known, true);
        }
        Self::walk(&held.held_path)
    }

    /// A bounded, link-safe walk: a junction inside a held folder is counted as itself, never
    /// followed into whatever it points at.
    fn walk(root: &str) -> HeldSize {
        match std::fs::symlink_metadata(root) {
            Ok(meta) if meta.file_attributes() & DIRECTORY != 0 => {}
            Ok(_) => return HeldSize::NONE,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return HeldSize::NONE,
            Err(_) => return HeldSize::new(0, false),
        }

        let deadline = Instant::now() + Self::WALK_TIME_CAP;
        let mut bytes = 0i64;
        let mut seen = 0usize;
        let mut complete = true;
        let mut pending = vec![root.to_string()];

        while let Some(current) = pending.pop() {
            if seen >= Self::WALK_ENTRY_CAP || Instant::now() > deadline {
                return HeldSize::new(bytes, false);
            }

            let entries = match std::fs::read_dir(&current) {
                Ok(entries) => entries,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            for entry in entries {
                let Ok(entry) = entry else {
                    complete = false;
                    continue;
                };
                seen += 1;
                let Ok(meta) = entry.metadata() else {
                    complete = false;
                    continue;
                };
                if DirectoryRemoval::is_link(&meta) {
                    continue;
                }
                if meta.file_attributes() & DIRECTORY != 0 {
                    pending.push(entry.path().to_string_lossy().into_owned());
                } else {
                    bytes += meta.file_size() as i64;
                }
            }
        }

        HeldSize::new(bytes, complete)
    }
}
