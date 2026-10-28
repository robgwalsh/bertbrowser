//! The Windows Recycle Bin, as much of it as this app needs: put items in, and take one back out.
//! Spec: Core/Services/Delete/IRecycleBin.cs, ShellRecycleBin.cs.
//!
//! `IFileOperation` rather than the older `SHFileOperation` for one decisive reason: its progress
//! sink's `PostDeleteItem` hands back `psiNewlyCreated`, which is the item as it now exists inside
//! the bin (`$Recycle.Bin\<SID>\$RXXXXXX.ext`) — or null when the shell erased it rather than
//! holding it. Capturing that path is what lets undo restore *this* item rather than searching the
//! bin for something whose original path looks similar, which is the difference between correct
//! and nearly-correct when the same path has been deleted twice.
//!
//! **The flags are the dangerous part.** `FOF_ALLOWUNDO` together with `FOF_NOCONFIRMATION` means
//! "if it cannot be recycled, erase it without asking", which on a full bin or a network share
//! destroys data silently — the opposite of everything this app's delete design stands for. Two
//! things stand against that: the planner routes items on volumes with no bin to the staging
//! folder before this type ever sees them, and `FOF_WANTNUKEWARNING` stays set so the one case
//! pre-flight cannot predict — an item over the bin's quota — still asks. That is the only
//! OS-drawn dialog this app allows, and it is deliberate: every other confirmation, progress bar
//! and error box is suppressed so the app's own dialog stays the confirmation the user answers.
//!
//! Everything runs on a dedicated apartment thread with a deadline: `IFileOperation` is
//! apartment-threaded and may put up UI. Progress crosses back to the caller's thread over a
//! channel, so the sink the shell calls into never touches a callback that is not `Send`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use bb_paths::{PathKey, fold_upper, root_len};
use windows::Win32::Foundation::{HANDLE, HWND, S_OK};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CMF_NORMAL, CMINVOKECOMMANDINFO, CSIDL_BITBUCKET, FILEOPERATION_FLAGS, FileOperation,
    IContextMenu, IEnumIDList, IFileOperation, IFileOperationProgressSink,
    IFileOperationProgressSink_Impl, ILFree, IShellFolder, IShellItem, SHCONTF_FOLDERS,
    SHCONTF_INCLUDEHIDDEN, SHCONTF_NONFOLDERS, SHCreateItemFromParsingName, SHCreateShellItem,
    SHGetDesktopFolder, SHGetSpecialFolderLocation, SHQUERYRBINFO, SHQueryRecycleBinW,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemID, GetMenuStringW, MF_BYPOSITION,
    SW_SHOWNORMAL,
};
use windows::core::{ComObject, HRESULT, PCSTR, PCWSTR, Ref, implement};
use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumeInformationW};

use super::models::{DeleteProgress, DeletedItem, FailedDelete, PlannedDelete};
use super::probe::RecycleProbe;
use crate::cancel::CancelToken;
use crate::fs::AccessDenied;

/// One item the Recycle Bin took.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecycledItem {
    /// Where it was.
    pub source_path: String,
    pub is_directory: bool,
    /// Its `$R` path inside the bin, or None when the shell erased it outright instead of holding
    /// it — which is not a failure, but does mean there is nothing to restore.
    pub recycled_path: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecycleResult {
    /// Items the bin accepted.
    pub recycled: Vec<RecycledItem>,
    /// Items it refused, one message each. A failure here never affects the rest.
    pub failed: Vec<FailedDelete>,
}

/// The Windows Recycle Bin, behind a trait so the executor's own rules stay testable against an
/// in-memory bin rather than against the user's real one.
pub trait RecycleBin {
    /// Sends items to the bin, as one batch: the shell wants one operation with every item added
    /// to it, and that is also the only way to get a single progress sink and a single
    /// confirmation. Never fails for an individual item — a refusal comes back in
    /// [`RecycleResult::failed`] so one bad item cannot cost the others.
    fn recycle(
        &self,
        items: &[PlannedDelete],
        cancel: &CancelToken,
        progress: Option<&dyn Fn(DeleteProgress)>,
    ) -> RecycleResult;

