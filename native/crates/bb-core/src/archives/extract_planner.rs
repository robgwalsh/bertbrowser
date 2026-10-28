//! Decides what an extract would write, touching nothing. Spec: Core/Services/Archives/
//! ExtractPlanner.cs.
//!
//! The plan is built while a dialog sits open, so the executor re-applies every rule against live
//! disk before writing, the contract the transfer, rename, delete and new-item executors all keep.
//! **The byte total is exact and free for an addressable container**, because the uncompressed
//! lengths are already in its directory. A sequential one reports `bytes_are_exact: false`:
//! pulling three files out of a solid archive still reads the whole block, and a bar that promised
//! four megabytes and then read two gigabytes is worse than one that never claimed to know.

use bb_paths::UniquePath;

use super::{
    ArchiveFailure, ArchiveIndex, ArchiveNode, ExtractConflict, ExtractPlan, ExtractRejection,
    ExtractRules, PlannedExtraction,
};

/// The filesystem questions the planner asks. Abstracted so the rules about where an extract may
/// write are testable without a real disk under them.
pub trait ExtractProbe: Send + Sync {
    fn directory_exists(&self, path: &str) -> bool;
    fn file_exists(&self, path: &str) -> bool;
}

/// The real filesystem.
pub struct FileSystemExtractProbe;

impl ExtractProbe for FileSystemExtractProbe {
    fn directory_exists(&self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }

    fn file_exists(&self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }
}

pub struct ExtractPlanner<P: ExtractProbe = FileSystemExtractProbe> {
    probe: P,
}

impl Default for ExtractPlanner<FileSystemExtractProbe> {
    fn default() -> Self {
        ExtractPlanner::new(FileSystemExtractProbe)
    }
}

impl<P: ExtractProbe> ExtractPlanner<P> {
    pub fn new(probe: P) -> ExtractPlanner<P> {
        ExtractPlanner { probe }
    }

    /// Plans pulling `entry_paths` (or everything under `relative_to` when empty) out of `index`.
    pub fn plan(
        &self,
        index: &ArchiveIndex,
        archive_file: &str,
        relative_to: &str,
        entry_paths: &[String],
        destination_directory: &str,
        conflict: ExtractConflict,
    ) -> ExtractPlan {
        if !index.ok() {
            return ExtractPlan::refused(
                if index.failure == ArchiveFailure::PasswordRequired {
                    ExtractRejection::PasswordRequired
                } else {
                    ExtractRejection::ArchiveUnreadable
                },
                index
                    .error
                    .as_deref()
                    .unwrap_or("The archive could not be read."),
            );
        }

        if let Some(bad) = ExtractRules::reject_destination(
            destination_directory,
            |p| self.probe.directory_exists(p),
            |p| self.probe.file_exists(p),
        ) {
            return ExtractPlan::refused(
                bad,
                match bad {
                    ExtractRejection::DestinationNotDirectory => {
                        "The destination is a file, not a folder."
                    }
                    ExtractRejection::DestinationInsideArchive => {
                        "An archive cannot be extracted into itself."
                    }
                    _ => "The destination folder is missing.",
                },
            );
        }

        // Nothing named means "everything under here", which is what the toolbar and an
        // empty-space right-click mean by Extract.
        let roots: Vec<String> = if entry_paths.is_empty() {
            index
                .children(relative_to)
                .unwrap_or_default()
                .iter()
                .map(|n| n.path.clone())
                .collect()
        } else {
            entry_paths.to_vec()
        };

        let mut files: Vec<&ArchiveNode> = Vec::new();
        let mut directories: Vec<&ArchiveNode> = Vec::new();
        for root in &roots {
            if let Some(node) = index.find(root) {
                Self::collect(index, node, &mut files, &mut directories);
            }
        }

        if files.is_empty() && directories.is_empty() {
            return ExtractPlan::refused(
                ExtractRejection::NothingToExtract,
                "There is nothing here to extract.",
            );
        }

        let mut items = Vec::with_capacity(files.len() + directories.len());
        let mut conflicts = Vec::new();
        let mut total = 0i64;

        // Directories first and in depth order, so the executor can create them before the files
        // that land in them, and so a cancel can remove them in reverse and find them empty.
        directories.sort_by_key(|d| d.path.len());
        for dir in &directories {
            items.push(PlannedExtraction {
                entry_path: dir.path.clone(),
                destination_path: ExtractRules::target_for(
                    &dir.path,
                    relative_to,
                    destination_directory,
                ),
                size_bytes: 0,
                is_directory: true,
            });
        }

        for file in &files {
            let mut target =
                ExtractRules::target_for(&file.path, relative_to, destination_directory);

            if self.exists(&target) {
                conflicts.push(target.clone());
                if conflict == ExtractConflict::Skip {
                    continue;
                }
                target = UniquePath::for_path(
                    &target,
                    false,
                    |p| self.probe.directory_exists(p),
                    |p| self.probe.file_exists(p),
                );
            }

            items.push(PlannedExtraction {
                entry_path: file.path.clone(),
                destination_path: target,
                size_bytes: file.size_bytes,
                is_directory: false,
            });
            total += file.size_bytes;
        }

        ExtractPlan {
            archive_file: archive_file.to_string(),
            destination_directory: destination_directory.to_string(),
            items,
            conflicts,
            rejected: None,
            total_bytes: total,
            bytes_are_exact: !index.capabilities.sequential_only,
        }
    }

    fn collect<'i>(
        index: &'i ArchiveIndex,
        node: &'i ArchiveNode,
        files: &mut Vec<&'i ArchiveNode>,
        directories: &mut Vec<&'i ArchiveNode>,
    ) {
        if !node.is_directory {
            // A symlink entry is listed but never recreated: writing one would put a link into the
            // destination pointing wherever the container asked, which is the escape the index's
            // own key rules exist to prevent.
            if node.link_target.is_none() {
                files.push(node);
            }
            return;
        }
        directories.push(node);
        for child in index.children_of(node) {
            Self::collect(index, child, files, directories);
        }
    }

    fn exists(&self, path: &str) -> bool {
        self.probe.file_exists(path) || self.probe.directory_exists(path)
    }
}
