//! Carries out a [`DeletePlan`]. Spec: Core/Services/Delete/DeleteExecutor.cs.
//!
//! An ordinary delete does not erase anything. Each item is *moved* into a hidden holding folder
//! at the root of its own volume — a rename, so it costs the same for one file as for a hundred
//! gigabytes — and stays there, intact, until the delete can no longer be undone. That is what
//! makes Ctrl+Z able to put a directory tree back byte for byte instead of best-effort.
//! [`DeleteExecutor::commit_staging`] is what finally erases the data, and the caller owes it
//! exactly one call per outcome once the undo record is retired.
//!
//! A permanent delete (Shift+Delete) skips all of that and erases in place. It is the only path
//! here that destroys data, so it is the only one that cannot be undone, and it is never taken
//! unless the plan says so.
//!
//! Every rule the planner applied is re-checked against live disk state, because a plan is built
//! while the confirmation is open and carried out after it is answered. A failure on one item
//! never affects the others.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;
use std::time::Duration;

use bb_paths::{PathKey, UniquePath, file_name, parent_key, root_len, split_last};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_COPY_ALLOWED, MoveFileExW, SetFileAttributesW,
};

use super::models::{
    DeleteDisposition, DeleteOutcome, DeletePlan, DeleteProgress, DeleteUndoResult, DeletedItem,
    FailedDelete, PlannedDelete,
};
use super::planner::parent_of;
use super::probe::{DeleteProbe, EntryAttributes, NoRecycleProbe, RecycleProbe};
use super::protected::ProtectedLocations;
use super::recycle_bin::RecycleBin;
use crate::FileTime;
use crate::attributes::{DIRECTORY, HIDDEN, READONLY, REPARSE_POINT};
use crate::cancel::CancelToken;
use crate::fs::{AccessDenied, DirectoryRemoval, FileSystemService, LocalFileSystem};

/// `ERROR_NOT_SAME_DEVICE`. A volume-root holding folder is on the same volume as the item nearly
/// always — but a mount point part-way down the path makes "the root of C:" and "where this file
/// physically lives" two different volumes, and then the move has to be staged locally instead.
const ERROR_NOT_SAME_DEVICE: i32 = 0x11;

pub struct DeleteExecutor<'a> {
    probe: &'a dyn DeleteProbe,
    protected_keys: Cow<'static, HashSet<String>>,
    staging_root: Option<String>,
    recycle_bin: Option<&'a dyn RecycleBin>,
    recycle_probe: &'a dyn RecycleProbe,
}

/// The mutable state of one run: holding folders made on first use, so a plan that fails
/// immediately leaves nothing behind.
#[derive(Default)]
struct Run {
    deleted: Vec<DeletedItem>,
    staging: Vec<String>,
    /// One holding folder per volume.
    volume_batches: HashMap<String, String>,
    /// One per parent folder, for the mount-point fallback.
    local_batches: HashMap<String, String>,
    /// Items bound for the Recycle Bin are gathered rather than done one by one: the shell wants
    /// one operation with everything added to it, which is also the only way to get a single
    /// progress sink out of it.
    to_recycle: Vec<PlannedDelete>,
    done: usize,
}

impl<'a> DeleteExecutor<'a> {
    /// The per-volume holding folder, at the volume root so it is one place per disk and can be
    /// swept up later even after a crash.
    pub const TRASH_FOLDER_NAME: &'static str = ".bertbrowser-trash";

    /// One folder per delete inside [`Self::TRASH_FOLDER_NAME`].
    pub const BATCH_PREFIX: &'static str = "delete-";

    /// The fallback holding folder, made beside the item itself, which is guaranteed to be on the
    /// item's own volume whatever mount points the path crosses.
    pub const LOCAL_STAGING_PREFIX: &'static str = ".bertbrowser-deleted-";

    /// An executor over `probe` that refuses [`ProtectedLocations::default_keys`], holds items at
    /// the root of their own volume, has no Recycle Bin and assumes no volume will take a recycled
    /// item.
    pub fn new(probe: &'a dyn DeleteProbe) -> DeleteExecutor<'a> {
        DeleteExecutor {
            probe,
            protected_keys: Cow::Borrowed(ProtectedLocations::default_keys()),
            staging_root: None,
            recycle_bin: None,
            recycle_probe: NoRecycleProbe::INSTANCE,
        }
    }

    /// Replaces the locations refused outright; an empty list protects nothing.
    pub fn protecting(mut self, paths: &[&str]) -> DeleteExecutor<'a> {
        self.protected_keys = Cow::Owned(ProtectedLocations::keys_of(paths.iter().copied()));
        self
    }