    /// Puts one item back where it came from. False when the bin no longer holds it — it was
    /// emptied, swept by Storage Sense, or restored by hand in the meantime.
    fn restore(&self, item: &DeletedItem) -> bool;
}

/// The real Recycle Bin, over `IFileOperation`.
#[derive(Default)]
pub struct ShellRecycleBin {
    /// Per-volume answers, cached for the life of this object: a plan asks once per item and the
    /// answer does not change between the plan and its execution in any way that matters.
    can_recycle: Mutex<HashMap<String, bool>>,
}

/// Long enough for a big batch of renames — recycling does not copy — and short enough that a
/// wedged shell does not hang the app forever.
const OPERATION_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// A restore is a shell verb that reports nothing, so success is observed rather than returned.
const RESTORE_TIMEOUT: Duration = Duration::from_secs(5);

const FOF_SILENT: u32 = 0x0004;
const FOF_NOCONFIRMATION: u32 = 0x0010;
const FOF_ALLOWUNDO: u32 = 0x0040;
const FOF_NOCONFIRMMKDIR: u32 = 0x0200;
const FOF_NOERRORUI: u32 = 0x0400;
const FOF_WANTNUKEWARNING: u32 = 0x4000;
const FOFX_NOMINIMIZEBOX: u32 = 0x0100_0000;
const FOFX_RECYCLEONDELETE: u32 = 0x0008_0000;
const FOFX_ADDUNDORECORD: u32 = 0x2000_0000;

/// Three flags say "recycle, do not erase" — `FOF_ALLOWUNDO` is the long-standing one,
/// `FOFX_RECYCLEONDELETE` and `FOFX_ADDUNDORECORD` are its modern spellings — because the cost of
/// one of them being wrong is the user's files. The suppression flags silence the shell's own
/// progress, confirmation and error UI, since this app draws its own; the one deliberate exception
/// is `FOF_WANTNUKEWARNING`, which overrides `FOF_NOCONFIRMATION` for exactly the "this cannot be
/// recycled — delete permanently?" case that pre-flight cannot predict.
///
/// `FOFX_EARLYFAILURE` is deliberately absent: it would abandon the rest of the batch on the first
/// error, and everything else in this app's delete path holds that one item's failure must never
/// cost the others.
const OPERATION_FLAGS: FILEOPERATION_FLAGS = FILEOPERATION_FLAGS(
    FOF_ALLOWUNDO
        | FOFX_RECYCLEONDELETE
        | FOFX_ADDUNDORECORD
        | FOF_SILENT
        | FOF_NOCONFIRMATION
        | FOF_NOERRORUI
        | FOF_NOCONFIRMMKDIR
        | FOFX_NOMINIMIZEBOX
        | FOF_WANTNUKEWARNING,
);

/// The copy engine's own access-denied codes. It has its own facility and does not reuse the
/// Win32 one; see [`is_access_denied`].
const COPYENGINE_E_ACCESS_DENIED_SRC: i32 = 0x8027_0021_u32 as i32;
const COPYENGINE_E_ACCESS_DENIED_DEST: i32 = 0x8027_0022_u32 as i32;

const DRIVE_NO_ROOT_DIR: u32 = 1;
const DRIVE_REMOTE: u32 = 4;

/// Command ids handed to the bin's context menu when it is filled.
const MENU_ID_FIRST: u32 = 1;
const MENU_ID_LAST: u32 = 0x7FFF;

impl ShellRecycleBin {
    pub fn new() -> ShellRecycleBin {
        ShellRecycleBin::default()
    }
}

// --- RecycleProbe ---

