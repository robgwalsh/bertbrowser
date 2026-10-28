//! The helper's end of the index pipe: a client that dials the app's well-known per-user endpoint.
//! Spec: the `NamedPipeClientStream` setup in src/BertBrowser.Indexer/Program.cs.
//!
//! Overlapped I/O, and that is load-bearing rather than a performance choice: Windows serialises
//! I/O on a non-overlapped handle, so the main thread parked in a blocking read would block the
//! volume threads' writes on the same handle. Each call here still blocks its own caller; only the
//! handle's mode changes, so the two directions proceed independently.
//!
//! `SECURITY_IDENTIFICATION`: the app's `PeerIntegrity` impersonates this client to read its
//! token's label, which identification permits and nothing more.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::ptr::null_mut;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_PIPE_BUSY,
    GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, ReadFile, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
use windows_sys::Win32::System::Threading::CreateEventW;

use bb_core::mft::IndexStream;

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;

pub struct PipeClient {
    handle: OwnedHandle,
}

// A kernel handle is process-wide; the host reads on one thread and the volume threads write.
unsafe impl Send for PipeClient {}
unsafe impl Sync for PipeClient {}

impl PipeClient {
    /// Tries once to connect to `\\.\pipe\<endpoint>`, waiting up to `timeout` on a busy pipe.
    /// None when no app is listening.
    pub fn connect(endpoint: &str, timeout: Duration) -> Option<PipeClient> {
        let name: Vec<u16> = format!("\\\\.\\pipe\\{endpoint}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        loop {
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE && !handle.is_null() {
                return Some(PipeClient {
                    handle: unsafe { OwnedHandle::from_raw_handle(handle.cast()) },
                });
            }
            match unsafe { GetLastError() } {
                // Every instance is busy: wait for one to free up, then try again.
                ERROR_PIPE_BUSY => {
                    if unsafe { WaitNamedPipeW(name.as_ptr(), timeout.as_millis() as u32) } == 0 {
                        return None;
                    }
                }
                ERROR_FILE_NOT_FOUND => return None,
                _ => return None,
            }
        }
    }

    fn raw(&self) -> HANDLE {
        self.handle.as_raw_handle().cast()
    }

    /// One overlapped operation, waited to completion on its own event.
    fn complete(&self, started: i32, overlapped: &mut OVERLAPPED) -> io::Result<usize> {
        if started == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
        }
        let mut transferred = 0u32;
        let ok = unsafe { GetOverlappedResult(self.raw(), overlapped, &mut transferred, 1) };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            // The app hung up: end of stream, not an error.
            if error == ERROR_BROKEN_PIPE {
                return Ok(0);
            }
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        Ok(transferred as usize)
    }
}

/// An auto-reset event for one overlapped call, closed on drop.
struct Event(HANDLE);

impl Event {
    fn new() -> io::Result<Event> {
        let handle = unsafe { CreateEventW(null_mut(), 0, 0, null_mut()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Event(handle))
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn overlapped(event: &Event) -> OVERLAPPED {
    let mut o: OVERLAPPED = unsafe { std::mem::zeroed() };
    o.hEvent = event.0;
    o
}

impl IndexStream for PipeClient {
    fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let event = Event::new()?;
        let mut o = overlapped(&event);
        let started = unsafe {
            ReadFile(
                self.raw(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                null_mut(),
                &mut o,
            )
        };
        self.complete(started, &mut o)
    }

    fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        let mut written = 0usize;
        while written < bytes.len() {
            let event = Event::new()?;
            let mut o = overlapped(&event);
            let rest = &bytes[written..];
            let started = unsafe {
                WriteFile(
                    self.raw(),
                    rest.as_ptr(),
                    rest.len() as u32,
                    null_mut(),
                    &mut o,
                )
            };
            let n = self.complete(started, &mut o)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "The pipe has been closed.",
                ));
            }
            written += n;
        }
        Ok(())
    }

    fn close(&self) {
        unsafe { CancelIoEx(self.raw(), std::ptr::null()) };
    }
}

impl AsRawHandle for PipeClient {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}