    /// Where to put the holding folder, instead of the root of each item's own volume. Only for
    /// tests, which have no business creating folders at the root of the machine's disks — and,
    /// being under the temp directory, still land on the same volume as the files they make,
    /// which is what the whole design rests on.
    pub fn staging_root(mut self, root: &str) -> DeleteExecutor<'a> {
        self.staging_root = Some(root.to_string());
        self
    }

    /// The Windows Recycle Bin. Without one, every item the plan wanted recycled is held in the
    /// staging folder instead — never erased.
    pub fn recycle_bin(mut self, bin: &'a dyn RecycleBin) -> DeleteExecutor<'a> {
        self.recycle_bin = Some(bin);
        self
    }

    /// Re-asks, against live state, whether an item's volume will take a recycled item.
    pub fn recycle_probe(mut self, probe: &'a dyn RecycleProbe) -> DeleteExecutor<'a> {
        self.recycle_probe = probe;
        self
    }

    pub fn execute(
        &self,
        plan: &DeletePlan,
        cancel: &CancelToken,
        progress: Option<&dyn Fn(DeleteProgress)>,
    ) -> DeleteOutcome {
        let mut run = Run::default();
        let mut failed = Vec::new();
        let total = plan.deletions.len();

        for item in &plan.deletions {
            if cancel.is_cancelled() {
                break;
            }
            if let Err(e) = self.execute_one(item, total, &mut run, progress) {
                failed.push(FailedDelete {
                    source_path: item.source_path.clone(),
                    message: format!("{}: {}", item.name(), e),
                    access_denied: AccessDenied::caused(Some(&e)),
                });
            }
        }

        if let Some(bin) = self.recycle_bin
            && !run.to_recycle.is_empty()
            && !cancel.is_cancelled()
        {
            // The batch's own 0..n progress shifted into its place in the whole plan, so the bar
            // does not restart when the Recycle Bin takes over.
            let done = run.done;
            let offset = progress.map(|report| {
                move |p: DeleteProgress| {
                    report(DeleteProgress {
                        done: done + p.done,
                        total,
                        current_name: p.current_name,
                    })
                }
            });
            let result = bin.recycle(
                &run.to_recycle,
                cancel,
                offset.as_ref().map(|f| f as &dyn Fn(DeleteProgress)),
            );
            for item in result.recycled {
                run.deleted.push(DeletedItem {
                    source_path: item.source_path,
                    is_directory: item.is_directory,
                    staged_path: None,
                    recycled_path: item.recycled_path,
                });
            }
            failed.extend(result.failed);
            run.done += run.to_recycle.len();
        }

        if let Some(report) = progress {
            report(DeleteProgress {
                done: run.done,
                total,
                current_name: String::new(),
            });
        }
        DeleteOutcome {
            permanent: plan.permanent(),
            deleted: run.deleted,
            failed,
            staging_directories: run.staging,
        }
    }

    fn execute_one(
        &self,
        item: &PlannedDelete,
        total: usize,
        run: &mut Run,
        progress: Option<&dyn Fn(DeleteProgress)>,
    ) -> io::Result<()> {
        self.revalidate(item)?;

        let disposition = self.live_disposition(item);
        if disposition == DeleteDisposition::Recycle {
            run.to_recycle.push(item.clone()); // counted when the batch runs
            return Ok(());
        }

        if let Some(report) = progress {
            report(DeleteProgress {
                done: run.done,
                total,
                current_name: item.name().to_string(),
            });
        }
        run.done += 1;

        let staged_path = match disposition {
            DeleteDisposition::Erase => {
                Self::erase(&item.source_path, item.is_directory)?;
                None
            }
            _ => Some(self.stage(item, run)?),
        };
        run.deleted.push(DeletedItem {
            source_path: item.source_path.clone(),
            is_directory: item.is_directory,
            staged_path,
            recycled_path: None,
        });
        Ok(())
    }

    /// The planner's routing, re-asked against live state — a plan is built while the
    /// confirmation is open. An item can lose its Recycle Bin between the two (a share goes away,
    /// the bin is turned off), and with no bin to hand it to the answer is the holding folder,
    /// never an erase.
    fn live_disposition(&self, item: &PlannedDelete) -> DeleteDisposition {
        if item.disposition != DeleteDisposition::Recycle {
            return item.disposition;
        }
        if self.recycle_bin.is_some() && self.recycle_probe.can_recycle(&item.source_path) {
            DeleteDisposition::Recycle
        } else {
            DeleteDisposition::Stage
        }
    }

    /// Re-applies the planner's rules against live disk state.
    fn revalidate(&self, item: &PlannedDelete) -> io::Result<()> {
        if !self.exists(&item.source_path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("'{}' no longer exists.", item.name()),
            ));
        }

        if parent_of(&item.source_path).is_none() {
            return Err(io::Error::other(format!(
                "'{}' is a drive root and cannot be deleted.",
                item.source_path
            )));
        }

        let key = PathKey::canonicalize(&item.source_path)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        if self.protected_keys.contains(&key) {
            return Err(io::Error::other(format!(
                "'{}' is a system location and will not be deleted.",
                item.source_path
            )));
        }
        Ok(())
    }

    // --- Staged delete ---

    /// Moves one item into a holding folder and returns where it now sits.
    fn stage(&self, item: &PlannedDelete, run: &mut Run) -> io::Result<String> {
        let parent = parent_of(&item.source_path)
            .ok_or_else(|| io::Error::other(format!("'{}' has no parent folder.", item.name())))?;

        if let Some(batch) = self.volume_batch(item, run)
            && let Some(staged) = self.try_move_into(item, &batch)?
        {
            return Ok(staged);
        }

        // Either the volume root refused the folder, or the item turned out to live on a
        // different volume than its path root suggests. A folder beside the item cannot be either.
        let local_key = PathKey::canonicalize(&parent)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        let local = match run.local_batches.get(&local_key) {
            Some(local) => local.clone(),
            None => {
                let local = create_staging_directory(&join(
                    &parent,
                    &format!("{}{}", Self::LOCAL_STAGING_PREFIX, short_id()),
                ))?;
                run.local_batches.insert(local_key, local.clone());
                run.staging.push(local.clone());
                local
            }
        };
        self.move_into(item, &local)
    }

    /// The holding folder at the root of the item's volume, made on first use. None when the root
    /// will not take one — a read-only or otherwise unwritable root, or an item that *is* the
    /// holding folder.
    fn volume_batch(&self, item: &PlannedDelete, run: &mut Run) -> Option<String> {
        let root = match &self.staging_root {
            Some(root) => root.clone(),
            None => {
                let full = PathKey::normalize_display(&item.source_path).ok()?;
                full[..root_len(&full)].to_string()
            }
        };
        if root.is_empty() {
            return None;
        }

        let root_key = PathKey::canonicalize(&root).ok()?;
        let batch = match run.volume_batches.get(&root_key) {
            Some(batch) => batch.clone(),
            None => {
                let trash = create_staging_directory(&join(&root, Self::TRASH_FOLDER_NAME)).ok()?;
                let batch = create_staging_directory(&join(
                    &trash,
                    &format!("{}{}", Self::BATCH_PREFIX, short_id()),
                ))
                .ok()?;
                run.volume_batches.insert(root_key, batch.clone());
                run.staging.push(batch.clone());
                batch
            }
        };

        // Deleting the holding folder itself, or anything above it, would have it move into itself.
        let source_key = PathKey::canonicalize(&item.source_path).ok()?;
        let batch_key = PathKey::canonicalize(&batch).ok()?;
        if batch_key == source_key || PathKey::is_under_key(&batch_key, &source_key) {
            None
        } else {
            Some(batch)
        }
    }

    /// Moves the item into `directory`. None — with nothing moved — when the item turns out to be
    /// on another volume; every other failure is a real one.
    fn try_move_into(&self, item: &PlannedDelete, directory: &str) -> io::Result<Option<String>> {
        match self.move_into(item, directory) {
            Ok(staged) => Ok(Some(staged)),
            Err(e) if e.raw_os_error() == Some(ERROR_NOT_SAME_DEVICE) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn move_into(&self, item: &PlannedDelete, directory: &str) -> io::Result<String> {
        // Two selected items can share a name — a flattened search result spans folders — so the
        // holding folder numbers the second one rather than colliding with the first.
        let staged = self.unique_path(&join(directory, item.name()));
        move_entry(&item.source_path, &staged, item.is_directory)?;
        Ok(staged)
    }

    // --- Permanent delete ---

    /// Erases in place. A read-only item is retried after clearing the attribute — the user asked
    /// for this one by name, and a tree that stops half-deleted on the first read-only file inside
    /// it is worse than one that goes.
    fn erase(path: &str, is_directory: bool) -> io::Result<()> {
        match Self::remove(path, is_directory) {
            Err(e) if AccessDenied::caused(Some(&e)) => {
                Self::clear_read_only(path, is_directory);
                Self::remove(path, is_directory)
            }
            result => result,
        }
    }

    /// [`DirectoryRemoval::remove_tree`], never a recursive delete: that erases the rest of a tree
    /// containing a junction and then fails naming the link, which on this path means a permanent
    /// delete destroying the contents and reporting that it failed.
    fn remove(path: &str, is_directory: bool) -> io::Result<()> {
        if is_directory {
            DirectoryRemoval::remove_tree(path)
        } else {
            std::fs::remove_file(path)
        }
    }

    /// Best-effort: anything still read-only afterwards fails the retry, which is reported.
    ///
    /// **The walk descends by hand and declines to push a link, and that is the whole point of
    /// it.** A walk that follows directory reparse points put this loop into a directory somewhere
    /// else entirely — and a test for the reparse bit on each entry does not help once the
    /// enumeration has already gone through the link. Clearing read-only outside the tree being
    /// deleted is a small harm (`remove` does not follow junctions, so nothing out there is
    /// erased) but it is a harm to files the user never selected. The same shape
    /// `DeleteSurveyor` walks with, for the same reason: a junction is the one entry deleting it
    /// removes.
    fn clear_read_only(path: &str, is_directory: bool) {
        if !is_directory {
            clear_read_only_attribute(path);
            return;
        }

        let Some(root) = EntryAttributes::of(path) else {
            return;
        };
        if root.attributes & REPARSE_POINT != 0 {
            return;
        }

        let mut pending = vec![PathBuf::from(path)];
        while let Some(current) = pending.pop() {
            // A folder that cannot be listed ends the sweep; the delete retry reports what is left.
            let Ok(entries) = std::fs::read_dir(&current) else {
                return;
            };
            for entry in entries {
                let Ok(entry) = entry else {
                    return;
                };
                // One stubborn entry must not stop the rest; the delete retry reports it.
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                let attributes = meta.file_attributes();
                let is_link = attributes & REPARSE_POINT != 0;
                if attributes & DIRECTORY != 0 && !is_link {
                    pending.push(entry.path());
                }
                if is_link {
                    continue;
                }
                if attributes & READONLY != 0 {
                    clear_read_only_attribute(&entry.path().to_string_lossy());
                }
            }
        }

        if root.attributes & READONLY != 0 {
            clear_read_only_attribute(path);
        }
    }

    // --- Undo ---

    /// Puts a staged delete back: each item returns from the holding folder to exactly where it
    /// was. An item whose original path has since been taken is reported rather than overwritten —
    /// nothing here writes over anything.
    pub fn undo(&self, outcome: &DeleteOutcome, cancel: &CancelToken) -> DeleteUndoResult {
        if outcome.permanent {
            return DeleteUndoResult {
                restored: 0,
                failed: vec![FailedDelete::new(
                    "",
                    "A permanent delete cannot be undone.".to_string(),
                )],
                reverted: Vec::new(),
            };
        }

        let mut failed = Vec::new();
        let mut reverted = Vec::new();

        // Reverse order, so a batch unwinds in the order it was made.
        for item in outcome.deleted.iter().rev() {
            if cancel.is_cancelled() {
                break;
            }
            match self.undo_one(item) {
                Ok(()) => reverted.push(item.clone()),
                Err(failure) => failed.push(failure),
            }
        }

        Self::purge_empty_staging(outcome);
        DeleteUndoResult {
            restored: reverted.len(),
            failed,
            reverted,
        }
    }

    fn undo_one(&self, item: &DeletedItem) -> Result<(), FailedDelete> {
        let refuse = |message: String| FailedDelete::new(&item.source_path, message);

        // Nothing here writes over anything, whichever way the item was held.
        if self.exists(&item.source_path) {
            return Err(refuse(format!(
                "{}: something else now occupies its original location.",
                item.name()
            )));
        }

        if let Some(recycled) = &item.recycled_path {
            let restored = self.recycle_bin.is_some_and(|bin| bin.restore(item));
            if !restored {
                return Err(refuse(format!(
                    "{}: the Recycle Bin no longer holds it ({recycled}).",
                    item.name()
                )));
            }
            return Ok(());
        }

        let staged = match &item.staged_path {
            Some(staged) if self.exists(staged) => staged,
            _ => {
                return Err(refuse(format!(
                    "{}: the deleted copy is no longer being held.",
                    item.name()
                )));
            }
        };

        let parent = parent_key(&item.source_path);
        if !parent.is_some_and(|p| self.probe.directory_exists(&p)) {
            return Err(refuse(format!(
                "{}: its original folder is gone.",
                item.name()
            )));
        }

        move_entry(staged, &item.source_path, item.is_directory).map_err(|e| FailedDelete {
            source_path: item.source_path.clone(),
            message: format!("{}: {}", item.name(), e),
            access_denied: AccessDenied::caused(Some(&e)),
        })
    }

    // --- Staging lifecycle ---

    /// Erases everything an outcome is still holding, committing the delete. Call this only once
    /// the outcome can no longer be undone: up to that point the held copies are the only ones
    /// there are. Guarded so it can only ever remove a folder this type made. A holding folder
    /// that survives is only wasted space; cleanup never fails loudly.
    pub fn commit_staging(outcome: &DeleteOutcome) {
        for directory in &outcome.staging_directories {
            if !Self::is_staging_directory(directory) {
                continue;
            }
            // `remove_tree`, not a recursive delete: a held item can be a tree with a junction in
            // it, and that call would erase most of the folder and then fail — ignored here, so
            // the debris would simply be left behind without a word.
            if is_dir(directory) && DirectoryRemoval::remove_tree(directory).is_err() {
                continue;
            }
            Self::remove_empty_trash_root(directory);
        }
    }

    /// Removes an outcome's holding folders only if they are empty — used after an undo, where
    /// anything left behind is an item that could not be put back and must not be erased along
    /// with the folder.
    fn purge_empty_staging(outcome: &DeleteOutcome) {
        for directory in &outcome.staging_directories {
            if !Self::is_staging_directory(directory) {
                continue;
            }
            if !is_dir(directory) || !is_empty_dir(directory) {
                continue;
            }
            if std::fs::remove_dir(directory).is_ok() {
                Self::remove_empty_trash_root(directory);
            }
        }
    }

    /// Everything an outcome is still holding — surfaced so a failed undo can say where the data
    /// went instead of leaving it to be found by accident.
    pub fn staged_items(outcome: &DeleteOutcome) -> Vec<String> {
        let mut items = Vec::new();
        for directory in &outcome.staging_directories {
            let Ok(entries) = std::fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                items.push(entry.path().to_string_lossy().into_owned());
            }
        }
        items
    }

    /// Erases holding folders left behind by a previous session — a crash, or a kill — from every
    /// ready volume. Only batches older than `older_than` go, so a second copy of the app running
    /// right now cannot have its pending undo swept out from under it. Best-effort and silent: a
    /// drive that refuses to be read is simply skipped.
    pub fn purge_abandoned_staging(older_than: Duration) {
        let roots: Vec<String> = LocalFileSystem
            .drives()
            .into_iter()
            .map(|d| d.name)
            .collect();
        Self::purge_abandoned_staging_under(older_than, &roots);
    }

    /// [`Self::purge_abandoned_staging`] over `roots` instead of every ready volume.
    pub fn purge_abandoned_staging_under(older_than: Duration, roots: &[String]) {
        let cutoff = FileTime::now().add_ticks(-((older_than.as_nanos() / 100) as i64));

        for root in roots {
            let trash = join(root, Self::TRASH_FOLDER_NAME);
            if !is_dir(&trash) {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&trash) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let is_batch = name
                    .to_string_lossy()
                    .get(..Self::BATCH_PREFIX.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(Self::BATCH_PREFIX));
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if !is_batch || meta.file_attributes() & DIRECTORY == 0 {
                    continue;
                }
                if FileTime(meta.creation_time() as i64) > cutoff {
                    continue;
                }
                let _ = DirectoryRemoval::remove_tree(&entry.path().to_string_lossy());
            }
            if is_empty_dir(&trash) {
                let _ = std::fs::remove_dir(&trash);
            }
        }
    }

    /// True for anything sitting in a holding folder or in the Windows Recycle Bin — i.e.
    /// deleted, whether this app is holding it or the shell is. Search asks, because an item the
    /// user has just deleted turning up in results looks exactly like a delete that did not work;
    /// and a recycled file surfacing as `C:\$Recycle.Bin\S-1-5-21-…\$RAB1234.txt` is worse still,
    /// since the name it turns up under is not even the one that was deleted. Asked once per row
    /// by a search and by the flat view, so the segments are walked in place.
    pub fn is_held_path(path: &str) -> bool {
        path.split(['\\', '/']).any(|segment| {
            !segment.is_empty()
                && (ProtectedLocations::is_recycle_bin_name(segment)
                    || segment.eq_ignore_ascii_case(Self::TRASH_FOLDER_NAME)
                    || segment
                        .get(..Self::LOCAL_STAGING_PREFIX.len())
                        .is_some_and(|head| head.eq_ignore_ascii_case(Self::LOCAL_STAGING_PREFIX)))
        })
    }

    /// The guard that makes the recursive deletes above safe: a path only counts if it is named
    /// the way this type names its holding folders.
    fn is_staging_directory(path: &str) -> bool {
        let trimmed = path.trim_end_matches(['\\', '/']);
        let (directory, name) = split_last(trimmed);
        if name.starts_with(Self::LOCAL_STAGING_PREFIX) {
            return true;
        }
        name.starts_with(Self::BATCH_PREFIX) && file_name(directory) == Self::TRASH_FOLDER_NAME
    }

    /// Takes the per-volume trash folder away once its last batch has gone, so a disk that is not
    /// mid-delete carries no trace of one.
    fn remove_empty_trash_root(batch_directory: &str) {
        let (trash, _) = split_last(batch_directory.trim_end_matches(['\\', '/']));
        if file_name(trash) != Self::TRASH_FOLDER_NAME {
            return;
        }
        if is_dir(trash) && is_empty_dir(trash) {
            let _ = std::fs::remove_dir(trash);
        }
    }

    // --- Filesystem primitives ---

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
        self.probe.file_exists(path) || self.probe.directory_exists(path)
    }
}