impl RecycleProbe for ShellRecycleBin {
    fn can_recycle(&self, path: &str) -> bool {
        let Ok(full) = PathKey::normalize_display(path) else {
            return false;
        };
        let root = &full[..root_len(&full)];
        if root.is_empty() {
            return false;
        }

        let key = fold_upper(root);
        let mut cache = self.can_recycle.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&cached) = cache.get(&key) {
            return cached;
        }
        let answer = query_can_recycle(root);
        cache.insert(key, answer);
        answer
    }
}

/// A network share has no Recycle Bin at all, and a volume that is not ready cannot be asked.
/// Beyond that, `SHQueryRecycleBin` succeeding is the shell's own answer to "is there a bin here".
/// The per-volume "don't move files to the Recycle Bin" setting is deliberately not probed —
/// finding it means resolving a volume GUID — because `FOF_WANTNUKEWARNING` catches that case
/// honestly at the point of deletion instead.
fn query_can_recycle(root: &str) -> bool {
    let wide = wide(root);
    // SAFETY: a NUL-terminated root, valid for both calls; the out pointers are valid for the call.
    unsafe {
        let drive_type = GetDriveTypeW(wide.as_ptr());
        if drive_type == DRIVE_NO_ROOT_DIR || drive_type == DRIVE_REMOTE {
            return false;
        }
        let mut serial = 0u32;
        let mut max_component = 0u32;
        let mut flags = 0u32;
        let ready = GetVolumeInformationW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            &mut max_component,
            &mut flags,
            std::ptr::null_mut(),
            0,
        );
        if ready == 0 {
            return false;
        }

        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        SHQueryRecycleBinW(PCWSTR(wide.as_ptr()), &mut info).is_ok()
    }
}

// --- RecycleBin ---

/// What the apartment thread sends back: progress as it happens, then the one result.
enum Event {
    Progress(DeleteProgress),
    Done(Result<RecycleResult, String>),
}

impl RecycleBin for ShellRecycleBin {
    fn recycle(
        &self,
        items: &[PlannedDelete],
        cancel: &CancelToken,
        progress: Option<&dyn Fn(DeleteProgress)>,
    ) -> RecycleResult {
        if items.is_empty() {
            return RecycleResult::default();
        }

        let (events, inbox) = mpsc::channel::<Event>();
        let batch = items.to_vec();
        let cancel = cancel.clone();
        let spawned = std::thread::Builder::new()
            .name("Recycle Bin".to_string())
            .spawn(move || {
                // Carried back rather than unwound here: a panic on this thread would be
                // unobserved, and the caller would be left believing nothing happened.
                let result = Apartment::enter()
                    .and_then(|_apartment| recycle_core(&batch, &cancel, &events));
                let _ = events.send(Event::Done(result));
            });
        if spawned.is_err() {
            return all_failed(items, "the Recycle Bin could not be reached.");
        }

        let deadline = Instant::now() + OPERATION_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match inbox.recv_timeout(remaining) {
                Ok(Event::Progress(report)) => {
                    if let Some(progress) = progress {
                        progress(report);
                    }
                }
                Ok(Event::Done(Ok(result))) => return result,
                Ok(Event::Done(Err(reason))) => return all_failed(items, &reason),
                Err(RecvTimeoutError::Timeout) => {
                    return all_failed(items, "the Recycle Bin did not respond.");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return all_failed(items, "the Recycle Bin could not be reached.");
                }
            }
        }
    }

    /// Restoring goes through the shell's own `undelete` verb, so the bin's bookkeeping — the
    /// paired `$I` metadata file — is cleaned up by the code that owns it. The canonical verb name
    /// is used rather than the menu text, which is localised.
    ///
    /// The verb returns nothing at all, so success is established by watching for the original
    /// path to come back. A timeout is reported as a failure naming the `$R` path, so the data is
    /// findable rather than quietly lost. One genuine difference from the staged undo: the shell
    /// recreates a missing parent folder chain, where restoring from the holding folder refuses.
    fn restore(&self, item: &DeletedItem) -> bool {
        let Some(held) = item.recycled_path.as_deref().filter(|h| !h.is_empty()) else {
            return false;
        };

        let (answer, inbox) = mpsc::channel::<bool>();
        let held = held.to_string();
        let source = item.source_path.clone();
        let spawned = std::thread::Builder::new()
            .name("Recycle Bin restore".to_string())
            .spawn(move || {
                // Any failure is reported by the caller as "the bin no longer holds it", which is
                // what a failure here means in every case we can distinguish.
                let restored = Apartment::enter()
                    .map(|_apartment| restore_core(&held, &source))
                    .unwrap_or(false);
                let _ = answer.send(restored);
            });
        if spawned.is_err() {
            return false;
        }
        inbox
            .recv_timeout(RESTORE_TIMEOUT + Duration::from_secs(5))
            .unwrap_or(false)
    }
}

