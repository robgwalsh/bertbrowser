//! The other end of a pick request: the pipe another program made for the answer, opened as its
//! client. One line is ever written to it - the path the user chose - or nothing at all, which is
//! how backing out is said.
//!
//! The name comes from outside and is validated before it gets here (`PickRequest`: a plain name,
//! no separators), so the only thing this can open is `\\.\pipe\<that name>` on this machine. It
//! is opened with identification-level impersonation: the server may learn who answered and
//! nothing more.

use std::fs::File;
use std::io::Write;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use bb_core::cli::PickRequest;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY,
    ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, GetLastError, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::{PeekNamedPipe, WaitNamedPipeW};

/// How long to wait for the asker's pipe. It made the pipe before it asked, so this is normally no
/// wait at all; the budget is for an asker that is slow to start listening, and it is short
/// because the request may be taken on the thread the window answers on.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(750);

pub struct ReplyPipe {
    pipe: File,
    /// Opened for reading as well, which is what lets `is_closed` look without writing.
    can_peek: bool,
}

impl ReplyPipe {
    /// Connects to the asker's pipe. `Err` when the name is not a plain pipe name or nothing is
    /// listening on it.
    pub fn connect(name: &str) -> std::io::Result<ReplyPipe> {
        if !PickRequest::is_acceptable_pipe_name(name) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "not a plain pipe name",
            ));
        }
        let path: Vec<u16> = format!(r"\\.\pipe\{name}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        // Reading is asked for only so the pipe can be watched; a server that made a one-way pipe
        // refuses it, and is answered through a write-only handle instead.
        let mut access = GENERIC_READ | GENERIC_WRITE;
        loop {
            // SAFETY: `path` is NUL-terminated and outlives the call.
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    access,
                    0,
                    null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                // SAFETY: a handle just opened and owned here.
                let pipe = File::from(unsafe { OwnedHandle::from_raw_handle(handle.cast()) });
                return Ok(ReplyPipe {
                    pipe,
                    can_peek: access & GENERIC_READ != 0,
                });
            }
            // SAFETY: reading the thread's last error.
            let error = unsafe { GetLastError() };
            if error == ERROR_ACCESS_DENIED && access & GENERIC_READ != 0 {
                access = GENERIC_WRITE;
                continue;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timed out connecting",
                ));
            }
            if error == ERROR_PIPE_BUSY {
                // SAFETY: as above.
                unsafe { WaitNamedPipeW(path.as_ptr(), remaining.as_millis().min(100) as u32) };
            } else if error == ERROR_FILE_NOT_FOUND {
                std::thread::sleep(Duration::from_millis(10));
            } else {
                return Err(std::io::Error::from_raw_os_error(error as i32));
            }
        }
    }

    /// Writes the answer: one line, UTF-8. The asker is given the right to come forward first,
    /// because the user has just finished here and is expecting to be back there.
    pub fn send_line(&mut self, text: &str) -> std::io::Result<()> {
        crate::foreground::grant_to_server_of(&self.pipe);
        self.pipe.write_all(text.as_bytes())?;
        self.pipe.write_all(b"\n")?;
        self.pipe.flush()
    }

    /// Whether the asker has gone, or stopped waiting. False when that cannot be told.
    pub fn is_closed(&self) -> bool {
        if !self.can_peek {
            return false;
        }
        let mut available = 0u32;
        // SAFETY: the handle is owned by `self.pipe`; no buffer is passed, only the count out.
        let ok = unsafe {
            PeekNamedPipe(
                self.pipe.as_raw_handle().cast(),
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        };
        if ok != 0 {
            return false;
        }
        // SAFETY: reading the thread's last error.
        let error = unsafe { GetLastError() };
        error == ERROR_BROKEN_PIPE || error == ERROR_PIPE_NOT_CONNECTED
    }
}
