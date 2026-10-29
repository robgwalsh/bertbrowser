//! Carries out a `TransferPlan`. Every rule the planner applied is re-checked here against live
//! disk state, because a plan is built when the drag hovers and executed when it drops; the
//! filesystem can change in between. Spec: Core/Services/Transfer/TransferExecutor.cs.
//!
//! The invariants this type exists to hold:
//! - Nothing is ever deleted to make room. `Replace` moves the displaced entry into a hidden
//!   staging folder, so an undo can put it back.
//! - A cross-volume directory move copies, verifies the copy matches the source by file count and
//!   total bytes, and only then deletes the source. A verification failure removes the partial
//!   copy and leaves the source untouched.
//! - A directory tree containing junctions or symlinks is refused across volumes rather than
//!   copied without them and then deleted.
//! - A failure on one item never aborts or rolls back the others; each is independent.
//! - A cancel takes effect *inside* a file, not merely between items, and leaves nothing
//!   half-written where a finished file belongs. Whatever got across before it stays across, and a
//!   cancelled move is still undoable.

use std::hash::{BuildHasher, RandomState};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

use bb_paths::{PathKey, UniquePath, fold_upper, root_len};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, GetFileAttributesW, MoveFileExW, SetFileAttributesW,
};

use super::copier::{FileCopier, FileSystemFileCopier};
use super::models::{
    CompletedTransfer, ConflictResolution, FailedTransfer, PlannedTransfer, Resolutions,
    TransferError, TransferOutcome, TransferPlan, TransferProgress, TransferVerb, join, parent_of,
};
use super::pause_gate::PauseGate;
use super::probe::{FileSystemTransferProbe, TransferProbe};
use super::progress::ProgressCoalescer;
use crate::attributes::{DIRECTORY, HIDDEN, REPARSE_POINT};
use crate::cancel::CancelToken;
use crate::format::thousands;
use crate::fs::{DirectoryRemoval, EntryStamp};

/// Restoring a transfer: how many items went back, and what could not. `reverted` is exactly the
/// items that went back, so a redo acts on those and nothing else; the elevation client, which
/// learns a count over the pipe and never a list, has its half worked out by `ElevatedRetry::merge`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferUndoResult {
    pub restored: usize,
    pub failed: Vec<FailedTransfer>,
    pub reverted: Vec<CompletedTransfer>,
}

/// `ERROR_NOT_SAME_DEVICE`: the only failure of a directory rename that may be answered with
/// copy-then-delete. Every other one (sharing violation, access denied, target exists) must
/// surface as a failure.
const ERROR_NOT_SAME_DEVICE: i32 = 17;

pub struct TransferExecutor {
    probe: Box<dyn TransferProbe>,
    copier: Box<dyn FileCopier>,
}

impl Default for TransferExecutor {
    fn default() -> Self {
        TransferExecutor::new(
            Box::new(FileSystemTransferProbe),
            Box::new(FileSystemFileCopier),
        )
    }
}

/// Files and bytes under a tree, for the cross-volume verification.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Measure {
    files: usize,
    bytes: i64,
}

impl TransferExecutor {
    pub const STAGING_PREFIX: &'static str = ".bertbrowser-replaced-";

    pub fn new(probe: Box<dyn TransferProbe>, copier: Box<dyn FileCopier>) -> TransferExecutor {
        TransferExecutor { probe, copier }
    }

    pub fn with_probe(probe: Box<dyn TransferProbe>) -> TransferExecutor {
        TransferExecutor::new(probe, Box::new(FileSystemFileCopier))
    }

