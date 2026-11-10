//! Everything an elevated helper knows about the app that started it: whether the pipe it connected
//! to really belongs to that process, and when that process goes away. Spec: Core/Ipc/PipeOwner.cs.
//!
//! Shared by both helpers rather than copied into each, because it is the check every one of them
//! has to get right and there should be one of it to audit. The pipe's DACL already admits only this
//! user, and nothing between two processes of one user is a security boundary. What `owns_pipe` adds
//! is different: it establishes that the endpoint is the process that launched us, rather than
//! another of the user's own that guessed or raced for the name. A helper that attached to the wrong
//! server would work on behalf of something that never asked.
//!
//! `watch_for_exit` is the belt to the pipe's braces *for the file-operation helper*, which serves
//! one request for one app and must not outlive it. Losing the pipe is that helper's primary signal
//! and covers a crash, since the kernel breaks it however the peer ends; this covers the exotic case
//! where a duplicated handle keeps the pipe open after the app is gone. **The index helper
//! deliberately does not use it**: it is meant to outlive every app, so losing a pipe there ends a
//! session and nothing more.

use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{
    INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    WaitForSingleObject,
};

use super::OwnedHandle;

const SYNCHRONIZE: u32 = 0x0010_0000;

pub struct PipeOwner;

impl PipeOwner {
    /// True when `pipe`'s server end belongs to `expected_process_id`.
    pub fn owns_pipe<H: AsRawHandle + ?Sized>(pipe: &H, expected_process_id: u32) -> bool {
        Self::server_process_id(pipe) == Some(expected_process_id)
    }

    /// The process id listening on `pipe` (the client end of a named pipe), for a helper that has
    /// no expected value to compare against and must ask what is there instead.
    pub fn server_process_id<H: AsRawHandle + ?Sized>(pipe: &H) -> Option<u32> {
        let handle: HANDLE = pipe.as_raw_handle().cast();
        let mut id = 0u32;
        if unsafe { GetNamedPipeServerProcessId(handle, &mut id) } == 0 {
            return None;
        }
        Some(id)
    }

    /// The full path of a running process's executable, or None when it cannot be read.
    ///
    /// Used by both elevated helpers to check that the process on the other end of the pipe is the
    /// copy of BertBrowser sitting beside them. A high-integrity process may open a
    /// medium-integrity one for `PROCESS_QUERY_LIMITED_INFORMATION`, so this direction works where
    /// the reverse would not. **It is a coherence check, not a boundary**: a program running as this
    /// user could copy the command line, or simply raise its own prompt. What protects the user is
    /// the UAC dialog naming the helper. This rules out confusion (a stale build, a raced name) and
    /// nothing more.
    pub fn image_path_of(process_id: u32) -> Option<String> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if handle.is_null() {
            return None;
        }
        let handle = OwnedHandle(handle);

        let mut name = [0u16; 1024];
        let mut size = name.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle.0, 0, name.as_mut_ptr(), &mut size) } == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&name[..size as usize]))
    }

    /// Runs `on_exit` on a background thread once the given process exits. Runs it at once if the
    /// process cannot be opened, which on this path means it has already gone.
    pub fn watch_for_exit<F>(process_id: u32, on_exit: F) -> std::io::Result<()>
    where
        F: FnOnce() + Send + 'static,
    {
        std::thread::Builder::new()
            .name("bertbrowser parent watchdog".to_string())
            .spawn(move || {
                let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, process_id) };
                if handle.is_null() {
                    on_exit();
                    return;
                }
                let handle = OwnedHandle(handle);
                if unsafe { WaitForSingleObject(handle.0, INFINITE) } == WAIT_OBJECT_0 {
                    on_exit();
                }
            })
            .map(|_| ())
    }
}
