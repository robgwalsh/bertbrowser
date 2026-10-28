//! The listening end of a pipe in this unprivileged process, shared by the index helper's endpoint
//! and the file-operation helper's. Spec: App/Services/Indexing/NamedPipeIndexTransport.cs,
//! App/Services/Elevation/NamedPipeElevationTransport.cs.
//!
//! **The app listens and the helper connects, and that is not arbitrary.** A named pipe created by a
//! high-integrity process carries a High mandatory label, and mandatory policy forbids writing up, so
//! this medium-integrity process could not write to a pipe its own helper had created. Creating the
//! pipe here makes the helper's connection a write-down, which is always allowed.
//!
//! Three checks on the peer, answering different questions. The DACL and the account comparison
//! establish it is this user. When the app launched the helper itself, `GetNamedPipeClientProcessId`
//! establishes it is the process that was started. When it merely attached to a helper already
//! running there is no such id, so `PeerIntegrity` establishes that whatever connected at least
//! holds an administrator token, which an ordinary program of this user does not. None of these is a
//! security boundary (nothing between two processes of one user is) but together they stop the app
//! mistaking something else for a helper.
//!
//! **Overlapped, with real buffers, and both are load-bearing.** A zero-size buffer holds nothing,
//! so on a duplex pipe where both ends greet each other every write blocks until the peer reads and
//! the two deadlock; and Windows serialises I/O on a non-overlapped handle, so a thread parked in a
//! read would block the other thread's writes on the same handle.

use std::io;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bb_core::ipc::PeerIntegrity;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, RevertToSelf, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    ImpersonateNamedPipeClient, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentThread, INFINITE, OpenThreadToken, WaitForSingleObject,
};

/// The rights the owning user's helper gets on the pipe: read and write, which for a pipe includes
/// creating an instance. `FILE_GENERIC_READ | FILE_GENERIC_WRITE`.
const USER_RIGHTS: u32 = 0x0012_019F;

/// The security descriptor of a pipe: only this user may even open it.
pub fn sddl_for(user_sid: &str) -> String {
    format!("D:(A;;0x{USER_RIGHTS:08X};;;{user_sid})")
}

/// What it takes for a connected peer to be accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerCheck {
    /// The app started the helper and knows its process id.
    Launched(u32),
    /// The app attached to a helper already running: it must hold an administrator token.
    Elevated,
    /// Any process of this user: the single-instance hand-off, where the peer is another copy of
    /// the app running as the same account and nothing more can be asked of it.
    SameUser,
}

/// One kernel pipe handle, closed when the last reference goes.
pub struct PipeHandle {
    handle: HANDLE,
}

// SAFETY: a pipe handle is a kernel object; overlapped I/O on it from several threads is its
// purpose, and every call uses its own OVERLAPPED and event.
unsafe impl Send for PipeHandle {}
unsafe impl Sync for PipeHandle {}

impl Drop for PipeHandle {
    fn drop(&mut self) {
        // SAFETY: the last reference to a handle this struct owns.
        unsafe {
            DisconnectNamedPipe(self.handle);
            CloseHandle(self.handle);
        }
    }
}

impl std::os::windows::io::AsRawHandle for PipeHandle {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.handle.cast()
    }
}

