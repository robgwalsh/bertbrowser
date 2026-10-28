//! Works out what renaming a selection to a pattern would produce, without touching anything. Every
//! rule that stops a rename destroying something lives here: nothing is ever renamed onto a name
//! that is already taken, and two items can never be planned onto the same name.
//! Spec: Core/Services/Rename/RenamePlanner.cs.
//!
//! A name held by another item *in the same batch* is not a collision — that item is about to
//! vacate it, which is what makes rotating a set of names work — but only if that item is itself
//! being renamed. If it was rejected, it stays put, and whoever aimed at its name is rejected too.

use std::collections::HashSet;

use bb_paths::{PathKey, file_name};

use super::paths::{combine, directory_name, trim_ending_separator};
use super::{
    PlannedRename, RejectedRename, RenamePattern, RenamePlan, RenameRejection, RenameRule,
    RenameSource, paths,
};
use crate::archives::ArchivePath;

/// The filesystem questions the planner asks. A trait so the collision rules can be unit-tested
/// against layouts that would otherwise need real files on a real disk.
pub trait RenameProbe {
    fn directory_exists(&self, path: &str) -> bool;

    fn file_exists(&self, path: &str) -> bool;
}

/// The real one, over the local disk.
pub struct FileSystemRenameProbe;

impl RenameProbe for FileSystemRenameProbe {
    fn directory_exists(&self, path: &str) -> bool {
        paths::directory_exists(path)
    }

    fn file_exists(&self, path: &str) -> bool {
        paths::file_exists(path)
    }
}

static LOCAL: FileSystemRenameProbe = FileSystemRenameProbe;

pub struct RenamePlanner<'a> {
    probe: &'a dyn RenameProbe,
}

impl<'a> RenamePlanner<'a> {
    pub fn new(probe: &'a dyn RenameProbe) -> RenamePlanner<'a> {
        RenamePlanner { probe }
    }

    /// A planner over the real disk.
    pub fn local() -> RenamePlanner<'static> {
        RenamePlanner { probe: &LOCAL }
    }

    /// What renaming these to this typed name would produce — the plain rename box.
    pub fn plan_simple(&self, sources: &[RenameSource], pattern: &str) -> RenamePlan {
        self.plan(sources, &RenameRule::simple(pattern))
    }

    /// What renaming these under this rule would produce.
    ///
    /// Every rule below the naming is shared with the plain path, deliberately: an advanced
    /// rename produces a different set of names, but "nothing lands on a taken name" and "two
    /// items can never be planned onto one name" are the same questions either way, and there is
    /// one place to audit them.
    pub fn plan(&self, sources: &[RenameSource], rule: &RenameRule) -> RenamePlan {
        // A rule that cannot be used at all is not any one item's problem, so it is reported once
        // with no source path rather than repeated against every row.
        if let Some(unusable) = RenamePattern::validate_rule(rule) {
            return RenamePlan::new(
                Vec::new(),
                vec![RejectedRename::new(
                    "",
                    RenameRejection::InvalidRule,
                    unusable,
                )],
            );
        }

        let distinct = distinct(sources);
        if distinct.is_empty() {
            return RenamePlan::empty();
        }

        let names = RenamePattern::apply(
            &distinct
                .iter()
                .map(|(s, _)| (*s).clone())
                .collect::<Vec<_>>(),
            rule,
        );

        let mut renames: Vec<PlannedRename> = Vec::new();
        let mut rejected: Vec<RejectedRename> = Vec::new();

        // Names spoken for by earlier entries in this same plan, so two items can never be
        // planned onto one name however the pattern was written.
        let mut claimed: HashSet<String> = HashSet::new();
        let source_keys: HashSet<&str> = distinct.iter().map(|(_, k)| k.as_str()).collect();
        // The batch members that are really on disk, and so really in the way until they move.
        let mut occupied: HashSet<String> = HashSet::new();
        // Selected folders, so an item that would have the ground moved out from under it can be
        // recognized before anything is planned.
        let directory_keys: HashSet<String> = distinct
            .iter()
            .filter(|(s, _)| self.probe.directory_exists(&s.path))
            .map(|(_, k)| k.clone())
            .collect();

        for ((source, key), named) in distinct.iter().zip(names) {
            let path = source.path.as_str();
            let leaf = file_name(path);

            let is_directory = self.probe.directory_exists(path);
            if !is_directory && !self.probe.file_exists(path) {
                // See DeletePlanner: an entry inside an archive fails the same existence test, and
                // deserves the honest message rather than being told it is gone.
                let inside_archive = ArchivePath::parse(path, paths::file_exists).is_some();
                rejected.push(RejectedRename::new(
                    path,
                    RenameRejection::SourceMissing,
                    if inside_archive {
                        format!("'{leaf}' is inside an archive. Extract it first.")
                    } else {
                        format!("'{leaf}' no longer exists.")
                    },
                ));
                continue;
            }
            occupied.insert(key.clone());

            let Some(parent) = parent_of(path) else {
                rejected.push(RejectedRename::new(
                    path,
                    RenameRejection::SourceIsRoot,
                    format!("'{path}' is a drive root and cannot be renamed."),
                ));
                continue;
            };

            if PathKey::has_ancestor_key_in(key, &directory_keys) {
                rejected.push(RejectedRename::new(
                    path,
                    RenameRejection::InsideARenamedFolder,
                    format!("'{leaf}' is inside a folder that is being renamed too."),
                ));
                continue;
            }

            // This one item's name could not be built — no date for the {modified} the template
            // asked for, or an expression that ran past its limit against this name in
            // particular. Reported against the item, since the rule itself is fine.
            if let Some(problem) = named.problem {
                rejected.push(RejectedRename::new(
                    path,
                    RenameRejection::InvalidRule,
                    problem,
                ));
                continue;
            }

            let name = named.name;
            if let Some(problem) = RenamePattern::validate(&name) {
                rejected.push(RejectedRename::new(
                    path,
                    RenameRejection::InvalidName,
                    problem,
                ));
                continue;
            }

            let target = combine(&parent, &name);
            let Ok(target_key) = PathKey::canonicalize(&target) else {
                rejected.push(RejectedRename::new(
                    path,
                    RenameRejection::InvalidName,
                    "That name can't be used here.".to_string(),
                ));
                continue;
            };

            if !claimed.insert(target_key.clone()) {
                rejected.push(RejectedRename::new(
                    path,
                    RenameRejection::NameTaken,
                    format!("Two of the selected items would both be named '{name}'."),
                ));
                continue;
            }

            // Occupied by something outside the batch is a hard stop; occupied by something inside
            // it is provisionally fine — the second pass below confirms that item really moves.
            if target_key != *key
                && !source_keys.contains(target_key.as_str())
                && self.exists(&target)
            {
                rejected.push(RejectedRename::new(
                    path,
                    RenameRejection::NameTaken,
                    format!("'{name}' already exists in this folder."),
                ));
                continue;
            }

            renames.push(PlannedRename::new(path, &target, is_directory));
        }

        confirm_vacancies(&mut renames, &mut rejected, &occupied);
        RenamePlan::new(renames, rejected)
    }

