//! Measures what a [`DeletePlan`] would actually remove, so the confirmation can say "3 folders,
//! 1,204 files, 3.2 GB" rather than "3 items". Nothing here writes, and nothing here is required:
//! a delete whose survey was cancelled still deletes exactly the same items.
//! Spec: Core/Services/Delete/DeleteSurveyor.cs.
//!
//! The walk skips reparse points instead of following them — a junction is counted as the one
//! directory entry it is, because that is all deleting it removes, and following it would report
//! (and appear to threaten) a tree that lives somewhere else entirely. A folder that cannot be read
//! marks the measurement incomplete rather than failing, so the totals are honest about being a
//! floor.

use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;

use super::models::{DeleteMeasurement, DeletePlan, DeleteSurvey, PlannedDelete};
use super::probe::EntryAttributes;
use crate::attributes::{DIRECTORY, REPARSE_POINT};
use crate::cancel::CancelToken;

pub struct DeleteSurveyor;

impl DeleteSurveyor {
    /// `progress` is told each item as it finishes, so a dialog can fill in row by row instead of
    /// waiting for a large tree at the end of the list.
    pub fn survey(
        &self,
        plan: &DeletePlan,
        cancel: &CancelToken,
        progress: Option<&dyn Fn(&DeleteMeasurement)>,
    ) -> DeleteSurvey {
        let mut items = Vec::new();
        for item in &plan.deletions {
            if cancel.is_cancelled() {
                break;
            }
            let measurement = Self::measure(item, cancel);
            if let Some(report) = progress {
                report(&measurement);
            }
            items.push(measurement);
        }
        DeleteSurvey { items }
    }

    /// Measures one planned item. Never fails: an unreadable item is reported as empty and
    /// incomplete.
    pub fn measure(item: &PlannedDelete, cancel: &CancelToken) -> DeleteMeasurement {
        let path = &item.source_path;
        let measurement = |bytes, files, directories, incomplete| DeleteMeasurement {
            source_path: path.clone(),
            is_directory: item.is_directory,
            bytes,
            files,
            directories,
            incomplete,
        };

        if !item.is_directory {
            // `FileInfo.Exists` is false for a folder and for anything that cannot be read.
            return match EntryAttributes::of(path) {
                Some(entry) if !entry.is_directory() => measurement(entry.size, 1, 0, false),
                _ => measurement(0, 0, 0, true),
            };
        }

        let mut bytes = 0u64;
        let mut files = 0usize;
        let mut directories = 1usize; // the folder itself goes too
        let mut incomplete = false;

        let mut pending: Vec<PathBuf> = Vec::new();
        match EntryAttributes::of(path) {
            Some(entry) if !entry.is_directory() => return measurement(0, 0, 1, true),
            Some(entry) if entry.attributes & REPARSE_POINT != 0 => {
                return measurement(0, 0, 1, false);
            }
            Some(_) => pending.push(PathBuf::from(path)),
            None => return measurement(0, 0, 1, true),
        }

        while let Some(current) = pending.pop() {
            if cancel.is_cancelled() {
                incomplete = true;
                break;
            }

            let Ok(entries) = std::fs::read_dir(&current) else {
                incomplete = true;
                continue;
            };

            for entry in entries {
                let Ok(entry) = entry else {
                    incomplete = true;
                    break;
                };
                let Ok(meta) = entry.metadata() else {
                    incomplete = true;
                    continue;
                };
                let attributes = meta.file_attributes();
                let is_link = attributes & REPARSE_POINT != 0;
                if attributes & DIRECTORY != 0 {
                    directories += 1;
                    // A junction is one entry to delete, not the tree it points at.
                    if !is_link {
                        pending.push(entry.path());
                    }
                } else {
                    files += 1;
                    if !is_link {
                        bytes += meta.file_size();
                    }
                }
            }
        }

        measurement(bytes, files, directories, incomplete)
    }
}
