//! Listing two folders and handing both to `FolderComparer`. Spec:
//! Core/Services/Compare/FolderCompareService.cs.
//!
//! Each side chooses its own source. A complete, unstale, live-tailed index that actually carries
//! byte lengths is read with one range scan; anything else is walked, and the two sides may well
//! differ. That choice is deliberately *not* something the user is told: unlike a duplicate scan,
//! which *is* the index, a comparison always has an answer available, so "the index cannot help
//! here" picks a source rather than raising a message. The agreement test is what keeps the two
//! sources from meaning different things by it. The sides are read one after the other rather
//! than in parallel: they are usually the same disk, and an index scan is bound by SQLite on one
//! connection either way.

use std::sync::Arc;

use bb_paths::{PathKey, root_len};
use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;

use super::equality::CompareTolerance;
use super::folder_comparer::{CompareResult, FolderComparer};
use super::models::{CompareEntry, CompareSourceKind};
use super::refusal::{CompareProbe, CompareRefusal, FileSystemCompareProbe};
use crate::cancel::CancelToken;
use crate::data::{self, FsIndexRepository};
use crate::fs::{FileSystemWalker, WalkError, is_held_path};
use crate::mft::MftIndex;

/// Whether a comparison could be run, and how far to trust it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareAvailability {
    Ready,
    /// A volume's first enumeration is still running, so a side read from the index is a floor
    /// rather than a listing. The verdicts stand, but something may still be missing.
    Building,
    /// The pair was refused before anything was read. `problem` says why.
    Refused,
    /// A side could not be listed at all: access denied at its root, or it went away between
    /// being chosen and being read.
    Unreadable,
}

/// How far a comparison has got. Two counters and no denominator: neither side's size is known
/// until its scan finishes, and a bar pinned at nothing reads as a stall.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CompareProgress {
    pub left_seen: usize,
    pub right_seen: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderCompareOutcome {
    pub left_path: String,
    pub right_path: String,
    pub result: CompareResult,
    pub availability: CompareAvailability,
    pub left_source: CompareSourceKind,
    pub right_source: CompareSourceKind,
    /// A side hit the entry ceiling, so what came back is a prefix of that folder rather than all
    /// of it.
    pub truncated: bool,
    pub cancelled: bool,
    pub problem: Option<String>,
}

impl FolderCompareOutcome {
    /// Whether this result may be turned into a sync. A refusal, an unreadable side, a cancelled
    /// run and a truncated scan each leave a listing that does not describe the folder it names,
    /// and "only on the right", read off a partial left side, would offer to delete files that
    /// are sitting right there.
    pub fn can_sync(&self) -> bool {
        matches!(
            self.availability,
            CompareAvailability::Ready | CompareAvailability::Building
        ) && !self.truncated
            && !self.cancelled
    }

    pub fn refused(left: &str, right: &str, problem: &str) -> FolderCompareOutcome {
        Self::empty(
            left,
            right,
            CompareAvailability::Refused,
            false,
            Some(problem.to_string()),
        )
    }

    fn empty(
        left: &str,
        right: &str,
        availability: CompareAvailability,
        cancelled: bool,
        problem: Option<String>,
    ) -> FolderCompareOutcome {
        FolderCompareOutcome {
            left_path: left.to_string(),
            right_path: right.to_string(),
            result: CompareResult::none(),
            availability,
            left_source: CompareSourceKind::Walk,
            right_source: CompareSourceKind::Walk,
            truncated: false,
            cancelled,
            problem,
        }
    }
}

/// The ceilings a comparison works within.
pub struct CompareLimits;

impl CompareLimits {
    /// How many entries one side may contribute. Measured against a real 1.65-million-row index:
    /// about 300 bytes and 1.5 microseconds an entry, so half a million a side is roughly 150 MB
    /// and a second of scanning each, a peak this app can afford for the seconds a comparison
    /// lasts. It clears the trees people actually compare (a large source checkout here is
    /// 261,000 entries) while stopping someone who points both panes at a drive root from
    /// spending a gigabyte finding that out.
    pub const MAX_ENTRIES_PER_SIDE: usize = 500_000;

    /// How often a walk reports its running count. Frequent enough to look alive, rare enough
    /// that the reporting is not what the walk is doing.
    pub(crate) const WALK_REPORT_INTERVAL: usize = 2_000;
}

/// What filesystem a path sits on, for the timestamp tolerance; None when the volume will not say.
pub type DriveFormat = dyn Fn(&str) -> Option<String> + Send + Sync;

pub struct FolderCompareService {
    index: Arc<FsIndexRepository>,
    mft: Arc<dyn MftIndex>,
    probe: Box<dyn CompareProbe + Send + Sync>,
    drive_format: Box<DriveFormat>,
}

