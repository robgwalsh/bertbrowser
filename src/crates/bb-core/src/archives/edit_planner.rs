//! Decides whether a container may be edited, and what the edit would amount to.
//! Spec: Core/Services/Archives/ArchiveEditPlanner.cs.
//!
//! **Nothing is modified in place, ever.** Every edit is a full rewrite, and the honest thing is to
//! say so before starting rather than to look fast and take ten minutes. **Most of this file is
//! refusals, and that is the design**: the formats and shapes below cannot be rewritten without
//! silently changing or losing something, so each is refused by name. Being told "7z archives
//! cannot be edited here" is a far better outcome than a rewrite that quietly turns a solid
//! archive into a non-solid one twice the size.

use std::collections::{HashMap, HashSet};

use bb_paths::{file_name, fold_upper, split_last};

use crate::rename::RenamePattern;

use super::{
    AddFile, ArchiveEdit, ArchiveEditPlan, ArchiveEditRejection, ArchiveFormats, ArchiveIndex,
    ArchiveNode,
};

pub struct ArchiveEditPlanner;

impl Default for ArchiveEditPlanner {
    fn default() -> Self {
        ArchiveEditPlanner
    }
}

impl ArchiveEditPlanner {
    /// Above this, an edit is refused with its cost rather than silently taking minutes.
    pub const MAX_REWRITE_BYTES: i64 = 2 * 1024 * 1024 * 1024;

    pub fn new() -> ArchiveEditPlanner {
        ArchiveEditPlanner
    }

    pub fn plan(
        &self,
        index: &ArchiveIndex,
        archive_file: &str,
        archive_bytes: i64,
        edits: &[ArchiveEdit],
    ) -> ArchiveEditPlan {
        if !index.ok() {
            return ArchiveEditPlan::refused(
                ArchiveEditRejection::Unreadable,
                index
                    .error
                    .as_deref()
                    .unwrap_or("The archive could not be read."),
            );
        }

        let Some(format) = ArchiveFormats::match_name(file_name(archive_file)) else {
            return ArchiveEditPlan::refused(
                ArchiveEditRejection::Unreadable,
                "That is not an archive this app can read.",
            );
        };

        // Worded for editing rather than reusing ArchiveWriteRules::why_not_writable, which is
        // about creating: "7z archives cannot be created" is a confusing thing to be told when
        // you were trying to delete a file out of one.
        if !format.writable {
            return ArchiveEditPlan::refused(
                ArchiveEditRejection::FormatNotWritable,
                &format!(
                    "{} archives can be read but not changed — nothing here can write one.",
                    format.suffix
                ),
            );
        }

        // A rewrite would have to recompress every block, and cannot reproduce the solid layout,
        // so the file that came back would be materially different from the one that went in.
        if index.capabilities.sequential_only && format.random_access {
            return ArchiveEditPlan::refused(
                ArchiveEditRejection::Solid,
                "This archive is solid. Changing it would have to rebuild it differently, so it is not offered.",
            );
        }

        // Re-encrypting means holding a password in order to *write* with it, which is a promise
        // a file browser should not make, and a rewrite that dropped the encryption would be worse.
        if index.capabilities.is_encrypted {
            return ArchiveEditPlan::refused(
                ArchiveEditRejection::Encrypted,
                "This archive is encrypted. Changing it would remove the encryption, so it is not offered.",
            );
        }

        if !index.capabilities.is_complete {
            return ArchiveEditPlan::refused(
                ArchiveEditRejection::Incomplete,
                "This archive is incomplete — part of it is missing. Changing it would make that permanent.",
            );
        }

        if archive_bytes > Self::MAX_REWRITE_BYTES {
            return ArchiveEditPlan::refused(
                ArchiveEditRejection::TooLarge,
                &format!(
                    "Changing this archive would mean rewriting {}, which is more than this will do in one go.",
                    crate::format::byte_size(archive_bytes)
                ),
            );
        }

        if edits.is_empty() {
            return ArchiveEditPlan::refused(
                ArchiveEditRejection::NothingToDo,
                "There is nothing to change.",
            );
        }

        // --- what each edit means, against what is really in there ---

        let mut removals: HashSet<String> = HashSet::new();
        let mut renames: HashMap<String, String> = HashMap::new();
        let mut additions: Vec<AddFile> = Vec::new();

        for edit in edits {
            match edit {
                ArchiveEdit::Remove { entry_path } => {
                    let Some(node) = index.find(entry_path) else {
                        return Self::missing(entry_path);
                    };
                    // A folder takes everything under it, which is what deleting a folder means
                    // everywhere else in this app.
                    for path in Self::subtree(index, node) {
                        removals.insert(fold_upper(&path));
                    }
                }

                ArchiveEdit::Rename {
                    entry_path,
                    new_name,
                } => {
                    let Some(node) = index.find(entry_path) else {
                        return Self::missing(entry_path);
                    };

                    if let Some(problem) = RenamePattern::validate(new_name) {
                        return ArchiveEditPlan::refused(
                            ArchiveEditRejection::InvalidName,
                            &problem,
                        );
                    }

                    let (parent, _) = split_last(entry_path);
                    let target = if parent.is_empty() {
                        new_name.clone()
                    } else {
                        format!("{parent}\\{new_name}")
                    };

                    // Nothing may land on a name the container already holds. Unlike a filesystem
                    // rename there is no staging trick available here, the rewrite writes each
                    // entry once, so a swap is refused rather than half-done.
                    if index.find(&target).is_some() {
                        return ArchiveEditPlan::refused(
                            ArchiveEditRejection::NameTaken,
                            &format!("'{new_name}' is already in this archive."),
                        );
                    }

                    // Renaming a folder moves everything beneath it.
                    for path in Self::subtree(index, node) {
                        let renamed = format!("{target}{}", &path[entry_path.len()..]);
                        renames.insert(fold_upper(&path), renamed);
                    }
                }

                ArchiveEdit::AddFile(add) => {
                    if index.find(&add.entry_path).is_some() {
                        return ArchiveEditPlan::refused(
                            ArchiveEditRejection::NameTaken,
                            &format!(
                                "'{}' is already in this archive.",
                                file_name(&add.entry_path)
                            ),
                        );
                    }
                    additions.push(add.clone());
                }
            }
        }

        ArchiveEditPlan {
            archive_file: archive_file.to_string(),
            edits: edits.to_vec(),
            renames,
            removals,
            additions,
            rewrite_bytes: archive_bytes,
            rejected: None,
        }
    }

    fn missing(entry_path: &str) -> ArchiveEditPlan {
        ArchiveEditPlan::refused(
            ArchiveEditRejection::EntryMissing,
            &format!("'{}' is no longer in this archive.", file_name(entry_path)),
        )
    }

    /// An entry and, when it is a folder, everything under it.
    fn subtree(index: &ArchiveIndex, node: &ArchiveNode) -> Vec<String> {
        let mut paths = vec![node.path.clone()];
        if !node.is_directory {
            return paths;
        }
        let mut stack: Vec<&ArchiveNode> = index.children_of(node);
        while let Some(next) = stack.pop() {
            paths.push(next.path.clone());
            stack.extend(index.children_of(next));
        }
        paths
    }
}