    /// `resolutions`: how to settle conflicts, keyed by `PathKey::canonicalize` of the source path.
    /// Anything unlisted falls back to the non-destructive `KeepBoth`.
    pub fn execute(
        &self,
        plan: &TransferPlan,
        resolutions: Option<&Resolutions>,
        cancel: &CancelToken,
        progress: Option<&mut dyn FnMut(TransferProgress)>,
        pause: Option<&PauseGate>,
    ) -> TransferOutcome {
        let mut completed = Vec::new();
        let mut skipped = Vec::new();
        let mut failed = Vec::new();
        let mut staging_directory: Option<String> = None;
        let mut cancelled = false;

        let mut run = ProgressCoalescer::new(cancel.clone(), progress, plan.transfers.len(), pause);
        for transfer in &plan.transfers {
            if cancel.is_cancelled() {
                cancelled = true;
                break;
            }

            // The label, not the leaf name: after a merge, four hundred items called img.jpg are
            // four hundred headlines the user cannot tell apart.
            run.begin_item(&plan.label_for(transfer));
            let resolution = Self::resolution(resolutions, transfer, plan.verb);
            match self.execute_one(plan, transfer, resolution, &mut staging_directory, &mut run) {
                Ok(Some(result)) => completed.push(result),
                Ok(None) => skipped.push(transfer.source_path.clone()),
                // Stopped part-way through this item. Everything it had started is already undone
                // by the primitive that was interrupted, so there is nothing to report about it.
                Err(TransferError::Cancelled) => {
                    cancelled = true;
                    break;
                }
                Err(e) => failed.push(FailedTransfer::from_error(
                    &transfer.source_path,
                    format!("{}: {}", transfer.name(), e.message()),
                    &e,
                )),
            }
            run.end_item();
        }

        // Even on a cancelled run: it can only remove what this run emptied, which is exactly the
        // right amount.
        let pruned = if plan.verb == TransferVerb::Move {
            Self::prune_emptied(&plan.prune_directories)
        } else {
            Vec::new()
        };

        run.finished();
        let mut outcome = TransferOutcome::new(
            plan.verb,
            &plan.destination_directory,
            completed,
            skipped,
            failed,
            staging_directory.into_iter().collect(),
            cancelled,
        );
        outcome.pruned_directories = pruned;
        outcome
    }

    /// Removes the source folders a merge emptied, and reports which ones went.
    ///
    /// **Why this is not "deleting to make room".** Nothing here can lose data, by four separate
    /// constructions: only paths *the plan listed* are considered, never a discovered walk; only a
    /// folder that is genuinely empty is touched; the removal is non-recursive, which cannot take
    /// anything with it; and a link is skipped outright, so a junction that merely looks empty is
    /// never mistaken for one. It exists because a whole-folder move removes those same
    /// directories implicitly, and expanding the move must not change what the source looks like
    /// afterwards. A Skip, a failure, or a file that arrived since planning all leave a folder
    /// non-empty, and it correctly stays.
    fn prune_emptied(candidates: &[String]) -> Vec<String> {
        let mut pruned = Vec::new();
        for path in candidates {
            let Ok(meta) = std::fs::symlink_metadata(path) else {
                continue;
            };
            if meta.file_attributes() & DIRECTORY == 0 || DirectoryRemoval::is_link(&meta) {
                continue;
            }
            if !is_empty_directory(path) {
                continue;
            }
            // A folder left standing is cosmetic. It must never fail the transfer that filled the
            // destination correctly.
            if std::fs::remove_dir(path).is_ok() {
                pruned.push(path.clone());
            }
        }
        pruned
    }