fn all_failed(items: &[PlannedDelete], reason: &str) -> RecycleResult {
    RecycleResult {
        recycled: Vec::new(),
        failed: items
            .iter()
            .map(|i| FailedDelete::new(&i.source_path, format!("{}: {reason}", i.name())))
            .collect(),
    }
}

/// A single-threaded apartment for the life of the value, on the thread that made it.
struct Apartment;

impl Apartment {
    fn enter() -> Result<Apartment, String> {
        // SAFETY: no pointers; paired with CoUninitialize on drop.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if hr.is_err() {
            return Err(format!("COM could not be initialised ({}).", hr.message()));
        }
        Ok(Apartment)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balances the CoInitializeEx in `enter`, on the same thread.
        unsafe { CoUninitialize() };
    }
}

/// How a batch ends short of its result.
enum BatchFailure {
    /// Cancelled between adds: nothing was performed, and every item is reported as not done.
    Cancelled,
    /// The shell refused somewhere; whatever the sink already recorded still stands.
    Shell(String),
}

fn recycle_core(
    items: &[PlannedDelete],
    cancel: &CancelToken,
    events: &Sender<Event>,
) -> Result<RecycleResult, String> {
    // SAFETY: a COM call on an apartment thread; the result is an owned interface.
    let operation: IFileOperation = unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }
        .map_err(|e| {
        format!(
            "The shell's file-operation object could not be created ({}).",
            e.message()
        )
    })?;

    let sink = ComObject::new(DeleteSink::new(items, events.clone()));
    let callback: IFileOperationProgressSink = sink.to_interface();
    let mut cookie = 0u32;

    let shell = |e: windows::core::Error| BatchFailure::Shell(e.message());
    // SAFETY: COM calls on the apartment thread that created `operation`; every interface passed
    // outlives the call.
    let batch: Result<(), BatchFailure> = unsafe {
        (|| {
            operation
                .SetOperationFlags(OPERATION_FLAGS)
                .map_err(shell)?;
            cookie = operation.Advise(&callback).map_err(shell)?;

            // Everything is added first and performed once: that is what gives a single sink, and
            // it is also far faster than one operation per item.
            //
            // Each add is guarded separately, and that guard is load-bearing. Resolving a path
            // that has since gone fails here — before PerformOperations has run — so a single
            // missing item would otherwise abort the batch and report every one of its siblings
            // as failed, having deleted none of them. One item's failure must never cost the
            // others.
            let mut added = 0;
            for item in items {
                if cancel.is_cancelled() {
                    return Err(BatchFailure::Cancelled);
                }
                let shell_item: windows::core::Result<IShellItem> =
                    SHCreateItemFromParsingName(PCWSTR(wide(&item.source_path).as_ptr()), None);
                match shell_item.and_then(|shell_item| operation.DeleteItem(&shell_item, None)) {
                    Ok(()) => added += 1,
                    Err(e) => sink.get().note_item_failure(item, &e.message()),
                }
            }

            // Performing an empty operation is legal but pointless, and skipping it keeps the
            // shell from putting anything on screen when every item was already gone.
            if added > 0 {
                operation.PerformOperations().map_err(shell)?;
            }
            Ok(())
        })()
    };

    if cookie != 0 {
        // SAFETY: the cookie came from Advise on this operation.
        let _ = unsafe { operation.Unadvise(cookie) };
    }
    drop(operation);

    match batch {
        Ok(()) => {}
        Err(BatchFailure::Cancelled) => return Err("the operation was cancelled.".to_string()),
        Err(BatchFailure::Shell(message)) => sink.get().note_batch_failure(message),
    }
    Ok(sink.get().result())
}

