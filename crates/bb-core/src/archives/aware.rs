//! Makes an archive look like a folder to everything that lists directories. Spec: Core/Services/
//! Archives/ArchiveAwareFileSystemService.cs.
//!
//! A decorator over the real file-system service, which is why the callers of that trait needed
//! no changes at all. **An ordinary path never reaches the archive code**: `ArchivePath::parse`
//! is a segment scan first, so browsing `C:\Users\Rob` costs one string walk and nothing else.
//! **Nothing inside an archive is ever hidden.** A container's attribute field holds either a DOS
//! byte or a Unix mode depending on which tool wrote it, and "Show hidden items" filters on it;
//! map it blindly and most of a Linux tarball vanishes under the default setting.

use std::fmt;
use std::io;
use std::sync::Arc;

use super::{
    ArchiveBrowser, ArchiveCache, ArchiveFailure, ArchiveIndex, ArchiveNode, ArchivePath,
    ArchiveReader, ArchiveSearchScanner,
};
use crate::FileTime;
use crate::attributes::{DIRECTORY, NORMAL};
use crate::cancel::CancelToken;
use crate::data::SearchHit;
use crate::fs::{DriveInfo, FileSystemService, SubdirectoryPresence};
use crate::models::FileEntry;
use crate::search::SearchQuery;

/// Where the session's archive passwords are kept, if anywhere. A seam rather than a concrete
/// type so the listing layer never holds secrets itself, and so a harness run can supply
/// passwords without a dialog.
pub trait ArchivePasswords: Send + Sync {
    /// The password for this archive, or None if none has been given.
    fn for_archive(&self, archive_file: &str) -> Option<String>;
}

/// Nobody has any passwords. The default, and what the harness gets unless told otherwise.
pub struct NoArchivePasswords;

impl ArchivePasswords for NoArchivePasswords {
    fn for_archive(&self, _archive_file: &str) -> Option<String> {
        None
    }
}

/// A listing failed because the container needs a password. Carried inside the `io::Error` a
/// listing fails with, so everything that already handles one keeps working, and the one caller
/// that needs to tell this case apart, to offer to unlock, asks `ArchiveLockedError::in_error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveLockedError {
    pub archive_file: String,
    pub message: String,
}

impl ArchiveLockedError {
    /// The locked-archive error inside `error`, if that is what it is.
    pub fn in_error(error: &io::Error) -> Option<&ArchiveLockedError> {
        error.get_ref()?.downcast_ref::<ArchiveLockedError>()
    }
}

impl fmt::Display for ArchiveLockedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ArchiveLockedError {}

pub struct ArchiveAwareFileSystemService {
    inner: Arc<dyn FileSystemService>,
    reader: Arc<dyn ArchiveReader>,
    passwords: Arc<dyn ArchivePasswords>,
    cache: ArchiveCache,
}

impl ArchiveAwareFileSystemService {
    pub fn new(
        inner: Arc<dyn FileSystemService>,
        reader: Arc<dyn ArchiveReader>,
    ) -> ArchiveAwareFileSystemService {
        Self::with_passwords(
            inner,
            reader,
            Arc::new(NoArchivePasswords),
            ArchiveCache::default(),
        )
    }

    pub fn with_passwords(
        inner: Arc<dyn FileSystemService>,
        reader: Arc<dyn ArchiveReader>,
        passwords: Arc<dyn ArchivePasswords>,
        cache: ArchiveCache,
    ) -> ArchiveAwareFileSystemService {
        ArchiveAwareFileSystemService {
            inner,
            reader,
            passwords,
            cache,
        }
    }

    /// One entry as a listing row. A directory carries its exact recursive size rather than -1,
    /// which is what tells the file list not to go asking `dir_size_cache` about a path that is
    /// not real. A missing timestamp becomes unknown, which the Modified column renders blank.
    fn to_entry(archive_file: &str, node: &ArchiveNode) -> FileEntry {
        FileEntry::new(
            &node.name,
            &ArchivePath::compose(archive_file, &node.path),
            node.is_directory,
            node.size_bytes,
            node.modified.unwrap_or(FileTime::UNKNOWN),
            if node.is_directory { DIRECTORY } else { NORMAL },
        )
    }
}

