//! Keeps the search index fresh for crawled roots: one `ReadDirectoryChangesW` thread per indexed
//! root patches `fs_entry` as files are created, changed, deleted or renamed. Events are queued and
//! drained every 250 ms so bulk operations batch into few transactions. On a buffer overflow or
//! other watcher failure the root is marked stale and the watcher dropped: the next search serves
//! cached results and triggers a re-crawl. Spec: Core/Services/IndexWatcherService.cs.
//!
//! The part worth testing, applying a drained batch to the index, is `ChangeApplier`, which has no
//! thread and no OS handle in it.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bb_paths::PathKey;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED, FILE_ACTION_RENAMED_NEW_NAME,
    FILE_ACTION_RENAMED_OLD_NAME, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
    FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, ReadDirectoryChangesW,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects,
};

use super::walker::FileSystemWalker;
use crate::FileTime;
use crate::attributes::{DIRECTORY, HIDDEN};
use crate::cancel::CancelToken;
use crate::data::{FsEntryRow, FsIndexRepository};

/// What a search asks of the watchers.
pub trait IndexWatcher: Send + Sync {
    /// True if live change events are currently being applied for this indexed root.
    fn is_watching(&self, root_key: &str) -> bool;

    /// Starts (or refreshes) watching an indexed root. No-op for UNC paths, where directory
    /// notifications are unreliable; those roots stay on the stale-while-revalidate path forever.
    fn watch(&self, root_key: &str, display_path: &str);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsChangeKind {
    Created,
    Changed,
    Deleted,
    Renamed,
}

/// One change the watcher saw, under the indexed root it was watching.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsChange {
    pub root_key: String,
    pub kind: FsChangeKind,
    pub full_path: String,
    /// The previous path, for a rename.
    pub old_full_path: Option<String>,
}

const MAX_WATCHERS: usize = 8;
const MINI_CRAWL_CAP: usize = 25_000;
const DRAIN_INTERVAL: Duration = Duration::from_millis(250);
const BUFFER_SIZE: usize = 64 * 1024;

/// Applies drained batches of watcher events to the index.
pub struct ChangeApplier {
    repository: Arc<FsIndexRepository>,
}

impl ChangeApplier {
    pub fn new(repository: Arc<FsIndexRepository>) -> ChangeApplier {
        ChangeApplier { repository }
    }

    /// Applies a batch. A single unappliable event (a transient IO or DB error) must not poison
    /// the batch: its root is marked stale instead, and the worst case is an entry that is wrong
    /// until the next re-crawl.
    pub fn apply(&self, changes: &[FsChange]) {
        let crawl_gen = FileTime::now().to_unix_millis();
        let mut upserts: Vec<FsEntryRow> = Vec::new();

        for change in changes {
            let applied = match change.kind {
                FsChangeKind::Created | FsChangeKind::Changed => {
                    self.collect_upsert(change, &mut upserts, crawl_gen)
                }
                FsChangeKind::Deleted => self.flush(&mut upserts, crawl_gen).and_then(|_| {
                    let key = PathKey::canonicalize(&change.full_path)?;
                    self.repository.delete_subtree(&key)
                }),
                FsChangeKind::Renamed => match &change.old_full_path {
                    Some(old) => self.flush(&mut upserts, crawl_gen).and_then(|_| {
                        self.repository.rename(
                            &PathKey::canonicalize(old)?,
                            &PathKey::canonicalize(&change.full_path)?,
                            bb_paths::file_name(&change.full_path),
                            crawl_gen,
                        )
                    }),
                    None => Ok(()),
                },
            };
            if applied.is_err() {
                let _ = self.repository.mark_root_stale(&change.root_key);
            }
        }
        if self.flush(&mut upserts, crawl_gen).is_err()
            && let Some(change) = changes.last()
        {
            let _ = self.repository.mark_root_stale(&change.root_key);
        }
    }

    fn flush(&self, upserts: &mut Vec<FsEntryRow>, crawl_gen: i64) -> crate::data::Result<()> {
        if upserts.is_empty() {
            return Ok(());
        }
        self.repository.upsert_entries(upserts, crawl_gen)?;
        upserts.clear();
        Ok(())
    }

