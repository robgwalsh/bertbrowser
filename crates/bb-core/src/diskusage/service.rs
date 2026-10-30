//! The disk-usage queries, shaped like `SearchService`: synchronous over the repositories, meant
//! to be called off the UI thread with a cancel token.
//!
//! Every number here comes from data the MFT pass already wrote: `fs_entry` for files and
//! `dir_size_cache` for folder totals. Nothing in this module walks the filesystem to size a
//! folder, which is the same rule the file list and the folder tree follow.
//! Spec: Core/Services/DiskUsage/DiskUsageService.cs.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use bb_paths::PathKey;

use super::{
    DiskUsageAvailability, DiskUsageBreakdown, DiskUsageNode, DiskUsageRules, LargestFilesOutcome,
};
use crate::FileTime;
use crate::archives::ArchivePath;
use crate::attributes::HIDDEN;
use crate::cancel::{CancelToken, Cancelled};
use crate::data::{self, DirSizeRepository, FsIndexRepository, SearchHit};
use crate::fs::FileSystemService;
use crate::mft::NullMftIndex;

/// The slice of the MFT index service disk usage asks: the two questions search asks, plus
/// whether *any* volume is indexed, which the whole-PC scope needs.
// Narrower than `mft::MftIndex` on purpose: a test fakes three answers rather than a whole indexer,
// and the app adapts the real index to it (`bb-app` `tools_disk`).
pub trait DiskUsageIndex: Send + Sync {
    fn is_building(&self) -> bool;
    fn is_indexed(&self, path_key: &str) -> bool;
    fn any_indexed(&self) -> bool;
}

impl DiskUsageIndex for NullMftIndex {
    fn is_building(&self) -> bool {
        false
    }

    fn is_indexed(&self, _path_key: &str) -> bool {
        false
    }

    fn any_indexed(&self) -> bool {
        false
    }
}

/// One file inside a container, as its index records it: the exact uncompressed length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveFileSize {
    /// The entry's path inside the container.
    pub entry_path: String,
    pub name: String,
    pub size_bytes: i64,
    /// Unknown when the container does not record one.
    pub modified_utc: FileTime,
}

/// What a container's index says about the folder asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveEntries {
    /// The container could not be read.
    Unreadable,
    /// The container is fine but holds no such folder.
    NotFound,
    /// Every file under the folder, at any depth.
    Files(Vec<ArchiveFileSize>),
}

/// The slice of the archive browser disk usage asks: whether a path is inside a container, and
/// what files a folder inside one holds.
// Implemented over the archive browser by `ArchiveUsage` (`archives.rs`).
pub trait DiskUsageArchives: Send + Sync {
    fn resolve(&self, path: &str) -> Option<ArchivePath>;
    fn files_under(
        &self,
        root: &ArchivePath,
        cancel: &CancelToken,
    ) -> Result<ArchiveEntries, Cancelled>;
}

/// What a disk-usage query can fail with.
#[derive(Debug)]
pub enum DiskUsageError {
    Cancelled,
    /// The folder could not be listed.
    Io(io::Error),
    Data(data::Error),
}

