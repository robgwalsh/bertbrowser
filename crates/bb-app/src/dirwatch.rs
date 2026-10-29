//! Tells a tab that its folder changed under it. One thread per watched folder, waiting on a change
//! notification and a stop event; a burst of changes is one callback, `debounce` after the last.
//! The notification carries no detail on purpose: the tab re-lists and diffs, which is what makes an
//! overflowed buffer a non-event.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FindCloseChangeNotification,
    FindFirstChangeNotificationW, FindNextChangeNotification,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects,
};

pub struct DirWatcher {
    stop: usize,
    thread: Option<JoinHandle<()>>,
}

impl DirWatcher {
    /// Watches `path` (not its subfolders). None when the folder cannot be watched; the tab then
    /// simply has no live refresh.
    pub fn start(
        path: &str,
        debounce: Duration,
        on_quiet: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<DirWatcher> {
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: a NUL-terminated path; the handle is closed by the thread (or below on failure).
        let change = unsafe {
            FindFirstChangeNotificationW(
                wide.as_ptr(),
                0,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_ATTRIBUTES
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE,
            )
        };
        if change == INVALID_HANDLE_VALUE || change.is_null() {
            return None;
        }
        // SAFETY: manual-reset, initially unsignalled, unnamed.
        let stop = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if stop.is_null() {
            // SAFETY: `change` was just opened.
            unsafe { FindCloseChangeNotification(change) };
            return None;
        }
        let (change_bits, stop_bits) = (change as usize, stop as usize);
        let thread = std::thread::Builder::new()
            .name("bb-watch".into())
            .spawn(move || watch_loop(change_bits, stop_bits, debounce, on_quiet))
            .ok();
        let Some(thread) = thread else {
            // SAFETY: both handles are ours and unused.
            unsafe {
                FindCloseChangeNotification(change);
                CloseHandle(stop);
            }
            return None;
        };
        Some(DirWatcher {
            stop: stop_bits,
            thread: Some(thread),
        })
    }
}

fn watch_loop(
    change_bits: usize,
    stop_bits: usize,
    debounce: Duration,
    on_quiet: Arc<dyn Fn() + Send + Sync>,
) {
    let change = change_bits as HANDLE;
    let stop = stop_bits as HANDLE;
    let handles = [change, stop];
    let mut deadline: Option<Instant> = None;
    loop {
        let timeout = match deadline {
            Some(d) => d.saturating_duration_since(Instant::now()).as_millis() as u32,
            None => INFINITE,
        };
        // SAFETY: two valid handles, owned by this thread until it returns.
        let r = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, timeout) };
        if r == WAIT_OBJECT_0 {
            // SAFETY: re-arms the notification.
            unsafe { FindNextChangeNotification(change) };
            deadline = Some(Instant::now() + debounce);
        } else if r == WAIT_TIMEOUT {
            deadline = None;
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_quiet()));
        } else {
            break;
        }
    }
    // SAFETY: the thread owns the notification handle.
    unsafe { FindCloseChangeNotification(change) };
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        // SAFETY: the stop event outlives the thread, which is joined before it is closed.
        unsafe { SetEvent(self.stop as HANDLE) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // SAFETY: the thread has ended.
        unsafe { CloseHandle(self.stop as HANDLE) };
    }
}