fn is_archive_file(candidate: &str) -> bool {
    std::path::Path::new(candidate).is_file()
}

impl ArchiveBrowser for ArchiveAwareFileSystemService {
    /// Reads a container, cached. Navigation and the preview pane both need the same index the
    /// listing was built from; re-reading it would be both slower and, if the file changed in
    /// between, a different answer.
    fn read_archive(&self, archive_file: &str) -> Arc<ArchiveIndex> {
        let password = self.passwords.for_archive(archive_file);
        self.cache
            .get(archive_file, password.as_deref(), |file, pw| {
                self.reader.read(file, pw, &CancelToken::none())
            })
    }

    fn resolve(&self, path: &str) -> Option<ArchivePath> {
        ArchivePath::parse(path, is_archive_file)
    }

    /// What the navigation gate asks *after* the cheap syntactic check says maybe. It opens the
    /// container, so it belongs on a worker thread. A failure is still navigable: the list shows
    /// the banner, which is the whole point of letting the load rather than the gate decide. Only
    /// "there is no such folder in a perfectly readable archive" is a refusal.
    fn can_list(&self, path: &str) -> bool {
        if std::path::Path::new(path).is_dir() {
            return true;
        }
        let Some(archive) = self.resolve(path) else {
            return false;
        };
        let index = self.read_archive(&archive.archive_file);
        !index.ok() || index.children(&archive.entry_path).is_some()
    }

    fn search_archive(
        &self,
        archive_file: &str,
        query: &SearchQuery,
        limit: usize,
        cancel: &CancelToken,
    ) -> Vec<SearchHit> {
        let index = self.read_archive(archive_file);
        ArchiveSearchScanner::search(&index, archive_file, "", Some(query), limit, cancel, true)
            .unwrap_or_default()
    }
}

impl FileSystemService for ArchiveAwareFileSystemService {
    fn list_directory(&self, path: &str) -> io::Result<Vec<FileEntry>> {
        let Some(archive) = self.resolve(path) else {
            return self.inner.list_directory(path);
        };

        let index = self.read_archive(&archive.archive_file);

        // The trait's contract is that listing fails on failure, and the file list already turns
        // an error into its banner. The one failure the caller must be able to tell apart rides
        // inside the error rather than through a second channel.
        if index.failure == ArchiveFailure::PasswordRequired {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                ArchiveLockedError {
                    archive_file: archive.archive_file.clone(),
                    message: index
                        .error
                        .clone()
                        .unwrap_or_else(|| "This archive is protected.".to_string()),
                },
            ));
        }
        if !index.ok() {
            return Err(io::Error::other(
                index
                    .error
                    .clone()
                    .unwrap_or_else(|| "The archive could not be read.".to_string()),
            ));
        }

        let children = index.children(&archive.entry_path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "There is no folder named '{}' in this archive.",
                    archive.entry_path
                ),
            )
        })?;

        Ok(children
            .iter()
            .map(|child| Self::to_entry(&archive.archive_file, child))
            .collect())
    }

    /// Answers for an archive file as readily as for a folder inside one, and stays honest rather
    /// than being made to lie in order to keep archives out of the folder tree. The tree cannot
    /// show one anyway: it builds nodes from `list_directory` filtered on `is_directory`, and an
    /// archive is a file. A structural fact is a better guarantee than a probe that reports "no
    /// subfolders" about something it can list the subfolders of.
    fn probe_subdirectories(&self, path: &str) -> SubdirectoryPresence {
        let Some(archive) = self.resolve(path) else {
            return self.inner.probe_subdirectories(path);
        };
        let index = self.read_archive(&archive.archive_file);
        if !index.ok() {
            return SubdirectoryPresence::default();
        }
        let Some(children) = index.children(&archive.entry_path) else {
            return SubdirectoryPresence::default();
        };
        let any = children.iter().any(|c| c.is_directory);
        // Nothing inside an archive is hidden, so "any" and "any visible" are the same answer.
        SubdirectoryPresence {
            any,
            any_visible: any,
        }
    }

    fn drives(&self) -> Vec<DriveInfo> {
        self.inner.drives()
    }
}