// --- The progress sink ---

#[derive(Default)]
struct SinkState {
    recycled: Vec<RecycledItem>,
    failed: Vec<FailedDelete>,
    seen: HashSet<String>,
    done: usize,
    batch_failure: Option<String>,
}

/// Records what happened to each item. `PostDeleteItem` is the only reason this exists:
/// `psiNewlyCreated` is the item's new home in the bin, and null means the shell erased it instead
/// — not a failure, but nothing to undo either.
#[implement(IFileOperationProgressSink)]
struct DeleteSink {
    items: Vec<PlannedDelete>,
    /// By upper-cased source path: the shell hands paths back in its own casing.
    by_source: HashMap<String, PlannedDelete>,
    events: Sender<Event>,
    state: RefCell<SinkState>,
}

impl DeleteSink {
    fn new(items: &[PlannedDelete], events: Sender<Event>) -> DeleteSink {
        DeleteSink {
            items: items.to_vec(),
            by_source: items
                .iter()
                .map(|i| (fold_upper(&i.source_path), i.clone()))
                .collect(),
            events,
            state: RefCell::new(SinkState::default()),
        }
    }

    fn note_batch_failure(&self, message: String) {
        self.state.borrow_mut().batch_failure = Some(message);
    }

    /// Records one item the shell would not even accept — most often a path that has gone since
    /// the plan was made. Marked as seen so [`Self::result`] does not report it a second time
    /// under the generic message.
    fn note_item_failure(&self, item: &PlannedDelete, message: &str) {
        let mut state = self.state.borrow_mut();
        state.seen.insert(fold_upper(&item.source_path));
        state.failed.push(FailedDelete::new(
            &item.source_path,
            format!("{}: {message}", item.name()),
        ));
    }

    fn result(&self) -> RecycleResult {
        let mut state = self.state.borrow_mut();
        // Anything the shell never reported on did not happen. Saying so beats leaving the caller
        // to believe an item was deleted when it is still on disk.
        for item in &self.items {
            if state.seen.contains(&fold_upper(&item.source_path)) {
                continue;
            }
            let reason = state
                .batch_failure
                .clone()
                .unwrap_or_else(|| "the Recycle Bin did not take it.".to_string());
            state.failed.push(FailedDelete::new(
                &item.source_path,
                format!("{}: {reason}", item.name()),
            ));
        }
        RecycleResult {
            recycled: std::mem::take(&mut state.recycled),
            failed: std::mem::take(&mut state.failed),
        }
    }

    fn planned_for(&self, item: Ref<'_, IShellItem>) -> Option<(String, &PlannedDelete)> {
        let path = path_of(item.as_ref())?;
        let key = fold_upper(&path);
        let planned = self.by_source.get(&key)?;
        Some((key, planned))
    }
}

impl IFileOperationProgressSink_Impl for DeleteSink_Impl {
    fn PreDeleteItem(&self, _flags: u32, item: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        if let Some((_, planned)) = self.planned_for(item) {
            let _ = self.events.send(Event::Progress(DeleteProgress {
                done: self.state.borrow().done,
                total: self.items.len(),
                current_name: planned.name().to_string(),
            }));
        }
        Ok(())
    }

