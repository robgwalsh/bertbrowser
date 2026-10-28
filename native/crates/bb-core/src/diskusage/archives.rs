//! Disk usage over a container: the `DiskUsageArchives` seam answered from the archive browser's
//! cached index. Spec: the `IArchiveBrowser.Resolve` / `ReadArchive(...).Find` calls in
//! Core/Services/DiskUsage/DiskUsageService.cs.
//!
//! Every size inside a container is exact, so this is the one place the view is never
//! approximate. The way that would regress is silently: a breakdown that asked `dir_size_cache`
//! would miss on every folder, report them all unknown, and then blame the index. Hence the
//! answer comes from the container's own tree and nothing here touches a `PathKey`-keyed table.

use std::sync::Arc;

use super::{ArchiveEntries, ArchiveFileSize, DiskUsageArchives};
use crate::FileTime;
use crate::archives::{ArchiveBrowser, ArchiveIndex, ArchiveNode, ArchivePath};
use crate::cancel::{CancelToken, Cancelled};

/// The archive browser as disk usage sees it. A newtype rather than a blanket impl, so a test
/// fake of either trait never has to be the other.
pub struct ArchiveUsage(pub Arc<dyn ArchiveBrowser>);

impl DiskUsageArchives for ArchiveUsage {
    fn resolve(&self, path: &str) -> Option<ArchivePath> {
        self.0.resolve(path)
    }

    fn files_under(
        &self,
        root: &ArchivePath,
        cancel: &CancelToken,
    ) -> Result<ArchiveEntries, Cancelled> {
        let index = self.0.read_archive(&root.archive_file);
        if !index.ok() {
            return Ok(ArchiveEntries::Unreadable);
        }
        let start = if root.is_root() {
            Some(index.root())
        } else {
            index.find(&root.entry_path)
        };
        let Some(start) = start else {
            return Ok(ArchiveEntries::NotFound);
        };
        let mut files = Vec::new();
        collect(&index, start, &mut files, cancel)?;
        Ok(ArchiveEntries::Files(files))
    }
}

/// Every file under `start`, at any depth, in the order the C# stack walk produced them (children
/// pushed in name order, so popped last-first). The order matters only for ties in the size sort
/// that follows, which is stable, and the tests pin it. A symlink entry is listed as the file it
/// is, never followed: the container's tree has no cycles, its links are just names.
fn collect(
    index: &ArchiveIndex,
    start: &ArchiveNode,
    out: &mut Vec<ArchiveFileSize>,
    cancel: &CancelToken,
) -> Result<(), Cancelled> {
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        cancel.check()?;
        if !node.is_directory {
            out.push(ArchiveFileSize {
                entry_path: node.path.clone(),
                name: node.name.clone(),
                size_bytes: node.size_bytes,
                modified_utc: node.modified.unwrap_or(FileTime::UNKNOWN),
            });
            continue;
        }
        stack.extend(index.children_of(node));
    }
    Ok(())
}