impl std::fmt::Display for DiskUsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiskUsageError::Cancelled => f.write_str("cancelled"),
            DiskUsageError::Io(e) => write!(f, "{e}"),
            DiskUsageError::Data(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DiskUsageError {}

impl From<Cancelled> for DiskUsageError {
    fn from(_: Cancelled) -> Self {
        DiskUsageError::Cancelled
    }
}

impl From<io::Error> for DiskUsageError {
    fn from(e: io::Error) -> Self {
        DiskUsageError::Io(e)
    }
}

impl From<data::Error> for DiskUsageError {
    fn from(e: data::Error) -> Self {
        DiskUsageError::Data(e)
    }
}

impl From<bb_paths::PathError> for DiskUsageError {
    fn from(e: bb_paths::PathError) -> Self {
        DiskUsageError::Data(data::Error::Path(e))
    }
}

pub struct DiskUsageService {
    index: Arc<FsIndexRepository>,
    dir_sizes: Arc<DirSizeRepository>,
    file_system: Arc<dyn FileSystemService>,
    mft_index: Arc<dyn DiskUsageIndex>,
    archives: Option<Arc<dyn DiskUsageArchives>>,
}

impl DiskUsageService {
    pub fn new(
        index: Arc<FsIndexRepository>,
        dir_sizes: Arc<DirSizeRepository>,
        file_system: Arc<dyn FileSystemService>,
        mft_index: Arc<dyn DiskUsageIndex>,
        archives: Option<Arc<dyn DiskUsageArchives>>,
    ) -> DiskUsageService {
        DiskUsageService {
            index,
            dir_sizes,
            file_system,
            mft_index,
            archives,
        }
    }

    /// The biggest files under `root_path`, or across every indexed volume when it is None,
    /// largest first.
    ///
    /// Unscoped, this is a full scan of the index and takes seconds on a large disk. It is meant
    /// to be reached from an explicit "analyse this" gesture, never from typing, and the token is
    /// honoured so changing the root abandons the scan in flight.
    pub fn largest_files(
        &self,
        root_path: Option<&str>,
        limit: usize,
        include_hidden: bool,
        cancel: &CancelToken,
    ) -> Result<LargestFilesOutcome, DiskUsageError> {
        let root_path = root_path.filter(|p| !p.is_empty());

        // fs_entry knows nothing about what is inside a container, and must never be taught: its
        // rows are PathKey-keyed, and a virtual key sorts strictly inside the archive's own
        // containing folder as well as inside the archive. Answered from the container's index
        // instead, where the sizes are exact and already in memory, so this is both correct and
        // free.
        if let (Some(path), Some(archives)) = (root_path, &self.archives)
            && let Some(in_archive) = archives.resolve(path)
        {
            return Ok(self.largest_in_archive(archives.as_ref(), &in_archive, limit, cancel)?);
        }

        let root_key = match root_path {
            Some(p) => Some(PathKey::canonicalize(p)?),
            None => None,
        };
        let files = self.index.largest_files(root_path, limit, include_hidden)?;
        cancel.check()?;

        let is_indexed = match &root_key {
            None => self.mft_index.any_indexed(),
            Some(key) => self.mft_index.is_indexed(key),
        };
        let availability = DiskUsageRules::classify(
            root_key.as_deref(),
            files.len(),
            files.first().map_or(0, |f| f.size_bytes),
            self.mft_index.is_building(),
            is_indexed,
        );

        // Rows that are all zero are not a result set, they are the absence of one. Handing them
        // back would leave every caller re-deciding whether to draw them.
        Ok(if availability == DiskUsageAvailability::NoSizeData {
            LargestFilesOutcome {
                files: Vec::new(),
                availability,
            }
        } else {
            LargestFilesOutcome {
                files,
                availability,
            }
        })
    }

    /// The biggest entries under a folder inside a container, from the container's own index.
    ///
    /// Always `Ready`: an archive records every uncompressed length, so unlike a volume there is
    /// no "not indexed yet" state to be in. That is what makes pointing the treemap at a backup
    /// zip the one place this view is never approximate.
    fn largest_in_archive(
        &self,
        archives: &dyn DiskUsageArchives,
        root: &ArchivePath,
        limit: usize,
        cancel: &CancelToken,
    ) -> Result<LargestFilesOutcome, Cancelled> {
        let entries = match archives.files_under(root, cancel)? {
            ArchiveEntries::Unreadable => {
                return Ok(LargestFilesOutcome {
                    files: Vec::new(),
                    availability: DiskUsageAvailability::NotIndexed,
                });
            }
            ArchiveEntries::NotFound => {
                return Ok(LargestFilesOutcome {
                    files: Vec::new(),
                    availability: DiskUsageAvailability::Ready,
                });
            }
            ArchiveEntries::Files(files) => files,
        };

        let mut files: Vec<SearchHit> = entries
            .into_iter()
            .map(|e| SearchHit {
                display_path: ArchivePath::compose(&root.archive_file, &e.entry_path),
                relative_dir_display: directory_name(&e.entry_path).to_string(),
                name: e.name,
                is_directory: false,
                size_bytes: e.size_bytes,
                modified_utc: e.modified_utc,
                hidden: false,
                attributes: 0,
                created_utc: FileTime::UNKNOWN,
                content_match: None,
            })
            .collect();

        files.sort_by_key(|f| std::cmp::Reverse(f.size_bytes));
        files.truncate(limit);

        Ok(LargestFilesOutcome {
            files,
            availability: DiskUsageAvailability::Ready,
        })
    }

    /// What `directory` is made of, by direct child, largest first.
    ///
    /// The file half of the answer never needs the index (those sizes come from the enumeration
    /// itself), so a leaf folder is fully answerable on a volume nothing has ever indexed.
    pub fn breakdown(
        &self,
        directory: &str,
        include_hidden: bool,
        cancel: &CancelToken,
    ) -> Result<DiskUsageBreakdown, DiskUsageError> {
        let root_key = PathKey::canonicalize(directory)?;
        let root_display = PathKey::normalize_display(directory)?;

        let entries = self.file_system.list_directory(directory)?;
        cancel.check()?;

        let visible: Vec<_> = entries
            .into_iter()
            .filter(|e| include_hidden || e.attributes & HIDDEN == 0)
            .collect();

        // One batched lookup for every subfolder's total, exactly as the file list and the folder
        // tree hydrate their size columns. Files need no lookup: the enumeration already carried
        // their real lengths.
        // Inside an archive there is nothing to look up: the lister already carried every
        // folder's exact recursive total, because the numbers were in the container's own
        // directory. Asking dir_size_cache would be a round trip that can only miss, and a miss
        // reads as "unknown", which would draw a treemap of files with every folder greyed out
        // and then have `classify_breakdown` blame the index for it.
        let inside_archive =
            ArchivePath::looks_virtual(directory) && !std::path::Path::new(directory).is_dir();

        let dir_keys: Vec<String> = if inside_archive {
            Vec::new()
        } else {
            visible
                .iter()
                .filter(|e| e.is_directory)
                .map(|e| PathKey::canonicalize(&e.full_path))
                .collect::<Result<_, _>>()?
        };
        let cached = if dir_keys.is_empty() {
            HashMap::new()
        } else {
            let borrowed: Vec<&str> = dir_keys.iter().map(String::as_str).collect();
            self.dir_sizes.get_many(&borrowed)?
        };
        cancel.check()?;

        let mut children = Vec::with_capacity(visible.len());
        for entry in &visible {
            let key = PathKey::canonicalize(&entry.full_path)?;
            let mut incomplete = false;
            let size = if entry.is_directory && inside_archive {
                Some(entry.size_bytes)
            } else if entry.is_directory {
                // A missing row is unknown, and stays None all the way to the view.
                match cached.get(&key) {
                    Some(row) => {
                        incomplete = row.incomplete;
                        Some(row.size_bytes)
                    }
                    None => None,
                }
            } else {
                Some(entry.size_bytes)
            };

            children.push(DiskUsageNode {
                path_key: key,
                display_path: entry.full_path.clone(),
                name: entry.name.clone(),
                is_directory: entry.is_directory,
                size_bytes: size,
                incomplete,
                hidden: entry.attributes & HIDDEN != 0,
            });
        }

        // Largest first, unknowns last: they have no area to give and belong out of the way.
        children.sort_by_key(|c| std::cmp::Reverse(c.size_bytes.unwrap_or(-1)));

        let total = if inside_archive {
            Some(children.iter().map(|c| c.size_bytes.unwrap_or(0)).sum())
        } else {
            self.dir_sizes.get(directory)?.map(|r| r.size_bytes)
        };
        let unknown_count = children.iter().filter(|c| c.size_bytes.is_none()).count();

        // Ready is passed directly rather than through `classify_breakdown`, and the distinction
        // is the same one that makes it a separate function from `classify`: it weighs evidence
        // *about the index*, and inside a container there is no index to have evidence about.
        // Asking it would be the same category error, one level down, and it would answer
        // NoSizeData about numbers that are exact.
        let availability = if inside_archive {
            DiskUsageAvailability::Ready
        } else {
            DiskUsageRules::classify_breakdown(
                dir_keys.len(),
                children
                    .iter()
                    .filter(|c| c.is_directory && c.size_bytes.is_some())
                    .count(),
                self.mft_index.is_building(),
                self.mft_index.is_indexed(&root_key),
            )
        };

        let unaccounted = DiskUsageRules::unaccounted(total, &children);
        Ok(DiskUsageBreakdown {
            root_display_path: root_display,
            total_bytes: total,
            children,
            unaccounted_bytes: unaccounted,
            unknown_child_count: unknown_count,
            availability,
        })
    }
}

/// `Path.GetDirectoryName` over an entry path: everything before the last separator, or "".
fn directory_name(entry_path: &str) -> &str {
    match entry_path.rfind(['/', '\\']) {
        Some(i) => &entry_path[..i],
        None => "",
    }
}
