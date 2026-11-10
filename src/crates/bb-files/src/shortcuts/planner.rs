//! Works out which `.lnk` files "Create shortcuts here" would write, and where.
//! Spec: Core/Services/Shortcuts/ShortcutPlanner.cs.
//!
//! The naming is Explorer's, extension included: `notes.txt` becomes `notes.txt - Shortcut.lnk`,
//! and a name already taken steps aside through `UniquePath` to `notes.txt - Shortcut (2).lnk` —
//! the same "(2)" the rest of the app produces for a paste or a New File.
//!
//! **Names are reserved as the plan is built**, not merely probed. One drag can carry two files
//! called `notes.txt` from different folders; nothing is on disk yet, so a planner that only asked
//! the probe would hand both the same link path and the second write would replace the first.
//!
//! A source that is not on disk is refused, and that single rule is what keeps items inside an
//! archive out: `C:\x\bundle.zip\notes.txt` reads like a path but names nothing a shortcut could
//! resolve. No archive-specific check is needed, and none is here.

use std::collections::HashSet;

use bb_paths::{PathKey, UniquePath, file_name};

use super::{RejectedShortcut, ShortcutCreation, ShortcutPlan};
use crate::rename::paths::{self, combine};

/// The filesystem questions the planner asks, a trait so the naming and collision rules can be
/// unit-tested without writing `.lnk` files.
pub trait ShortcutProbe {
    fn directory_exists(&self, path: &str) -> bool;

    fn file_exists(&self, path: &str) -> bool;
}

/// The real one, over the local disk.
pub struct FileSystemShortcutProbe;

impl ShortcutProbe for FileSystemShortcutProbe {
    fn directory_exists(&self, path: &str) -> bool {
        paths::directory_exists(path)
    }

    fn file_exists(&self, path: &str) -> bool {
        paths::file_exists(path)
    }
}

pub static LOCAL_PROBE: FileSystemShortcutProbe = FileSystemShortcutProbe;

const SUFFIX: &str = " - Shortcut";
const EXTENSION: &str = ".lnk";

pub struct ShortcutPlanner<'a> {
    probe: &'a dyn ShortcutProbe,
}

impl<'a> ShortcutPlanner<'a> {
    pub fn new(probe: &'a dyn ShortcutProbe) -> ShortcutPlanner<'a> {
        ShortcutPlanner { probe }
    }

    /// A planner over the real disk.
    pub fn local() -> ShortcutPlanner<'static> {
        ShortcutPlanner {
            probe: &LOCAL_PROBE,
        }
    }

    /// What creating shortcuts to `sources` in `directory` would produce. Never fails: an unusable
    /// path is a refusal like any other.
    pub fn plan(&self, sources: &[String], directory: &str) -> ShortcutPlan {
        if !is_usable(directory) || !self.probe.directory_exists(directory) {
            let problems = if sources.is_empty() {
                Vec::new()
            } else {
                vec![RejectedShortcut::new(
                    "",
                    "The folder to create the shortcuts in is no longer there.".to_string(),
                )]
            };
            return ShortcutPlan::new(directory, Vec::new(), problems);
        }

        let mut creations = Vec::new();
        let mut problems = Vec::new();

        // Link paths this plan has already claimed. Canonicalized, because two sources may differ
        // only in case and "notes.txt" must still collide with "NOTES.TXT".
        let mut reserved: HashSet<String> = HashSet::new();

        for source in sources {
            if !is_usable(source)
                || !(self.probe.file_exists(source) || self.probe.directory_exists(source))
            {
                problems.push(RejectedShortcut::new(
                    source,
                    format!("'{}' is no longer there.", file_name(source)),
                ));
                continue;
            }

            let name = file_name(source.trim_end_matches('\\'));
            if name.is_empty() {
                // A drive root has no file name to build one from.
                problems.push(RejectedShortcut::new(
                    source,
                    "A drive root has no name to make a shortcut from.".to_string(),
                ));
                continue;
            }

            let link = UniquePath::for_path(
                &combine(directory, &format!("{name}{SUFFIX}{EXTENSION}")),
                false,
                |p| self.probe.directory_exists(p),
                |candidate| {
                    PathKey::canonicalize(candidate).is_ok_and(|k| reserved.contains(&k))
                        || self.probe.file_exists(candidate)
                },
            );

            if let Ok(key) = PathKey::canonicalize(&link) {
                reserved.insert(key);
            }
            creations.push(ShortcutCreation::new(source, &link));
        }

        ShortcutPlan::new(directory, creations, problems)
    }
}

/// Whether a path can be canonicalized at all. The other planners swallow exactly this for the
/// same reason: an unusable path is a refusal to show, not a failure.
fn is_usable(path: &str) -> bool {
    PathKey::canonicalize(path).is_ok()
}
