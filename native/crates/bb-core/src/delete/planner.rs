//! Works out what deleting a selection would take with it, without touching anything. Every rule
//! that stops a delete reaching further than the user meant lives here, so the confirmation the
//! user answers is the same set of items the executor will act on.
//! Spec: Core/Services/Delete/DeletePlanner.cs.
//!
//! A selection can name a folder and something inside it at once — a flattened search result
//! shows both — and deleting both would leave the second one referring to a path that no longer
//! exists. The inner item is dropped as a benign no-op rather than reported: it really is being
//! deleted, just as part of its ancestor.

use std::borrow::Cow;
use std::collections::HashSet;

use bb_paths::{PathKey, file_name, parent_key};

use super::models::{
    DeleteDisposition, DeleteMode, DeletePlan, DeleteRejection, DeleteSource, PlannedDelete,
    RejectedDelete,
};
use super::probe::{DeleteProbe, NoRecycleProbe, RecycleProbe};
use super::protected::ProtectedLocations;
use crate::archives::ArchivePath;

pub struct DeletePlanner<'a> {
    probe: &'a dyn DeleteProbe,
    recycle_probe: &'a dyn RecycleProbe,
    protected_keys: Cow<'static, HashSet<String>>,
}

impl<'a> DeletePlanner<'a> {
    /// A planner that refuses [`ProtectedLocations::default_keys`] and assumes no volume has a
    /// Recycle Bin, so Core on its own routes everything to the holding folder rather than
    /// assuming a bin that may not be there.
    pub fn new(probe: &'a dyn DeleteProbe) -> DeletePlanner<'a> {
        DeletePlanner {
            probe,
            recycle_probe: NoRecycleProbe::INSTANCE,
            protected_keys: Cow::Borrowed(ProtectedLocations::default_keys()),
        }
    }

    /// Replaces the locations refused outright. Injectable so the rule can be tested without
    /// depending on where Windows happens to be installed; an empty list protects nothing.
    pub fn protecting(mut self, paths: &[&str]) -> DeletePlanner<'a> {
        self.protected_keys = Cow::Owned(ProtectedLocations::keys_of(paths.iter().copied()));
        self
    }

    /// How the planner asks whether a volume has a Recycle Bin that will take something.
    pub fn recycle_probe(mut self, probe: &'a dyn RecycleProbe) -> DeletePlanner<'a> {
        self.recycle_probe = probe;
        self
    }

    pub fn plan(&self, sources: &[DeleteSource], mode: DeleteMode) -> DeletePlan {
        let distinct = Self::distinct(sources);
        if distinct.is_empty() {
            return DeletePlan::empty(mode);
        }

        let mut deletions = Vec::new();
        let mut rejected = Vec::new();

        // Selected folders, so an item that is already going with its ancestor can be recognized.
        let directory_keys: HashSet<String> = distinct
            .iter()
            .filter(|(path, _)| self.probe.directory_exists(path))
            .map(|(_, key)| key.clone())
            .collect();

        for (path, key) in &distinct {
            let is_directory = self.probe.directory_exists(path);
            if !is_directory && !self.probe.file_exists(path) {
                // Something inside an archive is refused by the same test — it has no path on
                // disk — but "no longer exists" would be a lie about a file the user can see. The
                // menu already hides Delete there; this is the backstop and it should say why.
                let message = if ArchivePath::parse(path, is_file_on_disk).is_some() {
                    format!(
                        "'{}' is inside an archive. Extract it first.",
                        file_name(path)
                    )
                } else {
                    format!("'{}' no longer exists.", file_name(path))
                };
                rejected.push(RejectedDelete {
                    source_path: path.clone(),
                    reason: DeleteRejection::SourceMissing,
                    message,
                });
                continue;
            }

            if parent_of(path).is_none() {
                rejected.push(RejectedDelete {
                    source_path: path.clone(),
                    reason: DeleteRejection::SourceIsRoot,
                    message: format!("'{path}' is a drive root and cannot be deleted."),
                });
                continue;
            }

            if self.protected_keys.contains(key) {
                rejected.push(RejectedDelete {
                    source_path: path.clone(),
                    reason: DeleteRejection::ProtectedLocation,
                    message: format!("'{path}' is a system location and will not be deleted."),
                });
                continue;
            }

            if ProtectedLocations::is_inside_recycle_bin(path) {
                rejected.push(RejectedDelete {
                    source_path: path.clone(),
                    reason: DeleteRejection::ProtectedLocation,
                    message: format!(
                        "'{}' is in the Recycle Bin; empty it from Windows instead.",
                        file_name(path)
                    ),
                });
                continue;
            }

            if PathKey::has_ancestor_key_in(key, &directory_keys) {
                rejected.push(RejectedDelete {
                    source_path: path.clone(),
                    reason: DeleteRejection::InsideADeletedFolder,
                    message: format!(
                        "'{}' is inside a folder that is being deleted too.",
                        file_name(path)
                    ),
                });
                continue;
            }

            deletions.push(
                PlannedDelete::new(path, is_directory)
                    .with_disposition(self.disposition_for(mode, path)),
            );
        }

        DeletePlan {
            mode,
            deletions,
            rejected,
        }
    }

    /// Where one item is really going. A Recycle Bin delete falls back to the holding folder when
    /// the item's volume has no bin — a network share, removable media with it turned off —
    /// because the alternative the shell offers is erasing the item outright, which is not what
    /// the user asked for and not something to discover afterwards.
    fn disposition_for(&self, mode: DeleteMode, path: &str) -> DeleteDisposition {
        match mode {
            DeleteMode::Permanent => DeleteDisposition::Erase,
            DeleteMode::Staged => DeleteDisposition::Stage,
            DeleteMode::Recycle => {
                if self.recycle_probe.can_recycle(path) {
                    DeleteDisposition::Recycle
                } else {
                    DeleteDisposition::Stage
                }
            }
        }
    }

    /// Drops repeats, so nothing is planned — or counted in the confirmation — twice. Anything
    /// that cannot become a key is dropped with them.
    fn distinct(sources: &[DeleteSource]) -> Vec<(String, String)> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for source in sources {
            if source.path.trim().is_empty() {
                continue;
            }
            let Ok(key) = PathKey::canonicalize(&source.path) else {
                continue;
            };
            if seen.insert(key.clone()) {
                result.push((source.path.clone(), key));
            }
        }
        result
    }
}

/// `Path.GetDirectoryName(GetFullPath(path))`: None at a drive or volume root.
pub(crate) fn parent_of(path: &str) -> Option<String> {
    let full = PathKey::normalize_display(path).ok()?;
    parent_key(&full)
}

/// `File.Exists` on the real disk — the archive test asks about a container, not the probe's
/// model of the selection.
fn is_file_on_disk(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
}
