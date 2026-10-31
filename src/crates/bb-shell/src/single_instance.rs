//! One copy of the app per user: a mutex decides who is first, and the first copy listens on a pipe
//! so a second launch can hand over its command line instead of starting a whole second app.
//! Spec: App/Services/SingleInstance.cs, Core/Ipc/InstanceEndpoint.cs.
//!
//! The mutex is `Local\`, not `Global\`: this is a per-session, per-user thing, and a name in this
//! namespace cannot be claimed from another session, which is exactly the property the pipe
//! namespace lacks and why the endpoint is named the way it is (random, published in a file only
//! this account can read). The first copy invents its endpoint and publishes it once it is
//! listening; a later one can only be told, and starts normally if it could not reach it (the first
//! may be mid-shutdown, and starting beats exiting and doing nothing at all).

use std::fs::File;
use std::io::Write;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bb_core::cli::{CommandLineRequest, NavigationRequest};
use bb_core::ipc::{InstanceEndpoint, LineReader, write_line};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, GENERIC_WRITE,
    GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex};

use crate::elevation::nonce;
use crate::pipe::{PeerCheck, PipeServer};

/// How long a second launch waits for the first to answer before starting normally.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// How long the listener gives a connected peer to say its piece. A client that connects and never
/// writes must not park the listener for the rest of the session.
const READ_BUDGET: Duration = Duration::from_secs(5);

const POLL: Duration = Duration::from_millis(200);

/// Where the first copy publishes the name of its endpoint.
pub fn endpoint_path(data_dir: &Path) -> PathBuf {
    data_dir.join("instance.pipe")
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct SingleInstance {
    mutex: HANDLE,
    is_first: bool,
    pipe_name: Option<String>,
    data_dir: PathBuf,
    stopping: Arc<AtomicBool>,
    listener: Mutex<Option<JoinHandle<()>>>,
}

// SAFETY: the mutex handle is a kernel object; which thread closes it does not matter.
unsafe impl Send for SingleInstance {}
unsafe impl Sync for SingleInstance {}

impl SingleInstance {
    /// For the current user.
    pub fn claim(data_dir: &Path) -> SingleInstance {
        let key = bb_core::ipc::IndexEndpoint::current_user_sid()
            .unwrap_or_else(|_| "default".to_string());
        Self::claim_as(&key, data_dir)
    }

    /// For a named user key (a test uses its own, so it cannot meet the real app).
    pub fn claim_as(user_key: &str, data_dir: &Path) -> SingleInstance {
        let name = wide(&format!(r"Local\BertBrowser.{user_key}"));
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let mutex = unsafe { CreateMutexW(null(), 1, name.as_ptr()) };
        // SAFETY: reading the thread's last error straight after the call.
        let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let is_first = !mutex.is_null() && !existed;

        let pipe_name = if is_first {
            Some(InstanceEndpoint::name(user_key, &nonce()))
        } else {
            published(data_dir, user_key)
        };
        SingleInstance {
            mutex,
            is_first,
            pipe_name,
            data_dir: data_dir.to_path_buf(),
            stopping: Arc::new(AtomicBool::new(false)),
            listener: Mutex::new(None),
        }
    }

    pub fn is_first(&self) -> bool {
        self.is_first
    }

    /// Starts answering other launches; `on_request` runs on the listener thread for each valid
    /// request that names something. The endpoint is published afterwards, so the window between
    /// "a launch can find the name" and "something is answering to it" is as small as it can be.
    pub fn start_listening(&self, on_request: impl Fn(CommandLineRequest) + Send + 'static) {
        let (true, Some(name)) = (self.is_first, self.pipe_name.clone()) else {
            return;
        };
        let Ok(sid) = bb_core::ipc::IndexEndpoint::current_user_sid() else {
            return;
        };
        let Ok(server) = PipeServer::create(&name, &sid, 4096, 4096) else {
            return;
        };
        let stopping = self.stopping.clone();
        let handle = std::thread::Builder::new()
            .name("bertbrowser single-instance listener".to_string())
            .spawn(move || listen(server, &stopping, on_request))
            .ok();
        *self.listener.lock().unwrap() = handle;
        publish(&self.data_dir, &name);
    }

    /// Passes `request` to the first copy. True when it was delivered, in which case this launch
    /// has nothing more to do; false when there was nobody reachable to hand it to.
    pub fn hand_off(&self, request: &CommandLineRequest) -> bool {
        let Some(name) = &self.pipe_name else {
            return false;
        };
        if self.is_first {
            return false;
        }
        let Ok(mut client) = connect(name, CONNECT_TIMEOUT) else {
            return false;
        };
        // Before the write, not after: this process holds the foreground rights (the shell just
        // started it from a double-click) and the copy doing the work does not. Without the
        // hand-over its window opens the folder behind whatever the user was looking at.
        crate::foreground::grant_to_server_of(&client);
        write_line(&mut client, &NavigationRequest::format(request)).is_ok()
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        if self.is_first {
            // Before waking the listener: from here on there is no endpoint to hand off to, and a
            // launch that reads a name we are about to stop answering has to wait out a connect
            // timeout to find that out.
            if let Some(name) = &self.pipe_name {
                unpublish(&self.data_dir, name);
            }
        }
        if let Some(listener) = self.listener.lock().unwrap().take() {
            let _ = listener.join();
        }
        if !self.mutex.is_null() {
            // SAFETY: releases (when held by this thread) and closes the handle this struct owns.
            unsafe {
                if self.is_first {
                    ReleaseMutex(self.mutex);
                }
                CloseHandle(self.mutex);
            }
        }
    }
}

fn listen(server: PipeServer, stopping: &AtomicBool, on_request: impl Fn(CommandLineRequest)) {
    while !stopping.load(Ordering::SeqCst) {
        let Some(session) = server.accept(PeerCheck::SameUser, POLL) else {
            continue;
        };
        // A watchdog hangs up on a peer that connects and says nothing.
        let (done, waiting) = mpsc::channel::<()>();
        let guard = session.clone();
        let watchdog = std::thread::spawn(move || {
            if waiting.recv_timeout(READ_BUDGET).is_err() {
                guard.close();
            }
        });

        struct Reading(crate::pipe::PipeSession);
        impl std::io::Read for Reading {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(buf)
            }
        }
        let line = LineReader::new(Reading(session.clone()), NavigationRequest::MAX_LINE_LENGTH)
            .read_line();
        let _ = done.send(());
        let _ = watchdog.join();
        session.close();

        // One bad connection must not end the listener, or the app quietly stops answering for the
        // rest of the session. A request with no targets still counts: a bare launch (the Start
        // menu, Win+E) is asking for the window, and the receiver is what raises it.
        if let Ok(Some(line)) = line
            && let Some(request) = NavigationRequest::try_parse(&line)
        {
            on_request(request);
        }
    }
}

