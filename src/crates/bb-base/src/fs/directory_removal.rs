//! Erasing a directory tree, which this app has to do in a way `remove_dir_all` does not.
//! Spec: Core/Services/DirectoryRemoval.cs.
//!
//! **A recursive delete cannot be trusted on anything the user owns.** Given a tree with a junction
//! anywhere in it, .NET's erased everything else and *then* failed naming the link: on a permanent
//! delete the folder's contents were destroyed, could not be undone, and the user was told the
//! delete had failed; on a staging commit the failure was swallowed as harmless cleanup and half a
//! folder was left behind for good. So the walk is done here, once, for every caller that erases a
//! tree. A junction is removed as the single entry it is, never followed.

use std::fs::Metadata;
use std::io;
use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;

use crate::attributes::{DIRECTORY, REPARSE_POINT};

pub struct DirectoryRemoval;

impl DirectoryRemoval {
    /// Erases `root` and everything under it. Fails with exactly what the filesystem calls fail
    /// with: a caller that wants best-effort cleanup ignores it, one carrying out a delete the user
    /// asked for reports it. Walked with an explicit stack rather than by recursion, because the
    /// depth is whatever the user's disk contains; directories are collected in pre-order and
    /// removed in reverse, which is the post-order the removal needs.
    pub fn remove_tree(root: &str) -> io::Result<()> {
        let top = std::fs::symlink_metadata(root)?;
        if Self::is_link(&top) {
            // Non-recursive on purpose: this removes the link, never what it points at.
            return std::fs::remove_dir(root);
        }

        let mut directories: Vec<PathBuf> = vec![PathBuf::from(root)];
        let mut pending: Vec<PathBuf> = vec![PathBuf::from(root)];

        while let Some(current) = pending.pop() {
            for entry in std::fs::read_dir(&current)? {
                let entry = entry?;
                let meta = entry.metadata()?;
                let path = entry.path();
                // The attribute bit, not `FileType::is_dir`, which calls a junction a symlink.
                let is_dir = meta.file_attributes() & DIRECTORY != 0;
                if is_dir && !Self::is_link(&meta) {
                    pending.push(path.clone());
                    directories.push(path);
                } else if is_dir {
                    // A directory link: one entry to remove, non-recursively.
                    std::fs::remove_dir(&path)?;
                } else {
                    std::fs::remove_file(&path)?;
                }
            }
        }

        for directory in directories.iter().rev() {
            std::fs::remove_dir(directory)?;
        }
        Ok(())
    }

    /// A junction or symlink: one entry, not the tree it points at.
    pub fn is_link(meta: &Metadata) -> bool {
        meta.file_attributes() & REPARSE_POINT != 0
    }
}
