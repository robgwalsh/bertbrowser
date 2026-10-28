//! The judgements a duplicate scan has to make before it can show or act on anything, kept pure
//! and away from both SQLite and the filesystem so they can be tested rather than eyeballed. Spec:
//! Core/Services/Duplicates/DuplicateRules.cs.
//!
//! Two of them exist to protect the same invariant the rest of this app is shaped around — an
//! unmeasured thing is unknown, never zero. A scan that shortlists on size is silently useless on
//! a volume whose sizes are all zero, and a results list cannot tell that apart from a disk with no
//! duplicates on it. `classify` is where that is decided, once.

use std::sync::OnceLock;

use bb_paths::PathKey;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesCommon,
    FOLDERID_ProgramFilesCommonX86, FOLDERID_ProgramFilesX86, FOLDERID_System, FOLDERID_SystemX86,
    FOLDERID_Windows, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows_sys::core::GUID;

use super::{DuplicateFile, DuplicateGroup, DuplicateScanAvailability};
use crate::fs::is_held_path;

/// Which copy of a group to keep when the user asks for the others to be ticked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeepStrategy {
    /// The most recently modified.
    Newest,
    /// The least recently modified — usually the original.
    Oldest,
    /// The shortest path, which is the copy nearest the top of the tree.
    Shallowest,
}

pub struct DuplicateRules;

impl DuplicateRules {
    /// How much of a file the first hashing pass reads. Big enough that files differing anywhere
    /// in a header, a container's index or the first frame separate here for almost no I/O; small
    /// enough that a group of a hundred same-sized files costs six megabytes to eliminate rather
    /// than gigabytes. A file this size or smaller is *fully* hashed by that pass, so it never
    /// reaches the second one.
    pub const HEAD_SAMPLE_BYTES: i64 = 64 * 1024;

    /// What standing a scan's answer has, given what the index turned out to hold.
    /// `files_in_scope` is every file row in range, whatever its length; `sized_files_in_scope`
    /// how many of those have a real byte length — rows but no lengths is decisive: it is the
    /// sizeless `FSCTL_ENUM_USN_DATA` build, on which every file collides with every other and the
    /// shortlist means nothing. The evidence is weighed before the index service's own opinion,
    /// deliberately: a network share or a removable disk is filled in by the crawler rather than
    /// the MFT pass, so `is_indexed` is false there while the rows — and their real sizes — are
    /// perfectly good. Asking the service first would refuse to scan a folder it could scan.
    pub fn classify(
        files_in_scope: i64,
        sized_files_in_scope: i64,
        is_building: bool,
        is_indexed: bool,
    ) -> DuplicateScanAvailability {
        // Real lengths in range: the shortlist is meaningful whatever anyone thinks of the index.
        if sized_files_in_scope > 0 {
            return if is_building && !is_indexed {
                DuplicateScanAvailability::Building
            } else {
                DuplicateScanAvailability::Ready
            };
        }

        // Rows, and not one of them with a length. Still building is a "come back in a moment";
        // otherwise this is the name-only build and saying "no duplicates" would be a lie.
        if files_in_scope > 0 {
            return if is_building {
                DuplicateScanAvailability::Building
            } else {
                DuplicateScanAvailability::NoSizeData
            };
        }

        // No file rows at all.
        if is_building {
            return DuplicateScanAvailability::Building;
        }

        // An indexed scope that really is empty has been scanned correctly and found nothing,
        // which is a result. An unindexed one has not been looked at, which is not.
        if is_indexed {
            DuplicateScanAvailability::Ready
        } else {
            DuplicateScanAvailability::NotIndexed
        }
    }

    /// Whether a candidate should be left out of the scan altogether. Held paths go for the
    /// reason the search drops them: those files are still on disk so a Ctrl+Z can reach them,
    /// but they have been deleted as far as the user is concerned, and offering to delete one
    /// again reads as a delete that silently failed.
    pub fn is_excluded(path_key: &str, skip_system_folders: bool) -> bool {
        is_held_path(path_key) || (skip_system_folders && Self::is_system_subtree(path_key))
    }

    /// True for anything inside Windows, Program Files, or ProgramData.
    ///
    /// This is deliberately not the protected-locations set, and the difference runs both ways.
    /// That set is exact-match and covers the folders themselves only, because its job is to
    /// refuse the one selection that ends the machine; this one has to cover whole subtrees,
    /// because its job is to keep results the user cannot act on out of the list. And it must
    /// *not* include the profile root, which that set does: the profile is where a person's
    /// duplicates actually live. The bounds are computed once — `PathKey::is_under`
    /// re-canonicalizes on every call, which is fine for a menu and ruinous once per row of a
    /// shortlist.
    pub fn is_system_subtree(path_key: &str) -> bool {
        system_subtrees()
            .iter()
            .any(|(lo, hi)| path_key >= lo.as_str() && path_key < hi.as_str())
    }