    fn exists(&self, path: &str) -> bool {
        self.probe.directory_exists(path) || self.probe.file_exists(path)
    }
}

/// Second pass: an item aimed at a name held by another selected item is only safe if that item
/// is itself leaving. Dropping one item can therefore doom another that was counting on it to
/// move, so this repeats until nothing more changes.
fn confirm_vacancies(
    renames: &mut Vec<PlannedRename>,
    rejected: &mut Vec<RejectedRename>,
    occupied: &HashSet<String>,
) {
    loop {
        let vacating: HashSet<String> = renames
            .iter()
            .filter(|r| !r.is_no_op())
            .filter_map(|r| PathKey::canonicalize(&r.source_path).ok())
            .collect();

        let doomed = renames.iter().position(|r| {
            if r.is_no_op() {
                return false;
            }
            let (Ok(target_key), Ok(source_key)) = (
                PathKey::canonicalize(&r.target_path),
                PathKey::canonicalize(&r.source_path),
            ) else {
                return false;
            };
            // Its own name, cased differently, is not something it has to wait for.
            if target_key == source_key {
                return false;
            }
            occupied.contains(&target_key) && !vacating.contains(&target_key)
        });
        let Some(index) = doomed else {
            return;
        };

        let doomed = renames.remove(index);
        rejected.push(RejectedRename::new(
            &doomed.source_path,
            RenameRejection::NameTaken,
            format!("'{}' already exists in this folder.", doomed.target_name()),
        ));
    }
}

/// Drops repeats — a selection can name the same item twice only by accident, and renaming it
/// twice would number it twice. A path that cannot be canonicalized is left out the same way.
fn distinct(sources: &[RenameSource]) -> Vec<(&RenameSource, String)> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut result = Vec::new();
    for source in sources {
        if source.path.trim().is_empty() {
            continue;
        }
        let Ok(key) = PathKey::canonicalize(&source.path) else {
            continue;
        };
        if seen.insert(key.clone()) {
            result.push((source, key));
        }
    }
    result
}

/// The folder an item sits in, fully qualified, or None at a root.
fn parent_of(path: &str) -> Option<String> {
    let full = PathKey::normalize_display(path).ok()?;
    directory_name(trim_ending_separator(&full)).filter(|p| !p.is_empty())
}