/// Connects to `\\.\pipe\<name>` as a writer, waiting for the pipe to exist and to have room.
fn connect(name: &str, timeout: Duration) -> std::io::Result<File> {
    let path = wide(&format!(r"\\.\pipe\{name}"));
    let deadline = Instant::now() + timeout;
    loop {
        // SAFETY: `path` is NUL-terminated and outlives the call. Identification: the server may
        // learn who this is and nothing more.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            // SAFETY: a handle just opened and owned here.
            return Ok(File::from(unsafe {
                OwnedHandle::from_raw_handle(handle.cast())
            }));
        }
        // SAFETY: reading the thread's last error.
        let error = unsafe { GetLastError() };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || (error != ERROR_FILE_NOT_FOUND && error != ERROR_PIPE_BUSY) {
            return Err(if remaining.is_zero() {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out connecting")
            } else {
                std::io::Error::from_raw_os_error(error as i32)
            });
        }
        if error == ERROR_PIPE_BUSY {
            // SAFETY: as above.
            unsafe { WaitNamedPipeW(path.as_ptr(), remaining.as_millis().min(100) as u32) };
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

// ---- endpoint discovery ---------------------------------------------------------------------

fn publish(data_dir: &Path, name: &str) {
    // Best effort: a launch that cannot find the name just starts normally.
    let _ = std::fs::create_dir_all(data_dir);
    if let Ok(mut file) = File::create(endpoint_path(data_dir)) {
        let _ = file.write_all(name.as_bytes());
    }
}

fn published(data_dir: &Path, user_key: &str) -> Option<String> {
    let text = std::fs::read_to_string(endpoint_path(data_dir)).ok()?;
    let name = text.trim();
    InstanceEndpoint::is_acceptable(name, user_key).then(|| name.to_string())
}

/// Removes the published name, but only if it is still ours: a newer first copy may have replaced it.
fn unpublish(data_dir: &Path, name: &str) {
    if std::fs::read_to_string(endpoint_path(data_dir)).is_ok_and(|t| t.trim() == name) {
        let _ = std::fs::remove_file(endpoint_path(data_dir));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_cli_helpers::*;

    mod bb_cli_helpers {
        use bb_core::cli::{CommandLine, CommandLineRequest};
        pub fn request(paths: &[&str]) -> CommandLineRequest {
            CommandLine::parse(paths)
        }
    }

    fn rig(tag: &str) -> (String, PathBuf) {
        let key = format!("S-1-5-21-424242-{}-{tag}", std::process::id());
        let dir = bb_test_support::unique_temp_path(&format!("single-{tag}"), "");
        std::fs::create_dir_all(&dir).unwrap();
        (key, dir)
    }

    #[test]
    fn the_first_claim_wins_and_the_second_finds_the_published_name() {
        let (key, dir) = rig("claims");
        let first = SingleInstance::claim_as(&key, &dir);
        assert!(first.is_first());
        first.start_listening(|_| {});
        let second = SingleInstance::claim_as(&key, &dir);
        assert!(!second.is_first());
        assert_eq!(first.pipe_name, second.pipe_name);
        drop(second);
        drop(first);
        assert!(
            !endpoint_path(&dir).exists(),
            "the name is withdrawn on the way out"
        );
    }

    #[test]
    fn a_second_launch_hands_its_targets_over_and_the_first_receives_them() {
        let (key, dir) = rig("handoff");
        let first = SingleInstance::claim_as(&key, &dir);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        first.start_listening(move |r| {
            let _ = tx.lock().unwrap().send(r);
        });
        let second = SingleInstance::claim_as(&key, &dir);
        let asked = request(&["--new-tab", r"C:\Windows"]);
        assert!(second.hand_off(&asked));
        let got = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the first copy was told");
        assert_eq!(asked.targets, got.targets);
        assert_eq!(asked.mode, got.mode);
    }

    #[test]
    fn a_bare_second_launch_still_reaches_the_first_so_its_window_can_come_forward() {
        let (key, dir) = rig("bare");
        let first = SingleInstance::claim_as(&key, &dir);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        first.start_listening(move |r| {
            let _ = tx.lock().unwrap().send(r);
        });
        let second = SingleInstance::claim_as(&key, &dir);
        assert!(second.hand_off(&request(&[])));
        let got = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the first copy was told");
        assert!(!got.has_targets());
    }

    #[test]
    fn with_nobody_to_hand_to_a_launch_starts_normally() {
        let (key, dir) = rig("nobody");
        let first = SingleInstance::claim_as(&key, &dir);
        // Claimed but never listening: nothing is published, so a second has no name.
        let second = SingleInstance::claim_as(&key, &dir);
        assert!(!second.is_first());
        assert!(!second.hand_off(&request(&[r"C:\Windows"])));
        drop(first);
    }

    #[test]
    fn a_published_name_that_is_not_ours_is_not_trusted() {
        let (key, dir) = rig("forged");
        std::fs::write(endpoint_path(&dir), r"..\evil").unwrap();
        assert_eq!(None, published(&dir, &key));
        std::fs::write(
            endpoint_path(&dir),
            format!("BertBrowser.{key}.{}", "0".repeat(31)),
        )
        .unwrap();
        assert_eq!(None, published(&dir, &key), "a nonce of the wrong length");
    }

    #[test]
    fn a_name_is_only_withdrawn_by_the_copy_that_published_it() {
        let (_, dir) = rig("unpublish");
        publish(&dir, "BertBrowser.X.AAAA");
        unpublish(&dir, "BertBrowser.X.BBBB");
        assert!(endpoint_path(&dir).exists());
        unpublish(&dir, "BertBrowser.X.AAAA");
        assert!(!endpoint_path(&dir).exists());
    }

    #[test]
    fn a_peer_that_connects_and_says_nothing_does_not_stop_the_listener() {
        let (key, dir) = rig("silent");
        let first = SingleInstance::claim_as(&key, &dir);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        first.start_listening(move |r| {
            let _ = tx.lock().unwrap().send(r);
        });
        let name = first.pipe_name.clone().unwrap();
        let silent = connect(&name, Duration::from_secs(2)).unwrap();
        // Hung up on after the read budget; a real request behind it is still answered.
        drop(silent);
        let second = SingleInstance::claim_as(&key, &dir);
        assert!(second.hand_off(&request(&[r"C:\Windows"])));
        assert!(rx.recv_timeout(Duration::from_secs(10)).is_ok());
    }
}