struct SideListing {
    entries: Vec<CompareEntry>,
    source: CompareSourceKind,
    truncated: bool,
}

/// Why a side could not be listed.
enum ReadFailure {
    Cancelled,
    Unreadable(String),
}

impl From<data::Error> for ReadFailure {
    fn from(e: data::Error) -> Self {
        match e {
            data::Error::Cancelled => ReadFailure::Cancelled,
            other => ReadFailure::Unreadable(other.to_string()),
        }
    }
}

impl From<bb_paths::PathError> for ReadFailure {
    fn from(e: bb_paths::PathError) -> Self {
        ReadFailure::Unreadable(e.to_string())
    }
}

impl From<WalkError> for ReadFailure {
    fn from(e: WalkError) -> Self {
        match e {
            WalkError::Cancelled => ReadFailure::Cancelled,
            WalkError::Path(p) => ReadFailure::Unreadable(p.to_string()),
        }
    }
}

impl FolderCompareService {
    pub fn new(index: Arc<FsIndexRepository>, mft: Arc<dyn MftIndex>) -> FolderCompareService {
        FolderCompareService {
            index,
            mft,
            probe: Box::new(FileSystemCompareProbe),
            drive_format: Box::new(Self::drive_format),
        }
    }

    pub fn with_probe(
        mut self,
        probe: Box<dyn CompareProbe + Send + Sync>,
    ) -> FolderCompareService {
        self.probe = probe;
        self
    }

    /// Injected so a test can pose a FAT volume without one.
    pub fn with_drive_format(mut self, drive_format: Box<DriveFormat>) -> FolderCompareService {
        self.drive_format = drive_format;
        self
    }

    /// Compares two folders, blocking the caller. `progress` is told the running counts as each
    /// side is read.
    pub fn compare(
        &self,
        left_path: &str,
        right_path: &str,
        include_hidden: bool,
        cancel: &CancelToken,
        progress: Option<&dyn Fn(CompareProgress)>,
    ) -> FolderCompareOutcome {
        if let Some(problem) = CompareRefusal::check(left_path, right_path, self.probe.as_ref()) {
            return FolderCompareOutcome::refused(left_path, right_path, &problem);
        }

        let mut seen = CompareProgress::default();
        let report = |seen: CompareProgress| {
            if let Some(p) = progress {
                p(seen);
            }
        };

        let left = self.read(left_path, include_hidden, cancel, &mut |count| {
            seen.left_seen = count;
            report(seen);
        });
        let left = match left {
            Ok(l) => l,
            Err(failure) => return Self::failed(left_path, right_path, failure),
        };

        let right = self.read(right_path, include_hidden, cancel, &mut |count| {
            seen.right_seen = count;
            report(seen);
        });
        let right = match right {
            Ok(r) => r,
            Err(failure) => return Self::failed(left_path, right_path, failure),
        };

        let tolerance = CompareTolerance::for_formats(
            (self.drive_format)(left_path).as_deref(),
            (self.drive_format)(right_path).as_deref(),
        );
        let result = FolderComparer::compare(&left.entries, &right.entries, tolerance);

        // Only worth saying when a side actually came from the index: a walk is as complete as
        // the disk is, whatever the volume enumeration is doing elsewhere.
        let building = self.mft.is_building()
            && (left.source == CompareSourceKind::Index
                || right.source == CompareSourceKind::Index);

        FolderCompareOutcome {
            left_path: PathKey::normalize_display(left_path)
                .unwrap_or_else(|_| left_path.to_string()),
            right_path: PathKey::normalize_display(right_path)
                .unwrap_or_else(|_| right_path.to_string()),
            result,
            availability: if building {
                CompareAvailability::Building
            } else {
                CompareAvailability::Ready
            },
            left_source: left.source,
            right_source: right.source,
            truncated: left.truncated || right.truncated,
            cancelled: false,
            problem: None,
        }
    }

    fn failed(left_path: &str, right_path: &str, failure: ReadFailure) -> FolderCompareOutcome {
        match failure {
            ReadFailure::Cancelled => FolderCompareOutcome::empty(
                left_path,
                right_path,
                CompareAvailability::Ready,
                true,
                None,
            ),
            ReadFailure::Unreadable(message) => FolderCompareOutcome::empty(
                left_path,
                right_path,
                CompareAvailability::Unreadable,
                false,
                Some(format!("One of the folders could not be read. {message}")),
            ),
        }
    }

