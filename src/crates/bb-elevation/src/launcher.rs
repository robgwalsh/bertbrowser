//! Starting one short-lived elevated helper, and the pipe it calls back on.
//! Spec: Core/Services/Elevation/IElevationLauncher.cs, App/Services/Elevation/ElevatedProcess.cs
//! and ElevatedFileOperationLauncher.cs.
//!
//! The seam that keeps a UAC prompt out of a test and out of a scripted harness run. The real
//! launcher at the bottom is the one `ShellExecuteExW` with `runas` outside the app's process
//! launcher: thin, and untested, because the one thing it does is the one thing no test can answer.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::Mutex;
use std::time::Duration;

/// How starting the elevated helper went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElevationLaunch {
    Started,
    /// The user answered the UAC prompt with No (`ERROR_CANCELLED`). Their choice, reported as
    /// such rather than as a failure.
    Declined,
    /// No prompt was raised because this account cannot elevate.
    NotAdministrator,
    Failed,
}

/// `detail` is phrased for the status bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElevationLaunchResult {
    pub outcome: ElevationLaunch,
    pub process_id: u32,
    pub detail: String,
}

impl ElevationLaunchResult {
    pub fn started(process_id: u32) -> ElevationLaunchResult {
        ElevationLaunchResult {
            outcome: ElevationLaunch::Started,
            process_id,
            detail: String::new(),
        }
    }

    pub fn declined() -> ElevationLaunchResult {
        ElevationLaunchResult {
            outcome: ElevationLaunch::Declined,
            process_id: 0,
            detail: String::new(),
        }
    }

    pub fn not_administrator() -> ElevationLaunchResult {
        ElevationLaunchResult {
            outcome: ElevationLaunch::NotAdministrator,
            process_id: 0,
            detail: String::new(),
        }
    }

    pub fn failed(detail: &str) -> ElevationLaunchResult {
        ElevationLaunchResult {
            outcome: ElevationLaunch::Failed,
            process_id: 0,
            detail: detail.to_string(),
        }
    }
}

pub trait ElevationLauncher {
    /// Whether this account could elevate if asked. Asked *before* the consent dialog is shown,
    /// never after: a standard user shown a shield gets a credential prompt for somebody else's
    /// password, which is a good deal worse than never being offered one.
    ///
    /// Implementations must read the token's elevation type, never "is in the Administrators
    /// role": an administrator running normally holds a filtered token in which that role is
    /// deny-only, so the role check answers false for precisely the people who can elevate.
    fn can_elevate(&self) -> bool;

    /// Raises the prompt and starts the helper.
    fn launch(
        &self,
        pipe_name: &str,
        parent_process_id: u32,
        user_sid: &str,
    ) -> ElevationLaunchResult;

    /// Waits for a started helper to go away. The app cannot terminate it (a medium process may
    /// not open a high one for that), so this is a wait and never a kill.
    fn wait_for_exit(&self, process_id: u32, timeout: Duration);
}

/// A byte stream two threads can share: one reading while the other writes. A pipe handle opened
/// overlapped, or an in-memory pair in a test.
pub trait ElevationStream: Read + Write + Send {
    /// Another handle on the same stream, so one thread can read while another writes.
    fn split(&self) -> io::Result<Box<dyn ElevationStream>>;

    /// Ends the stream for every handle on it: a pending read returns end-of-stream and a later
    /// write fails. What the helper does once its work is finished, to unpark the thread reading
    /// for a `Cancel` that is now never coming.
    fn shutdown(&self);
}

/// The pipe the helper calls back on. Created app-side, so the high-integrity helper's connection
/// is a write-down: mandatory policy forbids writing *up*, so a pipe the helper created could not
/// be written to by the app that owns it.
pub trait ElevationTransport {
    fn endpoint(&self) -> &str;

    /// Waits for the process just launched, and only that one, to connect.
    fn accept(&mut self, process_id: u32, timeout: Duration) -> Option<Box<dyn ElevationStream>>;
}

