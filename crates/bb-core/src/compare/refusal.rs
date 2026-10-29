//! Whether two folders may be compared at all. Spec: Core/Services/Compare/CompareRefusal.cs.
//!
//! Pure but for the probe, because every one of these is a data-safety rule rather than a
//! convenience: a comparison that runs on the wrong pair is what makes "delete what is only on
//! the right" delete the wrong thing.

use bb_paths::PathKey;

use crate::archives::ArchivePath;

/// The filesystem questions a comparison has to ask before it starts, injected so the containment
/// rules can be tested against link layouts that are impractical to create.
pub trait CompareProbe {
    fn directory_exists(&self, path: &str) -> bool;

    fn file_exists(&self, path: &str) -> bool;

    /// The physical path with every junction and symlink resolved, best effort.
    fn resolve_final_path(&self, path: &str) -> String;
}

pub struct CompareRefusal;

impl CompareRefusal {
    /// The reason to refuse, or None to go ahead.
    pub fn check(left_path: &str, right_path: &str, probe: &dyn CompareProbe) -> Option<String> {
        if left_path.trim().is_empty() || right_path.trim().is_empty() {
            return Some("Compare needs a folder open on each side.".to_string());
        }

        // A path inside an archive is a real Windows path syntactically, so it canonicalizes
        // happily and lands inside the prefix bounds of the container's own folder, after which a
        // subtree query over that folder returns archive interiors. Refused outright, as search,
        // bookmarks and disk usage each refuse it.
        let is_file = |p: &str| probe.file_exists(p);
        if ArchivePath::parse(left_path, is_file).is_some()
            || ArchivePath::parse(right_path, is_file).is_some()
        {
            return Some(
                "Comparing inside an archive is not supported. Extract it first.".to_string(),
            );
        }

        if !probe.directory_exists(left_path) || !probe.directory_exists(right_path) {
            return Some("Compare needs a folder on both sides.".to_string());
        }

        // Checked literally and again on resolved paths, the double check the transfer planner
        // makes, and for the same reason: a junction can put one of these inside the other without
        // either path saying so. Comparing a folder with its own subtree would have every file show
        // up twice over and "only on the right" would name the tree itself.
        if Self::overlaps(left_path, right_path)
            || Self::overlaps(
                &probe.resolve_final_path(left_path),
                &probe.resolve_final_path(right_path),
            )
        {
            return Some(
                "Compare needs two different folders, neither inside the other.".to_string(),
            );
        }

        None
    }

    fn overlaps(a: &str, b: &str) -> bool {
        // A path that cannot be made into a key cannot be compared with anything, and the folder
        // check already refused it; here it simply does not overlap.
        let (Ok(left), Ok(right)) = (PathKey::canonicalize(a), PathKey::canonicalize(b)) else {
            return false;
        };
        left == right
            || PathKey::is_under_key(&left, &right)
            || PathKey::is_under_key(&right, &left)
    }
}

/// Real-filesystem [`CompareProbe`].
// TODO(merge): `resolve_final_path` is delegated in C# to `Transfer.FileSystemTransferProbe` so
// there is one definition of "where does this junction actually go"; swap this body for
// `crate::transfer::FileSystemTransferProbe::resolve_final_path` once that lands.
#[derive(Default)]
pub struct FileSystemCompareProbe;

impl CompareProbe for FileSystemCompareProbe {
    fn directory_exists(&self, path: &str) -> bool {
        std::fs::metadata(path).is_ok_and(|m| m.is_dir())
    }

    fn file_exists(&self, path: &str) -> bool {
        std::fs::metadata(path).is_ok_and(|m| m.is_file())
    }

    fn resolve_final_path(&self, path: &str) -> String {
        match std::fs::canonicalize(path) {
            Ok(resolved) => {
                let s = resolved.to_string_lossy();
                s.strip_prefix(r"\\?\UNC\")
                    .map(|rest| format!(r"\\{rest}"))
                    .or_else(|| s.strip_prefix(r"\\?\").map(str::to_string))
                    .unwrap_or_else(|| s.into_owned())
            }
            Err(_) => path.to_string(),
        }
    }
}