    fn PostDeleteItem(
        &self,
        _flags: u32,
        item: Ref<'_, IShellItem>,
        hr_delete: HRESULT,
        newly_created: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.state.borrow_mut().done += 1;

        // A child of a folder we deleted; only the roots we asked for are recorded.
        let Some((key, planned)) = self.planned_for(item) else {
            return Ok(());
        };
        let mut state = self.state.borrow_mut();
        state.seen.insert(key);

        if hr_delete.is_err() {
            // The shell hands back a per-item HRESULT, so unlike every other failure in this
            // codebase the permission case arrives already distinguished — no exception to
            // classify, just a comparison. Which codes count is the part to get right.
            let denied = is_access_denied(hr_delete.0);
            let reason = if denied {
                "access is denied.".to_string()
            } else {
                let message = hr_delete.message();
                if message.is_empty() {
                    "could not be deleted.".to_string()
                } else {
                    message
                }
            };
            state.failed.push(FailedDelete {
                source_path: planned.source_path.clone(),
                message: format!("{}: {reason}", planned.name()),
                access_denied: denied,
            });
            return Ok(());
        }

        // Null means it was erased rather than held — over the bin's quota, most often. The item
        // is gone either way; what changes is whether there is anything to undo.
        state.recycled.push(RecycledItem {
            source_path: planned.source_path.clone(),
            is_directory: planned.is_directory,
            recycled_path: path_of(newly_created.as_ref()),
        });
        Ok(())
    }

