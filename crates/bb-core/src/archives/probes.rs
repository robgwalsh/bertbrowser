//! Answers "what is already in here?" from a container's index instead of from disk.
//! Spec: Core/Services/Archives/ArchiveProbes.cs.
//!
//! This is why the rename planner needed no changes to work inside an archive. Its probe seam
//! exists so the collision rules can be tested against a fake filesystem; a container is just
//! another filesystem those rules know nothing about, and they turn out to be exactly right for
//! it. It answers about paths *inside one container* and falls through to the real disk for
//! anything else, so a planner holding one is not lying about the rest of the world.

use super::{ArchiveIndex, ArchivePath};
use crate::rename::RenameProbe;

pub struct ArchiveRenameProbe<'a> {
    index: &'a ArchiveIndex,
    archive_file: String,
}

impl<'a> ArchiveRenameProbe<'a> {
    pub fn new(index: &'a ArchiveIndex, archive_file: &str) -> ArchiveRenameProbe<'a> {
        ArchiveRenameProbe {
            index,
            archive_file: archive_file.to_string(),
        }
    }

    /// Whether the entry at `path` is a directory, or None when it is not in this container.
    fn find(&self, path: &str) -> Option<bool> {
        let entry = ArchivePath::parse(path, |candidate| self.is_this_archive(candidate))?;
        if entry.is_root() {
            return Some(true);
        }
        self.index.find(&entry.entry_path).map(|n| n.is_directory)
    }

    /// The one file this probe treats as a container. Deliberately not a disk check: a probe that
    /// reached the disk would answer about other archives it has never read.
    fn is_this_archive(&self, candidate: &str) -> bool {
        candidate.eq_ignore_ascii_case(&self.archive_file)
    }
}

impl RenameProbe for ArchiveRenameProbe<'_> {
    fn directory_exists(&self, path: &str) -> bool {
        self.find(path) == Some(true)
    }

    fn file_exists(&self, path: &str) -> bool {
        self.find(path) == Some(false)
    }
}