    fn read(
        &self,
        path: &str,
        include_hidden: bool,
        cancel: &CancelToken,
        report: &mut dyn FnMut(usize),
    ) -> Result<SideListing, ReadFailure> {
        if self.uses_index(path) {
            self.read_from_index(path, include_hidden, cancel, report)
        } else {
            Self::read_from_disk(path, include_hidden, cancel, report)
        }
    }

    /// Whether this side can be answered from the index.
    ///
    /// The last clause is the one that matters. The name-only build path writes every row with no
    /// length and no timestamp, and a comparison served from such a volume would find every file
    /// the same size as its twin. It would still be safe (a missing timestamp is `Unknown` at the
    /// leaf and nothing may be synced from it) but safe by refusing to say anything, where a walk
    /// gives a real answer. It cannot tell a volume with no lengths from a folder that honestly
    /// holds no bytes: such a folder falls to the walk, the harmless direction, and it is why the
    /// question is asked of the folder rather than of the volume.
    fn uses_index(&self, path: &str) -> bool {
        match self.index.find_covering_root(path) {
            Ok(Some(covering)) if !covering.stale => {
                self.mft.is_indexed(&covering.path_key)
                    && self.index.has_size_data(Some(path)).unwrap_or(false)
            }
            _ => false,
        }
    }

    fn read_from_index(
        &self,
        path: &str,
        include_hidden: bool,
        cancel: &CancelToken,
        report: &mut dyn FnMut(usize),
    ) -> Result<SideListing, ReadFailure> {
        let held = |key: &str| is_held_path(key);
        let (rows, truncated) = self.index.subtree(
            path,
            include_hidden,
            CompareLimits::MAX_ENTRIES_PER_SIDE,
            Some(&held),
            cancel,
        )?;

        let entries: Vec<CompareEntry> = rows
            .into_iter()
            .map(|row| CompareEntry {
                relative_key: row.relative_key,
                name: row.name,
                is_directory: row.is_directory,
                size_bytes: row.size_bytes,
                modified_utc: row.modified_utc,
            })
            .collect();

        report(entries.len());
        Ok(SideListing {
            entries,
            source: CompareSourceKind::Index,
            truncated,
        })
    }

    /// The walk twin. `FileSystemWalker` emits a junction or symlink as an entry and never
    /// descends it, and both index build paths hold a row for the link and none for anything
    /// beneath it, so the two sources agree about a linked folder by construction rather than by
    /// a rule written out twice and kept in step by hand.
    fn read_from_disk(
        path: &str,
        include_hidden: bool,
        cancel: &CancelToken,
        report: &mut dyn FnMut(usize),
    ) -> Result<SideListing, ReadFailure> {
        // The same slice the index scan takes, so a drive root does not leave a leading separator
        // on one side's keys and nothing pairs.
        let prefix_length = PathKey::prefix_bounds(&PathKey::canonicalize(path)?)?
            .0
            .len();
        let mut entries: Vec<CompareEntry> = Vec::new();
        let mut truncated = false;

        FileSystemWalker::walk(path, include_hidden, false, cancel, |entry| {
            if is_held_path(&entry.path_key) {
                return true;
            }

            if entries.len() == CompareLimits::MAX_ENTRIES_PER_SIDE {
                truncated = true;
                return false;
            }

            entries.push(CompareEntry {
                relative_key: entry.path_key[prefix_length..].to_string(),
                name: entry.name.clone(),
                is_directory: entry.is_directory,
                size_bytes: entry.size_bytes,
                modified_utc: entry.modified_utc,
            });

            if entries
                .len()
                .is_multiple_of(CompareLimits::WALK_REPORT_INTERVAL)
            {
                report(entries.len());
            }
            true
        })?;

        report(entries.len());
        Ok(SideListing {
            entries,
            source: CompareSourceKind::Walk,
            truncated,
        })
    }

    /// The volume's filesystem, or None when it will not say: a share, a disconnected mapping.
    /// None is what keeps the timestamp tolerance strict.
    fn drive_format(path: &str) -> Option<String> {
        let full = PathKey::normalize_display(path).ok()?;
        let root = &full[..root_len(&full)];
        if root.is_empty() {
            return None;
        }
        // GetVolumeInformationW wants the root with its trailing separator.
        let root = if root.ends_with('\\') {
            root.to_string()
        } else {
            format!("{root}\\")
        };
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let mut name = [0u16; 64];
        // SAFETY: every pointer is valid for the call and the buffer length is passed with it.
        let ok = unsafe {
            GetVolumeInformationW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                name.as_mut_ptr(),
                name.len() as u32,
            )
        };
        if ok == 0 {
            return None;
        }
        let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        Some(String::from_utf16_lossy(&name[..end]))
    }
}