    // Everything below is unused, but the interface has no gaps to spare.

    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn FinishOperations(&self, _hr: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreRenameItem(
        &self,
        _flags: u32,
        _item: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostRenameItem(
        &self,
        _flags: u32,
        _item: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
        _hr: HRESULT,
        _newly_created: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreMoveItem(
        &self,
        _flags: u32,
        _item: Ref<'_, IShellItem>,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostMoveItem(
        &self,
        _flags: u32,
        _item: Ref<'_, IShellItem>,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
        _hr: HRESULT,
        _newly_created: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreCopyItem(
        &self,
        _flags: u32,
        _item: Ref<'_, IShellItem>,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostCopyItem(
        &self,
        _flags: u32,
        _item: Ref<'_, IShellItem>,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
        _hr: HRESULT,
        _newly_created: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreNewItem(
        &self,
        _flags: u32,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostNewItem(
        &self,
        _flags: u32,
        _destination: Ref<'_, IShellItem>,
        _new_name: &PCWSTR,
        _template_name: &PCWSTR,
        _attributes: u32,
        _hr: HRESULT,
        _new_item: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn UpdateProgress(&self, _total: u32, _so_far: u32) -> windows::core::Result<()> {
        Ok(())
    }

    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Whether the shell refused an item for permissions.
///
/// **Measured, not assumed, and the plain Win32 code is not enough.** Deleting a file carrying a
/// Deny ACE comes back as `COPYENGINE_E_ACCESS_DENIED_SRC` (`0x80270021`), not `0x80070005` — the
/// copy engine has its own facility and uses it. Checking only the Win32 HRESULT meant a genuinely
/// protected file was reported as an ordinary failure and never offered an administrator retry,
/// which is precisely the case the whole feature exists for.
fn is_access_denied(hr: i32) -> bool {
    hr == AccessDenied::HRESULT
        || hr == COPYENGINE_E_ACCESS_DENIED_SRC
        || hr == COPYENGINE_E_ACCESS_DENIED_DEST
}

// --- Restore ---

fn restore_core(held: &str, source: &str) -> bool {
    // SAFETY: COM calls on the apartment thread; the bin's PIDL is freed after its last use.
    unsafe {
        let Ok(desktop) = SHGetDesktopFolder() else {
            return false;
        };
        let Ok(bin_pidl) = SHGetSpecialFolderLocation(None, CSIDL_BITBUCKET as i32) else {
            return false;
        };
        let restored = desktop
            .BindToObject::<_, IShellFolder>(bin_pidl, None)
            .ok()
            .is_some_and(|bin| find_and_restore(&bin, held, source));
        ILFree(Some(bin_pidl));
        restored
    }
}

/// Walks the bin for the one entry whose `$R` path is `held`, and restores it.
unsafe fn find_and_restore(bin: &IShellFolder, held: &str, source: &str) -> bool {
    // SAFETY: the caller's; every PIDL the enumerator hands out is freed before the next.
    unsafe {
        let mut enumerator: Option<IEnumIDList> = None;
        let flags = (SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0 | SHCONTF_INCLUDEHIDDEN.0) as u32;
        if bin
            .EnumObjects(HWND::default(), flags, &mut enumerator)
            .is_err()
        {
            return false;
        }
        let Some(enumerator) = enumerator else {
            return false;
        };

        let wanted = fold_upper(held);
        loop {
            let mut child: [*mut ITEMIDLIST; 1] = [std::ptr::null_mut()];
            let mut fetched = 0u32;
            if enumerator.Next(&mut child, Some(&mut fetched)) != S_OK || fetched == 0 {
                return false; // emptied, swept by Storage Sense, or already restored by hand
            }
            let pidl = child[0];

            let path = SHCreateShellItem(None, bin, pidl)
                .ok()
                .and_then(|item| path_of(Some(&item)));
            if !path.is_some_and(|p| fold_upper(&p) == wanted) {
                ILFree(Some(pidl));
                continue;
            }

            let invoked = invoke_undelete(bin, pidl);
            ILFree(Some(pidl));
            if !invoked {
                return false;
            }

            // The verb reports nothing, so the only honest answer is to watch for the file.
            let deadline = Instant::now() + RESTORE_TIMEOUT;
            while Instant::now() < deadline {
                if std::fs::symlink_metadata(source).is_ok() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            return false;
        }
    }
}

/// The canonical verb name is stable across languages; the displayed one ("Restore", "&Restore",
/// and every translation) is not, so it is only a fallback. The menu is filled first because a
/// handler resolves a verb against the menu it was asked to build.
unsafe fn invoke_undelete(bin: &IShellFolder, pidl: *mut ITEMIDLIST) -> bool {
    // SAFETY: the caller's; the menu handle is destroyed before returning.
    unsafe {
        let Ok(menu) =
            bin.GetUIObjectOf::<IContextMenu>(HWND::default(), &[pidl as *const ITEMIDLIST], None)
        else {
            return false;
        };
        let Ok(hmenu) = CreatePopupMenu() else {
            return false;
        };
        let _ = menu.QueryContextMenu(hmenu, 0, MENU_ID_FIRST, MENU_ID_LAST, CMF_NORMAL);

        let mut info = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            fMask: 0,
            hwnd: HWND::default(),
            lpVerb: PCSTR(c"undelete".as_ptr().cast()),
            lpParameters: PCSTR::null(),
            lpDirectory: PCSTR::null(),
            nShow: SW_SHOWNORMAL.0,
            dwHotKey: 0,
            hIcon: HANDLE::default(),
        };
        let mut invoked = menu.InvokeCommand(&info).is_ok();

        if !invoked {
            let count = GetMenuItemCount(Some(hmenu)).max(0) as u32;
            for position in 0..count {
                let mut buffer = [0u16; 256];
                let len = GetMenuStringW(hmenu, position, Some(&mut buffer), MF_BYPOSITION);
                let text =
                    String::from_utf16_lossy(&buffer[..len.max(0) as usize]).replace('&', "");
                if !text.eq_ignore_ascii_case("Restore") {
                    continue;
                }
                let id = GetMenuItemID(hmenu, position as i32);
                if id == u32::MAX || id < MENU_ID_FIRST {
                    continue;
                }
                info.lpVerb = PCSTR((id - MENU_ID_FIRST) as usize as *const u8);
                invoked = menu.InvokeCommand(&info).is_ok();
                if invoked {
                    break;
                }
            }
        }

        let _ = DestroyMenu(hmenu);
        invoked
    }
}

// --- Shell plumbing ---

/// The item's filesystem path, or None for anything that is not a filesystem item — nothing we
/// could match against.
fn path_of(item: Option<&IShellItem>) -> Option<String> {
    let item = item?;
    // SAFETY: a COM call; the returned string is the shell's, read once and freed here.
    unsafe {
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        if name.is_null() {
            return None;
        }
        let text = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const _));
        text
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
