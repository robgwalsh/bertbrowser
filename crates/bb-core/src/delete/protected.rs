//! The handful of folders this app will not delete however they were selected.
//! Spec: Core/Services/Delete/ProtectedLocations.cs.
//!
//! **The profile root is why this list still exists.** The app runs as the user now, so Windows
//! itself refuses a delete of `Windows` or `Program Files` — but the user profile is entirely
//! writable by its owner, and a stray Ctrl+A there would find nothing in the way. The system
//! folders stay as belt and braces: cheap, and they keep the rule honest on a machine where the
//! app has somehow been started elevated anyway.
//!
//! Only the folders themselves are protected, not what is inside them: the point is to refuse the
//! one selection that ends the machine, not to make the app decline to manage files. Drive and
//! volume roots are refused separately by the planner, which is what makes exact matching enough
//! here — every entry sits directly under a root that is already unreachable.

use std::collections::HashSet;
use std::sync::OnceLock;

use bb_paths::PathKey;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_Profile, FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesCommon,
    FOLDERID_ProgramFilesCommonX86, FOLDERID_ProgramFilesX86, FOLDERID_System, FOLDERID_SystemX86,
    FOLDERID_Windows,
};
use windows_sys::core::GUID;

pub struct ProtectedLocations;

impl ProtectedLocations {
    /// The per-volume Recycle Bin folder, and the name Windows used before it.
    const RECYCLE_BIN_FOLDERS: [&'static str; 3] = ["$Recycle.Bin", "$RECYCLE.BIN", "RECYCLER"];

    /// Canonical keys of the locations refused by default.
    pub fn default_keys() -> &'static HashSet<String> {
        static DEFAULT: OnceLock<HashSet<String>> = OnceLock::new();
        DEFAULT.get_or_init(Self::build)
    }

    /// True for a Recycle Bin folder or anything inside one. Unlike the exact matches above this
    /// covers the contents too, and deliberately: those `$R` files are what Ctrl+Z restores from,
    /// so deleting one out from under a pending undo would break it. It is a name test rather than
    /// a path list because the folder exists once per volume and enumerating drives to find them
    /// would mean touching every disconnected network share at startup.
    pub fn is_inside_recycle_bin(path: &str) -> bool {
        path.split(['\\', '/']).any(Self::is_recycle_bin_name)
    }

    /// True when one path segment is a Recycle Bin folder's name.
    pub fn is_recycle_bin_name(segment: &str) -> bool {
        Self::RECYCLE_BIN_FOLDERS
            .iter()
            .any(|folder| segment.eq_ignore_ascii_case(folder))
    }

    /// Canonicalizes `paths` into a set the planner can test against, dropping anything that is
    /// not a usable path — not a path we could ever be asked to delete either.
    pub fn keys_of<'a>(paths: impl IntoIterator<Item = &'a str>) -> HashSet<String> {
        paths
            .into_iter()
            .filter(|p| !p.trim().is_empty())
            .filter_map(|p| PathKey::canonicalize(p).ok())
            .collect()
    }

    fn build() -> HashSet<String> {
        let folders = [
            &FOLDERID_Windows,
            &FOLDERID_System,
            &FOLDERID_SystemX86,
            &FOLDERID_ProgramFiles,
            &FOLDERID_ProgramFilesX86,
            &FOLDERID_ProgramFilesCommon,
            &FOLDERID_ProgramFilesCommonX86,
            &FOLDERID_ProgramData,
            &FOLDERID_Profile,
        ];
        let paths: Vec<String> = folders.into_iter().filter_map(known_folder).collect();
        Self::keys_of(paths.iter().map(String::as_str))
    }
}

fn known_folder(id: &GUID) -> Option<String> {
    crate::fs::KnownFolders::path_of(id)
}
