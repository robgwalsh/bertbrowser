//! The helper's end of the pipe: an overlapped client handle that one thread can read while another
//! writes. This is load-bearing rather than a performance choice: on a synchronous handle Windows
//! serialises I/O, so the read loop waiting for a `Cancel` would block the worker's progress writes
//! on the same handle (the index helper's own note, written after exactly that deadlock).
//! Spec: src/BertBrowser.Elevator/Program.cs (`NamedPipeClientStream`, `PipeOptions.Asynchronous`).

use std::io::{self, Read, Write};
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bb_core::elevation::ElevationStream;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_NO_DATA,
    ERROR_OPERATION_ABORTED, ERROR_PIPE_BUSY, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING, ReadFile,
    SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};

struct Shared {
    handle: HANDLE,
    closed: AtomicBool,
}

// SAFETY: a pipe handle is a kernel object; overlapped I/O on it from several threads is its
// purpose, and every call here uses its own OVERLAPPED and event.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Drop for Shared {
    fn drop(&mut self) {
        // SAFETY: the last reference to a handle this struct owns.
        unsafe { CloseHandle(self.handle) };
    }
}

impl AsRawHandle for Shared {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.cast()
    }
}

#[derive(Clone)]
pub struct PipeStream {
    shared: Arc<Shared>,
}

impl AsRawHandle for PipeStream {
    fn as_raw_handle(&self) -> RawHandle {
        self.shared.as_raw_handle()
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

impl PipeStream {
    /// Connects to `\\.\pipe\<name>`, waiting for it to exist and to have room, up to `timeout`.
    /// The impersonation level is `Identification`: the server may learn who this is and nothing
    /// more.
    pub fn connect(name: &str, timeout: Duration) -> io::Result<PipeStream> {
        let path = wide(&format!(r"\\.\pipe\{name}"));
        let deadline = Instant::now() + timeout;
        loop {
            // SAFETY: `path` is NUL-terminated and outlives the call.
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_NONE,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Ok(PipeStream {
                    shared: Arc::new(Shared {
                        handle,
                        closed: AtomicBool::new(false),
                    }),
                });
            }
            // SAFETY: reading the thread's last error.
            let error = unsafe { GetLastError() };
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || (error != ERROR_FILE_NOT_FOUND && error != ERROR_PIPE_BUSY) {
                return Err(if remaining.is_zero() {
                    io::Error::new(io::ErrorKind::TimedOut, "timed out connecting to the pipe")
                } else {
                    io::Error::from_raw_os_error(error as i32)
                });
            }
            if error == ERROR_PIPE_BUSY {
                // SAFETY: as above.
                unsafe { WaitNamedPipeW(path.as_ptr(), remaining.as_millis().min(1000) as u32) };
            } else {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }

    /// Runs one overlapped transfer to completion on its own event.
    fn transfer(&self, start: impl FnOnce(HANDLE, *mut OVERLAPPED) -> i32) -> Result<usize, u32> {
        // SAFETY: a manual-reset, initially unsignalled, unnamed event.
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event.is_null() {
            // SAFETY: reading the thread's last error.
            return Err(unsafe { GetLastError() });
        }
        // SAFETY: OVERLAPPED is plain old data; zero is its documented initial state.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = event;

        let handle = self.shared.handle;
        let mut result = if start(handle, &mut overlapped) != 0 {
            Ok(())
        } else {
            // SAFETY: reading the thread's last error.
            match unsafe { GetLastError() } {
                ERROR_IO_PENDING => Ok(()),
                other => Err(other),
            }
        };

        let mut transferred = 0u32;
        if result.is_ok() {
            // SAFETY: `overlapped` and its event live until the wait and result have completed.
            unsafe {
                if WaitForSingleObject(event, INFINITE) != WAIT_OBJECT_0
                    || GetOverlappedResult(handle, &overlapped, &mut transferred, 0) == 0
                {
                    result = Err(GetLastError());
                }
            }
        }
        // SAFETY: closing the event created above.
        unsafe { CloseHandle(event) };
        result.map(|()| transferred as usize)
    }
}

fn to_io(error: u32) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

impl Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.shared.closed.load(Ordering::SeqCst) {
            return Ok(0);
        }
        let length = buf.len().min(u32::MAX as usize) as u32;
        let pointer = buf.as_mut_ptr();
        match self.transfer(|handle, overlapped| {
            // SAFETY: `buf` outlives the transfer, which `transfer` waits for.
            unsafe { ReadFile(handle, pointer.cast(), length, null_mut(), overlapped) }
        }) {
            Ok(n) => Ok(n),
            // The other end went away, or this end was shut down: end of stream either way.
            Err(ERROR_BROKEN_PIPE | ERROR_OPERATION_ABORTED | ERROR_NO_DATA) => Ok(0),
            Err(other) => Err(to_io(other)),
        }
    }
}

impl Write for PipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.shared.closed.load(Ordering::SeqCst) {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        let length = buf.len().min(u32::MAX as usize) as u32;
        let pointer = buf.as_ptr();
        self.transfer(|handle, overlapped| {
            // SAFETY: `buf` outlives the transfer, which `transfer` waits for.
            unsafe { WriteFile(handle, pointer.cast(), length, null_mut(), overlapped) }
        })
        .map_err(to_io)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl ElevationStream for PipeStream {
    fn split(&self) -> io::Result<Box<dyn ElevationStream>> {
        Ok(Box::new(self.clone()))
    }

    fn shutdown(&self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        // SAFETY: cancels every pending request on a handle this struct keeps open.
        unsafe { CancelIoEx(self.shared.handle, null()) };
    }
}