    /// The completed record, or None when the item was skipped.
    fn execute_one(
        &self,
        plan: &TransferPlan,
        transfer: &PlannedTransfer,
        resolution: ConflictResolution,
        staging_directory: &mut Option<String>,
        run: &mut ProgressCoalescer<'_, '_>,
    ) -> Result<Option<CompletedTransfer>, TransferError> {
        // A copied file's length is asked first: finding it proves the source is there, so the
        // existence check below need not stat the same file a second time (a present file costs far
        // more to stat than a missing one, and a copy of thousands of small files asks twice).
        let known_length = if !transfer.is_directory && plan.verb == TransferVerb::Copy {
            try_length(&transfer.source_path)
        } else {
            None
        };
        self.revalidate(plan, transfer, known_length.is_some())?;

        let mut destination_path = transfer.destination_path.clone();
        let mut displaced_stage_path: Option<String> = None;

        // The plan's conflict flag is a snapshot; ask disk again at the moment of the write.
        if self.exists(&destination_path) {
            match resolution {
                ConflictResolution::Skip => return Ok(None),

                // One arm for both, because they write the same thing: the difference between
                // them is whether the caller can undo the result, which is settled before here.
                ConflictResolution::Replace | ConflictResolution::Overwrite => {
                    let staging = match staging_directory {
                        Some(s) => s.clone(),
                        None => {
                            let s = Self::create_staging_directory(&plan.destination_directory)?;
                            *staging_directory = Some(s.clone());
                            s
                        }
                    };
                    let staged =
                        self.unique_path(&join(&staging, bb_paths::file_name(&destination_path)));
                    // A rename within the same directory tree: instant, and reversible by undo.
                    // Deliberately on an uncancellable run: clearing the name is bookkeeping, and a
                    // cancel landing half-way through it would strand the displaced entry.
                    self.move_entry(
                        &destination_path,
                        &staged,
                        is_directory_on_disk(&destination_path),
                        &mut ProgressCoalescer::silent(),
                    )?;
                    displaced_stage_path = Some(staged);
                }

                ConflictResolution::KeepBoth => {
                    destination_path = self.unique_path(&destination_path)
                }
            }
        }

        let written = if plan.verb == TransferVerb::Move {
            self.move_entry(
                &transfer.source_path,
                &destination_path,
                transfer.is_directory,
                run,
            )
        } else {
            self.copy_entry(
                &transfer.source_path,
                &destination_path,
                transfer.is_directory,
                known_length,
                run,
            )
        };
        if let Err(e) = written {
            // The name was cleared for a transfer that then failed or was cancelled. Nothing
            // succeeded, so the displaced entry goes straight back: no undo record will exist to
            // rescue it later.
            if let Some(staged) = &displaced_stage_path {
                self.restore_displaced(staged, &destination_path);
            }
            return Err(e);
        }

        Ok(Some(CompletedTransfer {
            source_path: transfer.source_path.clone(),
            written: if plan.verb == TransferVerb::Copy {
                EntryStamp::of(&destination_path)
            } else {
                None
            },
            final_path: destination_path,
            is_directory: transfer.is_directory,
            displaced_stage_path,
        }))
    }

    /// Stamps every completion of a copy that has no stamp yet: the ones an elevated pass
    /// reported, which arrive over the pipe as paths only. Run as soon as the outcome is merged,
    /// while what is at each path is still what the copy wrote.
    pub fn stamp_copies(mut outcome: TransferOutcome) -> TransferOutcome {
        if outcome.verb == TransferVerb::Copy {
            for c in &mut outcome.completed {
                if c.written.is_none() {
                    c.written = EntryStamp::of(&c.final_path);
                }
            }
        }
        outcome
    }

    /// Puts a staged entry back under the name it was displaced from. Never fails loudly: it runs
    /// while another failure is being propagated, and must not mask it. If even this fails the
    /// entry is still in staging, which is reported by `staged_items`.
    fn restore_displaced(&self, stage_path: &str, destination_path: &str) {
        if self.exists(stage_path) && !self.exists(destination_path) {
            let _ = self.move_entry(
                stage_path,
                destination_path,
                is_directory_on_disk(stage_path),
                &mut ProgressCoalescer::silent(),
            );
        }
    }

    /// Re-applies the planner's data-safety rules against live disk state.
    fn revalidate(
        &self,
        plan: &TransferPlan,
        transfer: &PlannedTransfer,
        source_found: bool,
    ) -> Result<(), TransferError> {
        if !source_found && !self.exists(&transfer.source_path) {
            return Err(TransferError::not_found(format!(
                "'{}' no longer exists.",
                transfer.name()
            )));
        }
        if !self.probe.directory_exists(&plan.destination_directory) {
            return Err(TransferError::not_found(
                "The destination folder no longer exists.",
            ));
        }
        if !transfer.is_directory {
            return Ok(());
        }

        let source_key = super::models::key_of(&transfer.source_path);
        let dest_key = super::models::key_of(&plan.destination_directory);
        let source_resolved =
            super::models::key_of(&self.probe.resolve_final_path(&transfer.source_path));
        let dest_resolved =
            super::models::key_of(&self.probe.resolve_final_path(&plan.destination_directory));

        if dest_key == source_key
            || dest_resolved == source_resolved
            || PathKey::is_under_key(&dest_key, &source_key)
            || PathKey::is_under_key(&dest_resolved, &source_resolved)
            || PathKey::is_under_key(&dest_resolved, &source_key)
            || PathKey::is_under_key(&dest_key, &source_resolved)
        {
            return Err(TransferError::other(format!(
                "Cannot move '{}' into itself or one of its subfolders.",
                transfer.name()
            )));
        }
        Ok(())
    }