/// One transport per operation: this helper is one-shot, so its pipe is too.
pub trait ElevationTransportFactory {
    fn create(&self) -> Box<dyn ElevationTransport>;
}

/// Starts `bb-elevator.exe` beside this executable for one file operation, through `ShellExecuteExW`
/// with the `runas` verb. One helper per operation, and it exits when that operation is done, so
/// this keeps one handle per launch and lets it go.
pub struct ShellExecuteLauncher {
    helper_path: String,
    running: Mutex<HashMap<u32, win32::ProcessHandle>>,
}

impl ShellExecuteLauncher {
    /// `helper_path` is resolved beside the calling executable by the caller and never by name:
    /// what is being launched is the one thing that gets an administrator token, so it must not be
    /// findable on `PATH`.
    pub fn new(helper_path: &str) -> ShellExecuteLauncher {
        ShellExecuteLauncher {
            helper_path: helper_path.to_string(),
            running: Mutex::new(HashMap::new()),
        }
    }

    /// Quoted for the pipe name, which is generated by the transport; the other two are a number
    /// and this process's own SID. Nothing in this line comes from a file being browsed.
    fn format_arguments(pipe_name: &str, parent_process_id: u32, user_sid: &str) -> String {
        format!("--pipe \"{pipe_name}\" --parent-pid {parent_process_id} --user-sid \"{user_sid}\"")
    }
}

impl ElevationLauncher for ShellExecuteLauncher {
    fn can_elevate(&self) -> bool {
        win32::can_elevate()
    }

    fn launch(
        &self,
        pipe_name: &str,
        parent_process_id: u32,
        user_sid: &str,
    ) -> ElevationLaunchResult {
        // Worth being specific about: a build that never copied the helper beside the app fails
        // here, before any prompt is raised, and "unavailable" alone gives no hint that the cause is
        // a missing file rather than anything to do with elevation.
        if !std::path::Path::new(&self.helper_path).is_file() {
            return ElevationLaunchResult::failed("the helper that does this is missing");
        }

        let arguments = Self::format_arguments(pipe_name, parent_process_id, user_sid);
        match win32::start_elevated(&self.helper_path, &arguments) {
            Ok((handle, process_id)) => {
                let mut running = self.running.lock().unwrap();
                // A pid can be reused once the process behind it has gone, so an old entry under
                // the same number is stale by definition and never the one being waited on.
                running.insert(process_id, handle);
                ElevationLaunchResult::started(process_id)
            }
            Err(win32::StartError::Declined) => ElevationLaunchResult::declined(),
            Err(win32::StartError::Failed(detail)) => ElevationLaunchResult::failed(&detail),
        }
    }

    fn wait_for_exit(&self, process_id: u32, timeout: Duration) {
        let handle = self.running.lock().unwrap().remove(&process_id);
        if let Some(handle) = handle {
            handle.wait(timeout);
        }
    }
}