/// `MoveFileEx` as `File.Move` and `Directory.Move` called it: never over an existing entry, and a
/// file — only a file — may be copied across volumes.
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

/// Creates the folder (and whatever is missing above it) and hides it. Visible staging is ugly but
/// not wrong, so the attribute is best-effort.
fn create_staging_directory(path: &str) -> io::Result<String> {
    std::fs::create_dir_all(path)?;
    if let Some(attributes) = attributes_of(path) {
        set_attributes(path, attributes | HIDDEN);
    }
    Ok(path.to_string())
}

fn clear_read_only_attribute(path: &str) {
    if let Some(attributes) = attributes_of(path) {
        set_attributes(path, attributes & !READONLY);
    }
}

fn attributes_of(path: &str) -> Option<u32> {
    EntryAttributes::of(path).map(|e| e.attributes)
}

fn set_attributes(path: &str, attributes: u32) {
    // SAFETY: a NUL-terminated path.
    unsafe { SetFileAttributesW(wide(path).as_ptr(), attributes) };
}

fn is_dir(path: &str) -> bool {
    EntryAttributes::of(path).is_some_and(|e| e.is_directory())
}

fn is_empty_dir(path: &str) -> bool {
    std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

/// `Path.Combine`: one separator between, whether or not the directory already ends in one.
fn join(directory: &str, name: &str) -> String {
    if directory.ends_with(['\\', '/']) {
        format!("{directory}{name}")
    } else {
        format!("{directory}\\{name}")
    }
}

/// Eight hex characters nothing else on the disk is likely to share.
fn short_id() -> String {
    format!("{:08x}", RandomState::new().hash_one(0u8) as u32)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