    fn resolution(
        resolutions: Option<&Resolutions>,
        transfer: &PlannedTransfer,
        verb: TransferVerb,
    ) -> ConflictResolution {
        let chosen = resolutions
            .and_then(|r| {
                PathKey::canonicalize(&transfer.source_path)
                    .ok()
                    .and_then(|key| r.get(&key).copied())
            })
            .unwrap_or(ConflictResolution::KeepBoth);

        // Copy is defined as purely additive, so it must never displace an existing entry.
        if chosen == ConflictResolution::Replace && verb == TransferVerb::Copy {
            ConflictResolution::KeepBoth
        } else {
            chosen
        }
    }

    // --- Undo ---

    /// Puts a completed move back: each item returns to the path it came from, then anything it
    /// displaced is restored from staging. Items whose original path has since been taken are
    /// reported rather than overwritten.
    pub fn undo(&self, outcome: &TransferOutcome, cancel: &CancelToken) -> TransferUndoResult {
        if outcome.verb != TransferVerb::Move {
            return TransferUndoResult {
                restored: 0,
                failed: vec![FailedTransfer::new(
                    "",
                    "Only a move can be undone.".to_string(),
                )],
                reverted: Vec::new(),
            };
        }

        let mut failed = Vec::new();
        let mut reverted = Vec::new();

        // Put back the folders a merge emptied, before anything is restored into them. Shallowest
        // first, so a parent exists by the time its child is created. Only the ones this outcome
        // removed: the guard below must keep meaning "the user deleted this", which is the whole
        // reason it refuses rather than recreating whatever it finds missing. A failure here means
        // the items that belonged there will report the folder as gone, which is true.
        for directory in outcome.pruned_directories.iter().rev() {
            let _ = std::fs::create_dir_all(directory);
        }

        // Reverse order so nested transfers unwind in the order they were made.
        for item in outcome.completed.iter().rev() {
            if cancel.is_cancelled() {
                break;
            }
            let name = bb_paths::file_name(&item.source_path);
            if !self.exists(&item.final_path) {
                failed.push(FailedTransfer::new(
                    &item.source_path,
                    format!("{name}: it is no longer where it was moved to."),
                ));
                continue;
            }
            if self.exists(&item.source_path) {
                failed.push(FailedTransfer::new(
                    &item.source_path,
                    format!("{name}: something else now occupies its original location."),
                ));
                continue;
            }
            match parent_of(&item.source_path) {
                Some(parent) if self.probe.directory_exists(&parent) => {}
                _ => {
                    failed.push(FailedTransfer::new(
                        &item.source_path,
                        format!("{name}: its original folder is gone."),
                    ));
                    continue;
                }
            }

            if let Err(e) = self.move_entry(
                &item.final_path,
                &item.source_path,
                item.is_directory,
                &mut ProgressCoalescer::silent(),
            ) {
                failed.push(FailedTransfer::from_error(
                    &item.source_path,
                    format!("{name}: {}", e.message()),
                    &e,
                ));
                continue;
            }
            reverted.push(item.clone());

            // The name it took over is free again: put the displaced entry back. Its own failure,
            // not the item's: the item did go back, and a redo must know that.
            if let Some(staged) = &item.displaced_stage_path
                && self.exists(staged)
                && !self.exists(&item.final_path)
                && let Err(e) = self.move_entry(
                    staged,
                    &item.final_path,
                    is_directory_on_disk(staged),
                    &mut ProgressCoalescer::silent(),
                )
            {
                failed.push(FailedTransfer::from_error(
                    staged,
                    format!(
                        "{}: what it replaced could not be put back ({}). It is at {staged}.",
                        bb_paths::file_name(&item.final_path),
                        e.message()
                    ),
                    &e,
                ));
            }
        }

        self.purge_staging(outcome);
        TransferUndoResult {
            restored: reverted.len(),
            failed,
            reverted,
        }
    }

