//! Decides what a drop would do, without touching anything. Every rule that protects data lives
//! here so it can be tested in isolation: a folder is never allowed into its own subtree, sources
//! that travel inside another source are dropped, and roots are refused.
//! Spec: Core/Services/Transfer/TransferPlanner.cs.
//!
//! Containment is checked twice, once on the literal paths and once on link-resolved paths,
//! because a junction can make an unrelated-looking destination physically sit inside the source.
//! Both checks must pass, so an unreadable or unresolvable link fails closed onto the literal one.

use std::collections::HashSet;

use bb_paths::PathKey;

use super::models::{
    PlannedTransfer, RejectedTransfer, TransferPlan, TransferRejection, TransferVerb, join, key_of,
    parent_of,
};
use super::probe::{FileSystemTransferProbe, TransferProbe};

pub struct TransferPlanner {
    probe: Box<dyn TransferProbe>,
}

impl Default for TransferPlanner {
    fn default() -> Self {
        TransferPlanner::new(Box::new(FileSystemTransferProbe))
    }
}

impl TransferPlanner {
    pub fn new(probe: Box<dyn TransferProbe>) -> TransferPlanner {
        TransferPlanner { probe }
    }

    pub fn plan(
        &self,
        source_paths: &[String],
        destination_directory: &str,
        verb: TransferVerb,
    ) -> TransferPlan {
        if destination_directory.trim().is_empty() {
            return TransferPlan::empty(verb, destination_directory);
        }

        // A destination problem sinks every source, so report it once per source and stop.
        if self.probe.file_exists(destination_directory)
            && !self.probe.directory_exists(destination_directory)
        {
            return Self::reject_all(
                source_paths,
                verb,
                destination_directory,
                TransferRejection::DestinationNotDirectory,
                "The drop target is a file, not a folder.",
            );
        }
        if !self.probe.directory_exists(destination_directory) {
            return Self::reject_all(
                source_paths,
                verb,
                destination_directory,
                TransferRejection::DestinationMissing,
                "The destination folder no longer exists.",
            );
        }

        let dest_key = key_of(destination_directory);
        let dest_resolved_key = key_of(&self.probe.resolve_final_path(destination_directory));

        // Distinct sources, and the keys of the directories among them, so a source nested under
        // another can be recognized before anything is planned.
        let sources = Self::distinct(source_paths);
        let directory_keys: HashSet<String> = sources
            .iter()
            .filter(|(path, _)| self.probe.directory_exists(path))
            .map(|(_, key)| key.clone())
            .collect();

        // Reserve the names already spoken for by earlier transfers in this same plan, so two
        // sources with the same name are not both reported as landing on the same path.
        let mut claimed: HashSet<String> = HashSet::new();
        let mut transfers = Vec::new();
        let mut rejected = Vec::new();

        for (path, key) in &sources {
            let is_directory = self.probe.directory_exists(path);
            let name = bb_paths::file_name(path);
            if !is_directory && !self.probe.file_exists(path) {
                rejected.push(RejectedTransfer::new(
                    path,
                    TransferRejection::SourceMissing,
                    format!("'{name}' no longer exists."),
                ));
                continue;
            }

            let Some(parent) = parent_of(path) else {
                rejected.push(RejectedTransfer::new(
                    path,
                    TransferRejection::SourceIsRoot,
                    format!("'{path}' is a drive root and cannot be moved."),
                ));
                continue;
            };

            if PathKey::has_ancestor_key_in(key, &directory_keys) {
                rejected.push(RejectedTransfer::new(
                    path,
                    TransferRejection::MovesWithAncestor,
                    format!("'{name}' travels with the folder above it."),
                ));
                continue;
            }

            if is_directory && dest_key == *key {
                rejected.push(RejectedTransfer::new(
                    path,
                    TransferRejection::DestinationIsSource,
                    format!("Cannot drop '{name}' onto itself."),
                ));
                continue;
            }

            if is_directory && self.is_inside(path, key, &dest_key, &dest_resolved_key) {
                rejected.push(RejectedTransfer::new(
                    path,
                    TransferRejection::DestinationInsideSource,
                    format!("Cannot move '{name}' into one of its own subfolders."),
                ));
                continue;
            }

            if verb == TransferVerb::Move && key_of(&parent) == dest_key {
                rejected.push(RejectedTransfer::new(
                    path,
                    TransferRejection::AlreadyInDestination,
                    format!("'{name}' is already here."),
                ));
                continue;
            }

            let destination_path = join(destination_directory, name);
            let destination_key = key_of(&destination_path);
            let conflicts = !claimed.insert(destination_key) || self.exists(&destination_path);
            transfers.push(PlannedTransfer::new(
                path,
                is_directory,
                &destination_path,
                conflicts,
            ));
        }

        TransferPlan::new(verb, destination_directory, transfers, rejected)
    }

    /// True when the destination sits at or under the source folder. Tested on the literal paths
    /// and again on link-resolved paths, so a junction cannot smuggle the destination inside the
    /// tree being moved.
    fn is_inside(
        &self,
        source_path: &str,
        source_key: &str,
        dest_key: &str,
        dest_resolved_key: &str,
    ) -> bool {
        if PathKey::is_under_key(dest_key, source_key) {
            return true;
        }

        let source_resolved_key = key_of(&self.probe.resolve_final_path(source_path));
        if source_resolved_key == dest_resolved_key
            || PathKey::is_under_key(dest_resolved_key, &source_resolved_key)
        {
            return true;
        }

        // Mixed forms catch a link on exactly one side of the comparison.
        PathKey::is_under_key(dest_resolved_key, source_key)
            || PathKey::is_under_key(dest_key, &source_resolved_key)
    }

    fn reject_all(
        source_paths: &[String],
        verb: TransferVerb,
        destination: &str,
        reason: TransferRejection,
        message: &str,
    ) -> TransferPlan {
        TransferPlan::new(
            verb,
            destination,
            Vec::new(),
            Self::distinct(source_paths)
                .into_iter()
                .map(|(path, _)| RejectedTransfer::new(&path, reason, message.to_string()))
                .collect(),
        )
    }

    /// Distinct sources in input order, folded case-insensitively like the filesystem. A path that
    /// cannot be keyed is dropped: there is nothing sensible to plan for it.
    fn distinct(source_paths: &[String]) -> Vec<(String, String)> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut result = Vec::new();
        for path in source_paths {
            if path.trim().is_empty() {
                continue;
            }
            let Ok(key) = PathKey::canonicalize(path) else {
                continue;
            };
            if seen.insert(key.clone()) {
                result.push((path.clone(), key));
            }
        }
        result
    }

    fn exists(&self, path: &str) -> bool {
        self.probe.directory_exists(path) || self.probe.file_exists(path)
    }
}