    /// Whether removing `ticked_count` of a group of `group_size` is something this feature will
    /// do. **A group may never have every copy ticked.** The point of the feature is to reclaim
    /// the space a redundant copy costs, which means one copy always stays; a batch that removed
    /// all of them would destroy the only remaining instance of a file the user was told they had
    /// several of. It is checked here, purely, as well as in the view, so a test can hold it still.
    pub fn can_remove(group_size: usize, ticked_count: usize) -> bool {
        ticked_count > 0 && group_size > 1 && ticked_count < group_size
    }

    /// Which copy of `group` to keep under `strategy`; every other one is what gets ticked. An
    /// index into `group.files`. Ties are broken by ordinal path so the same group always yields
    /// the same keeper — an auto-selection that shuffled between presses would be impossible to
    /// trust.
    ///
    /// # Panics
    /// On an empty group, which cannot reach here.
    pub fn choose_keeper(group: &DuplicateGroup, strategy: KeepStrategy) -> usize {
        assert!(!group.files.is_empty(), "Group is empty.");

        let mut best = 0;
        for i in 1..group.files.len() {
            if Self::is_better_keeper(&group.files[i], &group.files[best], strategy) {
                best = i;
            }
        }
        best
    }

    fn is_better_keeper(
        candidate: &DuplicateFile,
        incumbent: &DuplicateFile,
        strategy: KeepStrategy,
    ) -> bool {
        let verdict = match strategy {
            KeepStrategy::Newest => candidate.modified_utc.cmp(&incumbent.modified_utc),
            KeepStrategy::Oldest => incumbent.modified_utc.cmp(&candidate.modified_utc),
            KeepStrategy::Shallowest => {
                utf16_len(&incumbent.display_path).cmp(&utf16_len(&candidate.display_path))
            }
        };

        // The tiebreak is not decoration: several copies written by one unzip share a timestamp
        // to the tick, and that is exactly the case this feature is most often pointed at.
        verdict.is_gt()
            || (verdict.is_eq()
                && candidate
                    .display_path
                    .encode_utf16()
                    .lt(incumbent.display_path.encode_utf16()))
    }
}

/// `string.Length`: UTF-16 units, which is what the C# compared.
fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn system_subtrees() -> &'static [(String, String)] {
    static BOUNDS: OnceLock<Vec<(String, String)>> = OnceLock::new();
    BOUNDS.get_or_init(build_system_subtrees)
}

fn build_system_subtrees() -> Vec<(String, String)> {
    let folders = [
        &FOLDERID_Windows,
        &FOLDERID_System,
        &FOLDERID_SystemX86,
        &FOLDERID_ProgramFiles,
        &FOLDERID_ProgramFilesX86,
        &FOLDERID_ProgramFilesCommon,
        &FOLDERID_ProgramFilesCommonX86,
        &FOLDERID_ProgramData,
    ];

    let mut bounds: Vec<(String, String)> = Vec::new();
    for folder in folders {
        // The shell answers nothing for a folder this machine does not have.
        let Some(path) = known_folder(folder) else {
            continue;
        };
        if path.trim().is_empty() {
            continue;
        }
        // Not a path anything could be under either.
        let Ok(key) = PathKey::canonicalize(&path) else {
            continue;
        };
        let Ok(pair) = PathKey::prefix_bounds(&key) else {
            continue;
        };
        if !bounds.contains(&pair) {
            bounds.push(pair);
        }
    }
    bounds
}

/// `SHGetKnownFolderPath`, as `Environment.GetFolderPath` asked.
fn known_folder(id: &GUID) -> Option<String> {
    let mut wide: *mut u16 = std::ptr::null_mut();
    // SAFETY: the out pointer is valid for the call; the string it receives is freed below.
    let hr = unsafe {
        SHGetKnownFolderPath(id, KF_FLAG_DEFAULT as u32, std::ptr::null_mut(), &mut wide)
    };
    if hr < 0 || wide.is_null() {
        return None;
    }
    let mut len = 0;
    // SAFETY: a successful call returns a NUL-terminated string.
    while unsafe { *wide.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` characters are initialised, as counted above.
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(wide, len) });
    // SAFETY: the shell allocated this with the COM allocator and we own it.
    unsafe { CoTaskMemFree(wide as *const _) };
    Some(text)
}