    /// Undoes a completed *copy*: what it added is removed, and what it displaced is put back.
    /// `undo` refuses a copy; this is the door a caller that kept the outcome comes in by.
    ///
    /// **Removing what a copy added is not a breach of "nothing is ever deleted to make room".**
    /// Every path removed is one `execute_one` created: an `Overwrite` cleared the name into
    /// staging before writing, and every other resolution wrote to a name that was free, so no
    /// destination here ever held anything but this run's own bytes. **Only what still matches
    /// `written` is removed**: the undo history can reach a paste many operations later, so an
    /// edit made to a copied file in between is reported and left alone; the same check is what
    /// makes a replayed record harmless, since the second time round what is at the name is the
    /// original the first call put back. Removal goes through `DirectoryRemoval::remove_tree`,
    /// because a copied tree can hold a junction.
    pub fn undo_copies(
        &self,
        outcome: &TransferOutcome,
        cancel: &CancelToken,
    ) -> TransferUndoResult {
        if outcome.verb != TransferVerb::Copy {
            return TransferUndoResult {
                restored: 0,
                failed: vec![FailedTransfer::new(
                    "",
                    "Only a copy can be undone here.".to_string(),
                )],
                reverted: Vec::new(),
            };
        }

        let mut failed = Vec::new();
        let mut reverted = Vec::new();

        // Reverse order, so a folder is not removed out from under the entries copied into it.
        for item in outcome.completed.iter().rev() {
            if cancel.is_cancelled() {
                break;
            }
            let name = bb_paths::file_name(&item.final_path);
            match self.undo_one_copy(item) {
                Ok(true) => reverted.push(item.clone()),
                Ok(false) => failed.push(FailedTransfer::new(
                    &item.final_path,
                    format!("{name}: it was already gone."),
                )),
                Err(UndoCopyRefusal::Changed) => failed.push(FailedTransfer::new(
                    &item.final_path,
                    format!("{name}: it has changed since it was pasted, so it was left in place."),
                )),
                Err(UndoCopyRefusal::Failed(e)) => failed.push(FailedTransfer::from_error(
                    &item.final_path,
                    format!("{name}: {}", e.message()),
                    &e,
                )),
            }
        }

        self.purge_staging(outcome);
        TransferUndoResult {
            restored: reverted.len(),
            failed,
            reverted,
        }
    }

    /// True when something was removed or put back; false when the item was already gone.
    fn undo_one_copy(&self, item: &CompletedTransfer) -> Result<bool, UndoCopyRefusal> {
        let mut removed = false;
        if self.exists(&item.final_path) {
            if let Some(written) = &item.written
                && !written.matches(&item.final_path)
            {
                return Err(UndoCopyRefusal::Changed);
            }
            if item.is_directory {
                DirectoryRemoval::remove_tree(&item.final_path)?;
            } else {
                std::fs::remove_file(&item.final_path)?;
            }
            removed = true;
        }

        // The name is free again: put back whatever the copy took it from.
        let mut put_back = false;
        if let Some(staged) = &item.displaced_stage_path
            && self.exists(staged)
            && !self.exists(&item.final_path)
        {
            self.move_entry(
                staged,
                &item.final_path,
                is_directory_on_disk(staged),
                &mut ProgressCoalescer::silent(),
            )?;
            put_back = true;
        }

        Ok(removed || put_back)
    }

    /// Deletes an outcome's staging folder once it can no longer be undone. Refuses any path that
    /// is not a staging folder this type created, and leaves a non-empty one alone: losing the
    /// ability to undo must never turn into losing the data.
    pub fn purge_staging(&self, outcome: &TransferOutcome) {
        for staging in &outcome.staging_directories {
            if !Self::is_staging_directory(staging) || !is_directory_on_disk(staging) {
                continue;
            }
            if !is_empty_directory(staging) {
                continue; // still holds displaced data
            }
            // A leftover hidden folder is harmless; never let cleanup break the operation.
            let _ = std::fs::remove_dir(staging);
        }
    }