    fn collect_upsert(
        &self,
        change: &FsChange,
        upserts: &mut Vec<FsEntryRow>,
        crawl_gen: i64,
    ) -> crate::data::Result<()> {
        let key = PathKey::canonicalize(&change.full_path)?;
        let hidden = is_effectively_hidden(&change.full_path, &change.root_key);
        let name = bb_paths::file_name(&change.full_path).to_string();

        // The entry's own attributes, as GetFileAttributesEx reported them: a junction is a
        // directory whether or not its target is there.
        match std::fs::symlink_metadata(&change.full_path) {
            Ok(meta) if meta.file_attributes() & DIRECTORY != 0 => {
                upserts.push(FsEntryRow {
                    path_key: key,
                    name,
                    is_directory: true,
                    size_bytes: 0,
                    modified_utc: FileTime(meta.last_write_time() as i64),
                    hidden,
                    attributes: meta.file_attributes(),
                    created_utc: FileTime(meta.creation_time() as i64),
                });
                // A folder moved into the tree raises a single Created event for the top directory
                // only: index its contents with a bounded mini-crawl, seeding the walk with this
                // folder's effective hidden state so descendants inherit it.
                if change.kind == FsChangeKind::Created {
                    self.mini_crawl(change, upserts, crawl_gen, hidden)?;
                }
                Ok(())
            }
            Ok(meta) => {
                upserts.push(FsEntryRow {
                    path_key: key,
                    name,
                    is_directory: false,
                    size_bytes: meta.file_size() as i64,
                    modified_utc: FileTime(meta.last_write_time() as i64),
                    hidden,
                    attributes: meta.file_attributes(),
                    created_utc: FileTime(meta.creation_time() as i64),
                });
                Ok(())
            }
            // Already gone again (temp file churn); treat as a delete.
            Err(_) => self.repository.delete_subtree(&key),
        }
    }

    fn mini_crawl(
        &self,
        change: &FsChange,
        upserts: &mut Vec<FsEntryRow>,
        crawl_gen: i64,
        root_hidden: bool,
    ) -> crate::data::Result<()> {
        let mut count = 0usize;
        let mut capped = false;
        let mut failure = None;
        let _ = FileSystemWalker::walk(
            &change.full_path,
            true,
            root_hidden,
            &CancelToken::none(),
            |entry| {
                count += 1;
                if count > MINI_CRAWL_CAP {
                    capped = true;
                    return false;
                }
                upserts.push(FsEntryRow {
                    path_key: entry.path_key.clone(),
                    name: entry.name.clone(),
                    is_directory: entry.is_directory,
                    size_bytes: entry.size_bytes,
                    modified_utc: entry.modified_utc,
                    hidden: entry.hidden,
                    attributes: entry.attributes,
                    created_utc: entry.created_utc,
                });
                if upserts.len() >= 20_000 {
                    if let Err(e) = self.repository.upsert_entries(upserts, crawl_gen) {
                        failure = Some(e);
                        return false;
                    }
                    upserts.clear();
                }
                true
            },
        );
        if let Some(e) = failure {
            return Err(e);
        }
        if capped {
            // Too big to patch live; re-crawl on next search.
            self.repository.mark_root_stale(&change.root_key)?;
        }
        Ok(())
    }
}

/// Effective hidden state for a path the watcher just saw change: its own Hidden attribute or that
/// of any ancestor down to, but not including, the indexed root. Stopping at the root mirrors how
/// a full crawl seeds the root as non-hidden, so watcher and crawl agree even when the root itself
/// sits inside a hidden system folder (an indexed subtree under %AppData%). Best-effort: a lost
/// stat means "not hidden".
fn is_effectively_hidden(full_path: &str, root_key: &str) -> bool {
    let mut path = full_path.to_string();
    loop {
        match PathKey::canonicalize(&path) {
            Ok(key) if key == root_key => return false, // the root's own state doesn't count
            Ok(_) => {}
            Err(_) => return false,
        }
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_attributes() & HIDDEN != 0 => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
        let (parent, _) = bb_paths::split_last(path.trim_end_matches(['\\', '/']));
        if parent.is_empty() || parent.len() >= path.len() {
            return false;
        }
        // "C:" is the drive, not a folder; Path.GetDirectoryName gives "C:\" there.
        path = if parent.len() == 2 && parent.as_bytes()[1] == b':' {
            format!("{parent}\\")
        } else {
            parent.to_string()
        };
    }
}

// --- the live watchers ---

/// A Win32 event handle, closed on drop.
struct Event(HANDLE);

// SAFETY: an event handle is a process-wide kernel object; signalling it from any thread is the
// point of it.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    fn manual_reset() -> Event {
        // SAFETY: no attributes, manual reset, initially unsignalled, unnamed.
        Event(unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) })
    }

    fn set(&self) {
        // SAFETY: a valid event handle.
        unsafe { SetEvent(self.0) };
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: closing the handle this struct owns.
        unsafe { CloseHandle(self.0) };
    }
}

struct WatcherEntry {
    stop: Arc<Event>,
    thread: Option<JoinHandle<()>>,
    last_touched: Instant,
}