/// The Win32 behind the real launcher: the token's elevation type, `ShellExecuteExW` with `runas`,
/// and a wait on the handle it hands back rather than a reopen of the process, since a
/// medium-integrity process cannot count on opening a high-integrity one at all.
pub mod win32 {
    use std::ptr::null_mut;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, GetLastError, HANDLE};
    use windows_sys::Win32::Security::{
        AllocateAndInitializeSid, CheckTokenMembership, FreeSid, GetTokenInformation,
        SECURITY_NT_AUTHORITY, TOKEN_ELEVATION_TYPE, TOKEN_QUERY, TokenElevationType,
        TokenElevationTypeFull, TokenElevationTypeLimited,
    };
    use windows_sys::Win32::System::SystemServices::{
        DOMAIN_ALIAS_RID_ADMINS, SECURITY_BUILTIN_DOMAIN_RID,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessId, OpenProcessToken, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    pub struct ProcessHandle(HANDLE);

    // SAFETY: a process handle is a kernel object; it may be waited on from any thread.
    unsafe impl Send for ProcessHandle {}

    impl ProcessHandle {
        pub fn wait(&self, timeout: Duration) {
            let millis = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
            // SAFETY: the handle is open for as long as this struct lives.
            unsafe { WaitForSingleObject(self.0, millis) };
        }
    }

    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            // SAFETY: closing a handle this struct owns.
            unsafe { CloseHandle(self.0) };
        }
    }

    pub enum StartError {
        Declined,
        Failed(String),
    }

    /// `Limited` means a split token and therefore an admin, `Full` means already elevated, and
    /// `Default` means no split token at all, where group membership would be the question;
    /// there, and whenever the token cannot be read, the prompt is offered anyway: the worst
    /// case is one dialog, against silently never being able to do something the user could have.
    pub fn can_elevate() -> bool {
        let mut token: HANDLE = null_mut();
        // SAFETY: querying this process's own token; the handle is closed below.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return true;
        }
        let mut kind: TOKEN_ELEVATION_TYPE = 0;
        let mut returned = 0u32;
        // SAFETY: the buffer is exactly the size the class writes.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenElevationType,
                (&mut kind as *mut TOKEN_ELEVATION_TYPE).cast(),
                std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
                &mut returned,
            )
        };
        // SAFETY: closing the handle opened above.
        unsafe { CloseHandle(token) };
        if ok == 0 {
            return true;
        }
        if kind == TokenElevationTypeLimited || kind == TokenElevationTypeFull {
            return true;
        }
        is_administrator().unwrap_or(true)
    }

    /// `WindowsPrincipal.IsInRole(Administrator)`: meaningful only for a token with no split, which
    /// is the one case `can_elevate` asks it. None when the question could not be put.
    fn is_administrator() -> Option<bool> {
        let authority = SECURITY_NT_AUTHORITY;
        let mut sid = null_mut();
        // SAFETY: the authority is a static constant; the SID is freed below.
        let ok = unsafe {
            AllocateAndInitializeSid(
                &authority,
                2,
                SECURITY_BUILTIN_DOMAIN_RID as u32,
                DOMAIN_ALIAS_RID_ADMINS as u32,
                0,
                0,
                0,
                0,
                0,
                0,
                &mut sid,
            )
        };
        if ok == 0 {
            return None;
        }
        let mut member = 0i32;
        // SAFETY: a null token means the calling thread's; the SID was just allocated.
        let ok = unsafe { CheckTokenMembership(null_mut(), sid, &mut member) };
        // SAFETY: freeing what AllocateAndInitializeSid allocated.
        unsafe { FreeSid(sid) };
        if ok == 0 { None } else { Some(member != 0) }
    }

    pub fn start_elevated(
        exe_path: &str,
        arguments: &str,
    ) -> Result<(ProcessHandle, u32), StartError> {
        let verb = wide("runas");
        let file = wide(exe_path);
        let parameters = wide(arguments);
        let directory = std::path::Path::new(exe_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let directory = wide(&directory);

        // SAFETY: a plain-old-data struct, zeroed then filled; every pointer outlives the call.
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.lpDirectory = directory.as_ptr();
        info.nShow = SW_HIDE;

        // SAFETY: `info` is fully initialised and the strings it points at are alive.
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            // SAFETY: reading the thread's last error.
            let error = unsafe { GetLastError() };
            return Err(if error == ERROR_CANCELLED {
                StartError::Declined
            } else {
                StartError::Failed(std::io::Error::from_raw_os_error(error as i32).to_string())
            });
        }

        let handle = ProcessHandle(info.hProcess);
        // SAFETY: a process handle ShellExecuteEx just handed back.
        let process_id = unsafe { GetProcessId(info.hProcess) };
        if process_id == 0 {
            return Err(StartError::Failed(
                "it started but could not be identified".to_string(),
            ));
        }
        Ok((handle, process_id))
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}