    /// Discards an outcome's staging folder *and anything still in it*, committing the
    /// replacements it represents. Call this only when the outcome can no longer be undone; up to
    /// that point the displaced entries are the only copy left. Guarded so it can never delete
    /// anything other than a staging folder `create_staging_directory` made, and through
    /// `DirectoryRemoval::remove_tree`, because what a Replace displaced is the user's own folder
    /// and can contain a junction.
    pub fn commit_staging(outcome: &TransferOutcome) {
        for staging in &outcome.staging_directories {
            if !Self::is_staging_directory(staging) || !is_directory_on_disk(staging) {
                continue;
            }
            let _ = DirectoryRemoval::remove_tree(staging);
        }
    }

    /// Everything still parked in staging for an outcome that is no longer undoable. Surfaced so
    /// the user can be told where displaced items went instead of losing them.
    pub fn staged_items(outcome: &TransferOutcome) -> Vec<String> {
        let mut items = Vec::new();
        for staging in &outcome.staging_directories {
            if !Self::is_staging_directory(staging) || !is_directory_on_disk(staging) {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(staging) {
                items.extend(
                    entries
                        .flatten()
                        .map(|e| e.path().to_string_lossy().into_owned()),
                );
            }
        }
        items
    }

    /// Whether a path is a staging folder this type made: the guard on every recursive delete in
    /// this file. A path only qualifies if it is named the way this type names them, so a mangled
    /// or hand-built outcome cannot direct the erase at a real folder.
    fn is_staging_directory(path: &str) -> bool {
        bb_paths::file_name(path).starts_with(Self::STAGING_PREFIX)
    }

    // --- Filesystem primitives ---

    fn move_entry(
        &self,
        source: &str,
        destination: &str,
        is_directory: bool,
        run: &mut ProgressCoalescer<'_, '_>,
    ) -> Result<(), TransferError> {
        if !is_directory {
            // Spans volumes by itself, so unlike a directory rename there is no not-same-device
            // arm to write here. Within a volume it is a rename and reports no bytes at all, which
            // is why a same-volume move shows no progress rather than a bar that never moves.
            run.begin_file(0);
            let token = run.token().clone();
            self.copier.move_file(
                source,
                destination,
                Some(&mut |t, total| run.file_progress(t, total)),
                &token,
            )?;
            run.end_file();
            return Ok(());
        }

        if Self::same_volume(source, destination) {
            return Ok(rename_directory(source, destination)?);
        }

        match rename_directory(source, destination) {
            Ok(()) => Ok(()),
            // Mount points can make two paths share a root string but not a volume.
            Err(e) if e.raw_os_error() == Some(ERROR_NOT_SAME_DEVICE) => {
                self.cross_volume_move_directory(source, destination, run)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn copy_entry(
        &self,
        source: &str,
        destination: &str,
        is_directory: bool,
        known_length: Option<i64>,
        run: &mut ProgressCoalescer<'_, '_>,
    ) -> Result<(), TransferError> {
        if !is_directory {
            return self.copy_file(source, destination, known_length, run);
        }
        match self.copy_directory(source, destination, run) {
            // A copy is defined as purely additive, so a cancelled one must add nothing: half a
            // tree at the destination reads as a finished copy to everything that looks at it.
            Err(TransferError::Cancelled) => {
                try_delete_partial_copy(destination);
                Err(TransferError::Cancelled)
            }
            other => other,
        }
    }

    /// One file's bytes, with the run's counters bracketing them. `known_length` keeps the
    /// running total exact even when the copy reports coarsely.
    fn copy_file(
        &self,
        source: &str,
        destination: &str,
        known_length: Option<i64>,
        run: &mut ProgressCoalescer<'_, '_>,
    ) -> Result<(), TransferError> {
        run.begin_file(known_length.unwrap_or(0));
        let token = run.token().clone();
        self.copier.copy(
            source,
            destination,
            Some(&mut |t, total| run.file_progress(t, total)),
            &token,
        )?;
        run.end_file();
        Ok(())
    }

    /// Copy, verify, then delete. The source is only removed once the destination is confirmed to
    /// hold the same number of files and the same total bytes.
    fn cross_volume_move_directory(
        &self,
        source: &str,
        destination: &str,
        run: &mut ProgressCoalescer<'_, '_>,
    ) -> Result<(), TransferError> {
        if let Some(refusal) = Self::cross_volume_refusal(source) {
            return Err(TransferError::other(refusal));
        }

        let expected = measure(source)?;

        // Covers a cancel as well as a failure: either way the source is still there and the
        // half-copy at the destination is not something anyone should be left holding.
        if let Err(e) = self.copy_directory(source, destination, run) {
            try_delete_partial_copy(destination);
            return Err(e);
        }

        let actual = measure(destination)?;
        if actual != expected {
            try_delete_partial_copy(destination);
            return Err(TransferError::other(format!(
                "Verification failed while moving '{}' to another drive (expected {} files / {} bytes, found {} / {}). Nothing was removed.",
                bb_paths::file_name(source),
                thousands(expected.files as i64),
                thousands(expected.bytes),
                thousands(actual.files as i64),
                thousands(actual.bytes)
            )));
        }

        Ok(DirectoryRemoval::remove_tree(source)?)
    }

    /// Why `root` cannot be moved to another volume, or None when nothing stands in the way.
    /// Copy-then-delete cannot reproduce a junction or symbolic link, and the delete at the end of
    /// the move would then destroy it. **The root is asked about separately, and first**: a merged
    /// transfer emits a linked child as an item of its own, and walking through it would copy the
    /// target's contents and then delete the link. Public and pure so both refusals can be tested
    /// without a second volume to hand.
    pub fn cross_volume_refusal(root: &str) -> Option<String> {
        let name = bb_paths::file_name(root);
        if std::fs::symlink_metadata(root).is_ok_and(|m| DirectoryRemoval::is_link(&m)) {
            return Some(format!(
                "'{name}' is a junction or symbolic link and cannot be moved to another drive. Move it within the same drive, or recreate it at the destination."
            ));
        }
        if let Some(link) = find_reparse_point(root) {
            return Some(format!(
                "'{name}' contains a junction or symbolic link ({}) and cannot be moved to another drive. Move it within the same drive, or copy it and remove the original.",
                bb_paths::file_name(&link)
            ));
        }
        None
    }

    fn copy_directory(
        &self,
        source: &str,
        destination: &str,
        run: &mut ProgressCoalescer<'_, '_>,
    ) -> Result<(), TransferError> {
        run.token().check().map_err(|_| TransferError::Cancelled)?;
        std::fs::create_dir_all(destination)?;
        for entry in std::fs::read_dir(source)? {
            run.token().check().map_err(|_| TransferError::Cancelled)?;
            let entry = entry?;
            let meta = entry.metadata()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let attributes = meta.file_attributes();

            if attributes & REPARSE_POINT != 0 {
                return Err(TransferError::other(format!(
                    "'{name}' is a junction or symbolic link and cannot be copied."
                )));
            }

            let child_source = entry.path().to_string_lossy().into_owned();
            let child_destination = join(destination, &name);
            if attributes & DIRECTORY != 0 {
                self.copy_directory(&child_source, &child_destination, run)?;
            } else {
                // The length comes free from the enumeration that found the file, so the running
                // total is exact without a stat per entry.
                self.copy_file(
                    &child_source,
                    &child_destination,
                    Some(meta.file_size() as i64),
                    run,
                )?;
            }
        }
        // Cosmetic only: the copy carries the source folder's last-write time.
        copy_directory_write_time(source, destination);
        Ok(())
    }

    /// Conservative: false whenever the volumes cannot be established, which routes the move
    /// through the guarded fallback instead of assuming a plain rename will do. Shared with
    /// `TransferEstimator`, which has to answer the same question to know whether an item moves
    /// any bytes at all; the two must not drift apart.
    pub fn same_volume(a: &str, b: &str) -> bool {
        let (Ok(a), Ok(b)) = (PathKey::normalize_display(a), PathKey::normalize_display(b)) else {
            return false;
        };
        let root_a = &a[..root_len(&a)];
        let root_b = &b[..root_len(&b)];
        !root_a.is_empty() && !root_b.is_empty() && fold_upper(root_a) == fold_upper(root_b)
    }

    fn create_staging_directory(destination_directory: &str) -> Result<String, TransferError> {
        let nonce = RandomState::new().hash_one(0u8) as u32;
        let path = join(
            destination_directory,
            &format!("{}{nonce:08x}", Self::STAGING_PREFIX),
        );
        std::fs::create_dir_all(&path)?;
        // Visible staging is ugly but not wrong.
        set_hidden(&path);
        Ok(path)
    }

    /// "name (2)"-style free path, through this executor's probe.
    fn unique_path(&self, path: &str) -> String {
        UniquePath::for_path(
            path,
            self.probe.directory_exists(path),
            |p| self.probe.directory_exists(p),
            |p| self.probe.file_exists(p),
        )
    }

    fn exists(&self, path: &str) -> bool {
        self.probe.exists(path)
    }
}

enum UndoCopyRefusal {
    Changed,
    Failed(TransferError),
}

impl From<io::Error> for UndoCopyRefusal {
    fn from(e: io::Error) -> Self {
        UndoCopyRefusal::Failed(e.into())
    }
}

impl From<TransferError> for UndoCopyRefusal {
    fn from(e: TransferError) -> Self {
        UndoCopyRefusal::Failed(e)
    }
}

/// `Directory.Move`: `MoveFileExW` with no flags, which fails when the destination exists and
/// reports `ERROR_NOT_SAME_DEVICE` across volumes rather than copying.
fn rename_directory(source: &str, destination: &str) -> io::Result<()> {
    let (src, dst) = (wide(source), wide(destination));
    // SAFETY: both NUL-terminated and valid for the call.
    let ok = unsafe { MoveFileExW(src.as_ptr(), dst.as_ptr(), 0) };
    if ok != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn is_directory_on_disk(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & DIRECTORY != 0)
}

fn is_empty_directory(path: &str) -> bool {
    std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

fn try_length(path: &str) -> Option<i64> {
    super::probe::file_length(path)
}

/// Files and bytes under `root`, however deep. The refusal has already ruled out links, so there
/// is nothing here to decline to descend into.
fn measure(root: &str) -> io::Result<Measure> {
    let mut total = Measure { files: 0, bytes: 0 };
    let mut pending = vec![root.to_string()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.file_attributes() & DIRECTORY != 0 {
                pending.push(entry.path().to_string_lossy().into_owned());
            } else {
                total.files += 1;
                total.bytes += meta.file_size() as i64;
            }
        }
    }
    Ok(total)
}

/// The first junction/symlink anywhere *inside* the tree, or None. Says nothing about the root.
fn find_reparse_point(root: &str) -> Option<String> {
    let mut pending = vec![root.to_string()];
    while let Some(dir) = pending.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let path = entry.path().to_string_lossy().into_owned();
            if DirectoryRemoval::is_link(&meta) {
                return Some(path);
            }
            if meta.file_attributes() & DIRECTORY != 0 {
                pending.push(path);
            }
        }
    }
    None
}

/// Best effort, through `DirectoryRemoval::remove_tree` for the reason a recursive delete is
/// banned everywhere else in this app: given a junction it would erase the rest of the tree and
/// then fail, swallowed here as harmless cleanup.
fn try_delete_partial_copy(destination: &str) {
    if is_directory_on_disk(destination) {
        let _ = DirectoryRemoval::remove_tree(destination);
    }
}

fn copy_directory_write_time(source: &str, destination: &str) {
    let Ok(modified) = std::fs::symlink_metadata(source).and_then(|m| m.modified()) else {
        return;
    };
    // A directory handle needs backup semantics to open at all.
    let Ok(dir) = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(destination)
    else {
        return;
    };
    let _ = dir.set_modified(modified);
}

fn set_hidden(path: &str) {
    let wide_path = wide(path);
    // SAFETY: NUL-terminated and valid for both calls.
    unsafe {
        let attributes = GetFileAttributesW(wide_path.as_ptr());
        if attributes != u32::MAX {
            SetFileAttributesW(wide_path.as_ptr(), attributes | HIDDEN);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