impl WatcherEntry {
    fn stop_and_join(mut self) {
        self.stop.set();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Inner {
    applier: ChangeApplier,
    repository: Arc<FsIndexRepository>,
    watchers: Mutex<HashMap<String, WatcherEntry>>,
    pending: Mutex<Vec<FsChange>>,
    draining: Mutex<bool>,
    stopping: Mutex<bool>,
    tick: Condvar,
}

/// The real watcher service.
pub struct IndexWatcherService {
    inner: Arc<Inner>,
    drain: Option<JoinHandle<()>>,
}

impl IndexWatcherService {
    pub fn new(repository: Arc<FsIndexRepository>) -> IndexWatcherService {
        let inner = Arc::new(Inner {
            applier: ChangeApplier::new(repository.clone()),
            repository,
            watchers: Mutex::new(HashMap::new()),
            pending: Mutex::new(Vec::new()),
            draining: Mutex::new(false),
            stopping: Mutex::new(false),
            tick: Condvar::new(),
        });
        let drain = {
            let inner = inner.clone();
            std::thread::Builder::new()
                .name("index-watcher-drain".into())
                .spawn(move || {
                    loop {
                        {
                            let stopping = inner.stopping.lock().unwrap_or_else(|p| p.into_inner());
                            let (stopping, _) = inner
                                .tick
                                .wait_timeout(stopping, DRAIN_INTERVAL)
                                .unwrap_or_else(|p| p.into_inner());
                            if *stopping {
                                return;
                            }
                        }
                        inner.drain();
                    }
                })
                .ok()
        };
        IndexWatcherService { inner, drain }
    }

    /// Applies a batch of watcher events to the index directly, the way the drain does.
    pub fn apply(&self, changes: &[FsChange]) {
        self.inner.applier.apply(changes);
    }
}

impl IndexWatcher for IndexWatcherService {
    fn is_watching(&self, root_key: &str) -> bool {
        let mut watchers = self
            .inner
            .watchers
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        match watchers.get_mut(root_key) {
            Some(entry) => {
                entry.last_touched = Instant::now();
                true
            }
            None => false,
        }
    }

    fn watch(&self, root_key: &str, display_path: &str) {
        if display_path.starts_with(r"\\") {
            return;
        }
        let mut watchers = self
            .inner
            .watchers
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(existing) = watchers.get_mut(root_key) {
            existing.last_touched = Instant::now();
            return;
        }
        if watchers.len() >= MAX_WATCHERS
            && let Some(lru) = watchers
                .iter()
                .min_by_key(|(_, e)| e.last_touched)
                .map(|(k, _)| k.clone())
            && let Some(entry) = watchers.remove(&lru)
        {
            entry.stop_and_join();
        }

        // Root disappeared or is unwatchable: the freshness check will fail and re-crawl.
        let Ok(directory) = OpenOptions::new()
            .access_mode(FILE_LIST_DIRECTORY)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
            .open(display_path)
        else {
            return;
        };

        let stop = Arc::new(Event::manual_reset());
        let thread = {
            let stop = stop.clone();
            let inner = Arc::downgrade(&self.inner);
            let root_key = root_key.to_string();
            let display_path = display_path.to_string();
            std::thread::Builder::new()
                .name("index-watcher".into())
                .spawn(move || watch_loop(directory, stop, inner, root_key, display_path))
                .ok()
        };
        watchers.insert(
            root_key.to_string(),
            WatcherEntry {
                stop,
                thread,
                last_touched: Instant::now(),
            },
        );
    }
}

impl Drop for IndexWatcherService {
    fn drop(&mut self) {
        *self
            .inner
            .stopping
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = true;
        self.inner.tick.notify_all();
        if let Some(drain) = self.drain.take() {
            let _ = drain.join();
        }
        let entries: Vec<WatcherEntry> = self
            .inner
            .watchers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain()
            .map(|(_, e)| e)
            .collect();
        for entry in entries {
            entry.stop_and_join();
        }
    }
}

impl Inner {
    fn drain(&self) {
        {
            let mut draining = self.draining.lock().unwrap_or_else(|p| p.into_inner());
            if *draining {
                return;
            }
            *draining = true;
        }
        let changes: Vec<FsChange> =
            std::mem::take(&mut *self.pending.lock().unwrap_or_else(|p| p.into_inner()));
        if !changes.is_empty() {
            self.applier.apply(&changes);
        }
        *self.draining.lock().unwrap_or_else(|p| p.into_inner()) = false;
    }

    /// Buffer overflow or other watcher failure: events were lost, so the index for this root can
    /// no longer be trusted. Drop the watcher and mark the root stale; the next search serves
    /// cached results instantly and re-crawls in the background. Called from the watcher's own
    /// thread, so the entry is dropped without a join.
    fn on_watcher_error(&self, root_key: &str) {
        let removed = self
            .watchers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(root_key);
        if let Some(mut entry) = removed {
            entry.stop.set();
            entry.thread.take(); // this thread; it is exiting
        }
        // Best-effort; the root stays "fresh" only until its watcher check fails.
        let _ = self.repository.mark_root_stale(root_key);
    }
}

/// One root's `ReadDirectoryChangesW` loop. Overlapped, so the stop event can interrupt a wait.
fn watch_loop(
    directory: std::fs::File,
    stop: Arc<Event>,
    inner: Weak<Inner>,
    root_key: String,
    display_path: String,
) {
    let handle = directory.as_raw_handle() as HANDLE;
    let io_done = Event::manual_reset();
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let filter = FILE_NOTIFY_CHANGE_FILE_NAME
        | FILE_NOTIFY_CHANGE_DIR_NAME
        | FILE_NOTIFY_CHANGE_LAST_WRITE
        | FILE_NOTIFY_CHANGE_SIZE;
    let prefix = display_path.trim_end_matches('\\').to_string();
    let mut pending_old: Option<String> = None;

    loop {
        // SAFETY: a zeroed OVERLAPPED with our event is what the call expects.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = io_done.0;
        // SAFETY: the buffer and the overlapped outlive the call and the wait below.
        let issued = unsafe {
            ReadDirectoryChangesW(
                handle,
                buffer.as_mut_ptr().cast(),
                BUFFER_SIZE as u32,
                1,
                filter,
                std::ptr::null_mut(),
                &mut overlapped,
                None,
            )
        };
        if issued == 0 {
            if let Some(inner) = inner.upgrade() {
                inner.on_watcher_error(&root_key);
            }
            return;
        }

        let handles = [stop.0, io_done.0];
        // SAFETY: two valid handles, wait for any.
        let woke = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        if woke == WAIT_OBJECT_0 {
            // SAFETY: cancelling the read issued above, then waiting for it to drain.
            unsafe {
                CancelIoEx(handle, &overlapped);
                let mut bytes = 0u32;
                GetOverlappedResult(handle, &overlapped, &mut bytes, 1);
            }
            return;
        }

        let mut bytes = 0u32;
        // SAFETY: the operation has completed; this only reads its result.
        let ok = unsafe { GetOverlappedResult(handle, &overlapped, &mut bytes, 0) };
        if ok == 0 || bytes == 0 {
            // Zero bytes means the buffer overflowed and the records were lost.
            if let Some(inner) = inner.upgrade() {
                inner.on_watcher_error(&root_key);
            }
            return;
        }

        let Some(inner) = inner.upgrade() else {
            return;
        };
        let changes = parse_notifications(
            &buffer[..bytes as usize],
            &prefix,
            &root_key,
            &mut pending_old,
        );
        if !changes.is_empty() {
            inner
                .pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .extend(changes);
        }
    }
}

/// Walks the `FILE_NOTIFY_INFORMATION` records in a completed buffer. A rename arrives as two
/// records, old name then new; the pair becomes one `Renamed` change.
fn parse_notifications(
    buffer: &[u8],
    prefix: &str,
    root_key: &str,
    pending_old: &mut Option<String>,
) -> Vec<FsChange> {
    let mut changes = Vec::new();
    let mut offset = 0usize;
    loop {
        if offset + 12 > buffer.len() {
            break;
        }
        let read_u32 = |at: usize| {
            u32::from_le_bytes([buffer[at], buffer[at + 1], buffer[at + 2], buffer[at + 3]])
        };
        let next = read_u32(offset) as usize;
        let action = read_u32(offset + 4);
        let name_len = read_u32(offset + 8) as usize;
        let name_start = offset + 12;
        if name_start + name_len > buffer.len() {
            break;
        }
        let units: Vec<u16> = buffer[name_start..name_start + name_len]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes(*p))
            .collect();
        let full_path = format!("{prefix}\\{}", String::from_utf16_lossy(&units));

        let change = |kind: FsChangeKind, old: Option<String>| FsChange {
            root_key: root_key.to_string(),
            kind,
            full_path: full_path.clone(),
            old_full_path: old,
        };
        match action {
            FILE_ACTION_ADDED => changes.push(change(FsChangeKind::Created, None)),
            FILE_ACTION_MODIFIED => changes.push(change(FsChangeKind::Changed, None)),
            FILE_ACTION_REMOVED => changes.push(change(FsChangeKind::Deleted, None)),
            FILE_ACTION_RENAMED_OLD_NAME => *pending_old = Some(full_path.clone()),
            FILE_ACTION_RENAMED_NEW_NAME => {
                let old = pending_old.take();
                changes.push(change(FsChangeKind::Renamed, old));
            }
            _ => {}
        }

        if next == 0 {
            break;
        }
        offset += next;
    }
    changes
}