fn to_io(error: u32) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Runs one overlapped transfer to completion on its own event.
fn transfer(
    handle: HANDLE,
    start: impl FnOnce(HANDLE, *mut OVERLAPPED) -> i32,
) -> Result<usize, u32> {
    // SAFETY: a manual-reset, initially unsignalled, unnamed event.
    let event = unsafe { CreateEventW(null(), 1, 0, null()) };
    if event.is_null() {
        // SAFETY: reading the thread's last error.
        return Err(unsafe { GetLastError() });
    }
    // SAFETY: OVERLAPPED is plain old data; zero is its documented initial state.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    overlapped.hEvent = event;

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

/// One connection's view of a server pipe. Closing it hangs up on this helper without destroying the
/// endpoint the next one needs; the shared flag is what keeps a stale session from reading the next
/// session's bytes.
#[derive(Clone)]
pub struct PipeSession {
    pipe: Arc<PipeHandle>,
    closed: Arc<AtomicBool>,
}

impl PipeSession {
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.closed.load(Ordering::SeqCst) {
            return Ok(0);
        }
        let length = buf.len().min(u32::MAX as usize) as u32;
        let pointer = buf.as_mut_ptr();
        match transfer(self.pipe.handle, |handle, overlapped| {
            // SAFETY: `buf` outlives the transfer, which `transfer` waits for.
            unsafe { ReadFile(handle, pointer.cast(), length, null_mut(), overlapped) }
        }) {
            Ok(n) => Ok(n),
            // The other end went away, or this end was shut down: end of stream either way.
            Err(
                ERROR_BROKEN_PIPE
                | ERROR_OPERATION_ABORTED
                | ERROR_NO_DATA
                | ERROR_PIPE_NOT_CONNECTED,
            ) => Ok(0),
            Err(other) => Err(to_io(other)),
        }
    }

    pub fn write_all(&self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            if self.closed.load(Ordering::SeqCst) {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
            let chunk = &bytes[..bytes.len().min(u32::MAX as usize)];
            let pointer = chunk.as_ptr();
            let length = chunk.len() as u32;
            let written = transfer(self.pipe.handle, |handle, overlapped| {
                // SAFETY: `chunk` outlives the transfer, which `transfer` waits for.
                unsafe { WriteFile(handle, pointer.cast(), length, null_mut(), overlapped) }
            })
            .map_err(to_io)?;
            if written == 0 {
                return Err(io::Error::from(io::ErrorKind::WriteZero));
            }
            bytes = &bytes[written..];
        }
        Ok(())
    }

    /// Ends this session for every clone of it: a pending read returns end-of-stream and a later
    /// write fails. The endpoint stays up to host the next helper.
    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // SAFETY: cancels every pending request on a handle this session keeps alive, then hangs up.
        unsafe {
            CancelIoEx(self.pipe.handle, null());
            DisconnectNamedPipe(self.pipe.handle);
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

/// The connect in progress, kept across calls. `accept` can time out and be called again (look for
/// a helper briefly, keep listening in case one starts later), and two concurrent connects on one
/// server are an error, so the second attempt waits on the connect the first one started.
struct PendingConnect {
    overlapped: OVERLAPPED,
    event: HANDLE,
}

// SAFETY: only ever touched under the server's gate.
unsafe impl Send for PendingConnect {}

struct ServerState {
    pending: Option<Box<PendingConnect>>,
    /// A previous session left the pipe connected; it has to be let go before it can wait again.
    connected: bool,
}

pub struct PipeServer {
    pipe: Arc<PipeHandle>,
    name: String,
    state: Mutex<ServerState>,
    current: Mutex<Option<PipeSession>>,
}

/// Why a pipe could not be created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PipeCreateError {
    /// The name is taken, which for a per-user name means another copy of this app owns it.
    InUse,
    Failed(String),
}

impl PipeServer {
    /// `\\.\pipe\<name>`: byte mode, one instance, only `user_sid` may open it, remote clients
    /// refused. Real buffers (see the module comment).
    pub fn create(
        name: &str,
        user_sid: &str,
        in_buffer: u32,
        out_buffer: u32,
    ) -> Result<PipeServer, PipeCreateError> {
        let sddl = wide(&sddl_for(user_sid));
        let mut descriptor: *mut std::ffi::c_void = null_mut();
        // SAFETY: `sddl` is NUL-terminated; the descriptor is freed below.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(PipeCreateError::Failed(
                "the pipe's access rules".to_string(),
            ));
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let path = wide(&format!(r"\\.\pipe\{name}"));
        // SAFETY: every pointer outlives the call.
        let handle = unsafe {
            CreateNamedPipeW(
                path.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                out_buffer,
                in_buffer,
                0,
                &attributes,
            )
        };
        // SAFETY: reading the thread's last error, then freeing what Convert... allocated.
        let error = unsafe { GetLastError() };
        unsafe { LocalFree(descriptor) };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            // ERROR_ACCESS_DENIED / ERROR_PIPE_BUSY: the first-instance flag found one already there.
            return Err(match error {
                5 | 231 => PipeCreateError::InUse,
                other => PipeCreateError::Failed(to_io(other).to_string()),
            });
        }
        Ok(PipeServer {
            pipe: Arc::new(PipeHandle { handle }),
            name: name.to_string(),
            state: Mutex::new(ServerState {
                pending: None,
                connected: false,
            }),
            current: Mutex::new(None),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Waits for a client to connect and checks it. `None` if nobody did in time (safe to call again:
    /// the endpoint stays up) or the peer was not acceptable (it is hung up on).
    pub fn accept(&self, check: PeerCheck, timeout: Duration) -> Option<PipeSession> {
        let mut state = self.state.lock().unwrap();
        if state.pending.is_none() {
            // Whoever held the pipe last is let go first.
            if let Some(previous) = self.current.lock().unwrap().take() {
                previous.close();
            }
            if state.connected {
                // SAFETY: hangs up the previous session on a handle this server owns.
                unsafe { DisconnectNamedPipe(self.pipe.handle) };
                state.connected = false;
            }

            // SAFETY: a manual-reset, initially unsignalled, unnamed event.
            let event = unsafe { CreateEventW(null(), 1, 0, null()) };
            if event.is_null() {
                return None;
            }
            // SAFETY: OVERLAPPED is plain old data; zero is its documented initial state.
            let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
            overlapped.hEvent = event;
            let mut pending = Box::new(PendingConnect { overlapped, event });
            // SAFETY: the boxed OVERLAPPED has a stable address until the connect completes, which
            // `Drop` and the paths below make sure of.
            let started = unsafe { ConnectNamedPipe(self.pipe.handle, &mut pending.overlapped) };
            if started == 0 {
                // SAFETY: reading the thread's last error.
                match unsafe { GetLastError() } {
                    ERROR_IO_PENDING => state.pending = Some(pending),
                    ERROR_PIPE_CONNECTED => {
                        // SAFETY: closing the event created above.
                        unsafe { CloseHandle(event) };
                        state.connected = true;
                    }
                    _ => {
                        // SAFETY: as above.
                        unsafe { CloseHandle(event) };
                        return None;
                    }
                }
            } else {
                // SAFETY: as above.
                unsafe { CloseHandle(event) };
                state.connected = true;
            }
        }

        if let Some(pending) = state.pending.as_ref() {
            let millis = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
            // SAFETY: the event belongs to the pending connect, which is alive.
            match unsafe { WaitForSingleObject(pending.event, millis) } {
                WAIT_TIMEOUT => return None,
                WAIT_OBJECT_0 => {}
                _ => {
                    self.abandon_pending(&mut state);
                    return None;
                }
            }
            let mut transferred = 0u32;
            // SAFETY: the connect has signalled; reading its result.
            let ok = unsafe {
                GetOverlappedResult(self.pipe.handle, &pending.overlapped, &mut transferred, 0)
            };
            let pending = state.pending.take().expect("checked above");
            // SAFETY: closing the event of a connect that has finished.
            unsafe { CloseHandle(pending.event) };
            if ok == 0 {
                return None;
            }
            state.connected = true;
        }

        if !state.connected {
            return None;
        }

        if !self.acceptable(check) {
            // SAFETY: hangs up a peer that is not ours.
            unsafe { DisconnectNamedPipe(self.pipe.handle) };
            state.connected = false;
            return None;
        }

        let session = PipeSession {
            pipe: self.pipe.clone(),
            closed: Arc::new(AtomicBool::new(false)),
        };
        *self.current.lock().unwrap() = Some(session.clone());
        Some(session)
    }

    fn abandon_pending(&self, state: &mut ServerState) {
        if let Some(pending) = state.pending.take() {
            let mut transferred = 0u32;
            // SAFETY: cancel the connect, then wait for it to finish before the OVERLAPPED goes.
            unsafe {
                CancelIoEx(self.pipe.handle, &pending.overlapped);
                GetOverlappedResult(self.pipe.handle, &pending.overlapped, &mut transferred, 1);
                CloseHandle(pending.event);
            }
        }
    }

    fn acceptable(&self, check: PeerCheck) -> bool {
        if !client_is_this_user(self.pipe.handle) {
            return false;
        }
        match check {
            PeerCheck::Launched(process_id) => {
                let mut client = 0u32;
                // SAFETY: the pipe is connected and `client` is a valid out pointer.
                let ok = unsafe { GetNamedPipeClientProcessId(self.pipe.handle, &mut client) };
                ok != 0 && client == process_id
            }
            PeerCheck::Elevated => PeerIntegrity::client_is_elevated(&*self.pipe),
            PeerCheck::SameUser => true,
        }
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.abandon_pending(&mut state);
        if let Some(session) = self
            .current
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            session.close();
        }
    }
}

/// The account the connected client runs as is this process's own. Read from the client's token by
/// impersonating it at `SECURITY_IDENTIFICATION`, which permits exactly this and nothing more.
fn client_is_this_user(pipe: HANDLE) -> bool {
    let Ok(own) = bb_core::ipc::IndexEndpoint::current_user_sid() else {
        return false;
    };
    // SAFETY: the pipe is connected; impersonation is reverted by the guard.
    if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
        return false;
    }
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            // SAFETY: ends the impersonation begun above.
            unsafe { RevertToSelf() };
        }
    }
    let _revert = Revert;

    let mut token: HANDLE = null_mut();
    // SAFETY: opening the impersonation token of the current thread; closed below.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
        return false;
    }
    let mut needed = 0u32;
    // SAFETY: a size query.
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut needed) };
    let theirs = if needed == 0 {
        None
    } else {
        let mut buffer = vec![0u64; needed.div_ceil(8) as usize];
        // SAFETY: the buffer is at least `needed` bytes and aligned for TOKEN_USER.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        };
        if ok == 0 {
            None
        } else {
            // SAFETY: the buffer now holds a TOKEN_USER.
            let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
            let mut text: *mut u16 = null_mut();
            // SAFETY: converts a SID this call just read; the string is freed below.
            if sid.is_null() || unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
                None
            } else {
                let mut length = 0;
                // SAFETY: `text` is a NUL-terminated string Windows allocated.
                while unsafe { *text.add(length) } != 0 {
                    length += 1;
                }
                // SAFETY: as above.
                let s =
                    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
                unsafe { LocalFree(text.cast()) };
                Some(s)
            }
        }
    };
    // SAFETY: closing the token opened above.
    unsafe { CloseHandle(token) };
    theirs.is_some_and(|sid| sid.eq_ignore_ascii_case(&own))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT,
    };

    fn sid() -> String {
        bb_core::ipc::IndexEndpoint::current_user_sid().expect("a user sid")
    }

    fn unique(tag: &str) -> String {
        format!(
            "BertBrowser.Test.{tag}.{}.{:?}",
            std::process::id(),
            std::time::Instant::now()
        )
        .replace([' ', ':', '{', '}'], "")
    }

    fn connect(name: &str) -> std::fs::File {
        let path = wide(&format!(r"\\.\pipe\{name}"));
        // SAFETY: NUL-terminated path; a plain synchronous client handle is enough for a test.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                null_mut(),
            )
        };
        assert_ne!(
            INVALID_HANDLE_VALUE,
            handle,
            "connect: {}",
            io::Error::last_os_error()
        );
        let _ = FILE_FLAG_OVERLAPPED;
        // SAFETY: a handle we just opened and own.
        std::fs::File::from(unsafe { OwnedHandle::from_raw_handle(handle.cast()) })
    }

    #[test]
    fn the_descriptor_admits_only_the_named_user() {
        assert_eq!("D:(A;;0x0012019F;;;S-1-5-21-1)", sddl_for("S-1-5-21-1"));
    }

    #[test]
    fn a_second_server_on_a_taken_name_is_in_use_not_a_crash() {
        let name = unique("taken");
        let _first = PipeServer::create(&name, &sid(), 4096, 4096).unwrap();
        assert_eq!(
            Some(PipeCreateError::InUse),
            PipeServer::create(&name, &sid(), 4096, 4096).err()
        );
    }

    #[test]
    fn nobody_connecting_times_out_and_the_endpoint_stays_up() {
        let name = unique("quiet");
        let server = PipeServer::create(&name, &sid(), 4096, 4096).unwrap();
        assert!(
            server
                .accept(
                    PeerCheck::Launched(std::process::id()),
                    Duration::from_millis(30)
                )
                .is_none()
        );
        // A client arriving after the timeout is still picked up by the next call.
        let mut client = connect(&name);
        client.write_all(b"hello\n").unwrap();
        let session = server
            .accept(
                PeerCheck::Launched(std::process::id()),
                Duration::from_secs(2),
            )
            .expect("the late client");
        let mut buf = [0u8; 16];
        let n = session.read(&mut buf).unwrap();
        assert_eq!(b"hello\n", &buf[..n]);
    }

    #[test]
    fn a_peer_that_is_not_the_process_we_launched_is_hung_up_on() {
        let name = unique("wrongpid");
        let server = PipeServer::create(&name, &sid(), 4096, 4096).unwrap();
        let _client = connect(&name);
        assert!(
            server
                .accept(
                    PeerCheck::Launched(std::process::id().wrapping_add(7)),
                    Duration::from_secs(2)
                )
                .is_none()
        );
    }

    #[test]
    fn this_unelevated_test_process_is_not_accepted_as_an_attaching_helper() {
        // The medium-integrity peer check: an ordinary program of this user is not a helper.
        let name = unique("medium");
        let server = PipeServer::create(&name, &sid(), 4096, 4096).unwrap();
        let _client = connect(&name);
        let elevated = bb_core::elevation::elevated_process::can_elevate();
        let got = server.accept(PeerCheck::Elevated, Duration::from_secs(2));
        // Only an elevated test run would be accepted.
        if !is_test_process_elevated() {
            assert!(
                got.is_none(),
                "an unelevated peer must be refused (can_elevate={elevated})"
            );
        }
    }

    fn is_test_process_elevated() -> bool {
        use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TokenElevation};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut token: HANDLE = null_mut();
        // SAFETY: querying this process's own token.
        unsafe {
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut n = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                (&mut e as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut n,
            );
            CloseHandle(token);
            ok != 0 && e.TokenIsElevated != 0
        }
    }

    #[test]
    fn a_session_round_trips_and_a_second_helper_can_follow_the_first() {
        let name = unique("sessions");
        let server = PipeServer::create(&name, &sid(), 4096, 4096).unwrap();
        for round in 0..2 {
            // Listening again is what `accept` does first; a short wait leaves the pipe listening.
            assert!(
                server
                    .accept(
                        PeerCheck::Launched(std::process::id()),
                        Duration::from_millis(10)
                    )
                    .is_none()
            );
            let mut client = connect(&name);
            let session = server
                .accept(
                    PeerCheck::Launched(std::process::id()),
                    Duration::from_secs(2),
                )
                .unwrap_or_else(|| panic!("round {round}"));
            session.write_all(b"Ready\n").unwrap();
            let mut buf = [0u8; 16];
            let n = client.read(&mut buf).unwrap();
            assert_eq!(b"Ready\n", &buf[..n]);
            client.write_all(b"Go\n").unwrap();
            let n = session.read(&mut buf).unwrap();
            assert_eq!(b"Go\n", &buf[..n]);
            session.close();
            assert!(session.write_all(b"x").is_err());
            assert_eq!(0, session.read(&mut buf).unwrap());
        }
    }

    #[test]
    fn closing_unblocks_a_read_parked_on_another_thread() {
        let name = unique("unblock");
        let server = PipeServer::create(&name, &sid(), 4096, 4096).unwrap();
        let _client = connect(&name);
        let session = server
            .accept(
                PeerCheck::Launched(std::process::id()),
                Duration::from_secs(2),
            )
            .unwrap();
        let reader = session.clone();
        let handle = std::thread::spawn(move || {
            let mut buf = [0u8; 8];
            reader.read(&mut buf)
        });
        std::thread::sleep(Duration::from_millis(100));
        session.close();
        assert_eq!(0, handle.join().unwrap().unwrap());
    }

    #[test]
    fn two_threads_can_use_one_session_at_once_without_deadlocking() {
        let name = unique("duplex");
        let server = PipeServer::create(&name, &sid(), 16 * 1024, 4096).unwrap();
        let mut client = connect(&name);
        let session = server
            .accept(
                PeerCheck::Launched(std::process::id()),
                Duration::from_secs(2),
            )
            .unwrap();
        let reader = session.clone();
        // A read is parked on the handle while this thread writes: serialised I/O would hang here.
        let parked = std::thread::spawn(move || {
            let mut buf = [0u8; 8];
            reader.read(&mut buf).map(|n| buf[..n].to_vec())
        });
        std::thread::sleep(Duration::from_millis(100));
        session.write_all(b"out\n").unwrap();
        let mut buf = [0u8; 8];
        let n = client.read(&mut buf).unwrap();
        assert_eq!(b"out\n", &buf[..n]);
        client.write_all(b"in\n").unwrap();
        assert_eq!(b"in\n".to_vec(), parked.join().unwrap().unwrap());
    }
}
