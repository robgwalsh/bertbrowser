//! Carries out a `RenamePlan`. Nothing is ever overwritten: every move refuses an existing target,
//! and a name that turns out to be taken fails that one item and leaves the rest alone.
//! Spec: Core/Services/Rename/RenameExecutor.cs.
//!
//! The awkward part is a batch whose names overlap — rotating "a, b" into "b, a", or shifting a
//! numbered set along by one. Anything whose current name *another* item is aiming at is moved
//! aside to a temporary name first, so the second pass only ever writes into empty space. A
//! failure in that second pass puts the item back under its original name; if even that fails, the
//! staged path is named in the error rather than being left silently.

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::io;

use bb_paths::PathKey;
use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_COPY_ALLOWED, MoveFileExW};

use super::paths::{combine, directory_name, exists, wide};
use super::{CompletedRename, FailedRename, PlannedRename, RenameOutcome, RenamePlan};
use crate::fs::AccessDenied;

pub struct RenameExecutor;

impl RenameExecutor {
    pub fn execute(&self, plan: &RenamePlan) -> RenameOutcome {
        let work = plan.work();
        if work.is_empty() {
            return RenameOutcome::empty();
        }

        let mut completed = Vec::new();
        let mut failed = Vec::new();

        // Which item, if any, is aiming at each name. Targets are unique within a plan, so this is
        // one-to-one, and an item that only changes its own casing maps to itself — which is not
        // something it has to wait for.
        let mut wanted_by: HashMap<String, usize> = HashMap::new();
        for (i, item) in work.iter().enumerate() {
            if let Ok(key) = PathKey::canonicalize(&item.target_path) {
                wanted_by.insert(key, i);
            }
        }

        // Where each item sits now — the staging pass moves some of them out from under
        // themselves — and which of them are standing on a temporary name as a result.
        let mut current: Vec<String> = work.iter().map(|w| w.source_path.clone()).collect();
        let mut is_staged = vec![false; work.len()];
        let mut stumbled = vec![false; work.len()];

        for (i, item) in work.iter().enumerate() {
            let wanter = PathKey::canonicalize(&item.source_path)
                .ok()
                .and_then(|k| wanted_by.get(&k).copied());
            if wanter.is_none_or(|w| w == i) {
                continue;
            }

            match staging_path(item).and_then(|temporary| {
                move_entry(&item.source_path, &temporary, item.is_directory)?;
                Ok(temporary)
            }) {
                Ok(temporary) => {
                    current[i] = temporary;
                    is_staged[i] = true;
                }
                Err(e) => {
                    stumbled[i] = true;
                    failed.push(FailedRename {
                        source_path: item.source_path.clone(),
                        message: format!("'{}' could not be renamed: {e}", item.source_name()),
                        access_denied: AccessDenied::caused(Some(&e)),
                        stranded_path: None,
                    });
                }
            }
        }

        for (i, item) in work.iter().enumerate() {
            if stumbled[i] {
                continue;
            }

            let from = &current[i];
            match Self::place(item, from) {
                Ok(()) => completed.push(CompletedRename {
                    source_path: item.source_path.clone(),
                    final_path: item.target_path.clone(),
                    is_directory: item.is_directory,
                }),
                Err(e) => {
                    // Where the item actually ended up, which is only ever somewhere unexpected
                    // when putting it back failed too. Taken as a value rather than spliced into
                    // the message, so a retry can start from it — see `FailedRename`.
                    let stranded = restore(item, from, is_staged[i]);
                    let note = match &stranded {
                        Some(path) => format!(" It is currently at '{path}'."),
                        None => String::new(),
                    };
                    failed.push(FailedRename {
                        source_path: item.source_path.clone(),
                        message: format!(
                            "'{}' could not be renamed: {e}{note}",
                            item.source_name()
                        ),
                        access_denied: AccessDenied::caused(Some(&e)),
                        stranded_path: stranded,
                    });
                }
            }
        }

        RenameOutcome { completed, failed }
    }

    fn place(item: &PlannedRename, from: &str) -> io::Result<()> {
        // The planner decided this name was free; disk is the authority, and it may have changed
        // while the dialog was open.
        let same =
            PathKey::canonicalize(from).ok() == PathKey::canonicalize(&item.target_path).ok();
        if !same && exists(&item.target_path) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("'{}' already exists.", item.target_name()),
            ));
        }
        move_entry(from, &item.target_path, item.is_directory)
    }

    // --- Undo ---

    /// Puts a completed rename back. A rename is its own inverse, so this is the same execution
    /// with every path swapped — which means undoing a rotation, or a batch that shifted a
    /// numbered set along, goes through exactly the staging the forward direction did. An item
    /// whose old name has since been taken is reported rather than overwritten, and one that has
    /// been renamed again in the meantime simply fails: nothing here writes over anything.
    pub fn undo(&self, outcome: &RenameOutcome) -> RenameOutcome {
        self.execute(&Self::undo_plan(outcome))
    }

    /// The plan that reverses `outcome`.
    pub fn undo_plan(outcome: &RenameOutcome) -> RenamePlan {
        RenamePlan::new(
            outcome
                .completed
                .iter()
                .map(|c| PlannedRename::new(&c.final_path, &c.source_path, c.is_directory))
                .collect(),
            Vec::new(),
        )
    }
}

/// Puts a staged item back under its original name after its rename failed. None when there was
/// nothing to put back or it went back cleanly, and the staged path when it could not — the one
/// case where an item is left somewhere the user didn't put it, so it must never be silent, and
/// the one case where a retry has to start from somewhere other than the path the plan named.
fn restore(item: &PlannedRename, from: &str, was_staged: bool) -> Option<String> {
    if !was_staged {
        return None;
    }
    match move_entry(from, &item.source_path, item.is_directory) {
        Ok(()) => None,
        Err(_) => Some(from.to_string()),
    }
}

/// A name nothing else can be sitting on, in the item's own folder so the move stays a rename
/// rather than becoming a cross-volume copy.
fn staging_path(item: &PlannedRename) -> io::Result<String> {
    let directory = directory_name(&item.source_path).ok_or_else(|| {
        io::Error::other(format!("'{}' has no parent folder.", item.source_name()))
    })?;
    let state = RandomState::new();
    let nonce = format!("{:016x}{:016x}", state.hash_one(0u8), state.hash_one(1u8));
    Ok(combine(&directory, &format!(".bertbrowser-rename-{nonce}")))
}

/// `Directory.Move` / `File.Move(overwrite: false)`: `MoveFileEx` without the replace flag, so an
/// existing target is a failure. (`std::fs::rename` would replace a file in its way.)
fn move_entry(from: &str, to: &str, is_directory: bool) -> io::Result<()> {
    let flags = if is_directory {
        0
    } else {
        MOVEFILE_COPY_ALLOWED
    };
    // SAFETY: two NUL-terminated paths, valid for the call.
    let ok = unsafe { MoveFileExW(wide(from).as_ptr(), wide(to).as_ptr(), flags) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
