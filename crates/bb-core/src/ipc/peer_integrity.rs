//! Whether the process on the other end of a pipe holds an administrator token. Spec:
//! Core/Ipc/PeerIntegrity.cs.
//!
//! Used by the app to check that something claiming to be the index helper actually is one. It
//! matters because the helper outlives the app: a session that *attaches* to a helper this process
//! did not launch cannot compare a process id against one it started, so
//! `GetNamedPipeClientProcessId` has nothing to be checked against.
//!
//! **This works in the direction an image-path check does not.** A medium-integrity process cannot
//! open a high-integrity one to ask what it is running (see `PipeOwner::image_path_of`), but it can
//! impersonate a client that connected to its own pipe and read that token's label: the helper
//! connects at `SECURITY_IDENTIFICATION`, which permits querying even though it permits nothing
//! else. Verified against a real medium server and a real elevated client rather than assumed.
//!
//! **It is a coherence check, not a security boundary.** Nothing between two processes of one user
//! is one. What it rules out is another of this user's own ordinary programs being mistaken for the
//! helper and feeding this app an index state nothing is actually maintaining.

use std::os::windows::io::AsRawHandle;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, RevertToSelf,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenIntegrityLevel,
};
use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;
use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

use super::OwnedHandle;

pub struct PeerIntegrity;

impl PeerIntegrity {
    /// The mandatory label of an elevated process.
    pub const HIGH_INTEGRITY_RID: u32 = 0x3000;

    /// The decision, apart from the token it is made about: High or better counts as elevated.
    pub fn is_elevated_rid(rid: u32) -> bool {
        rid >= Self::HIGH_INTEGRITY_RID
    }

    /// True when the client connected to `server` (the server end of a named pipe) holds a High or
    /// better token. False whenever the answer cannot be established, so an unreadable token is
    /// refused rather than assumed friendly.
    pub fn client_is_elevated<H: AsRawHandle + ?Sized>(server: &H) -> bool {
        let pipe: HANDLE = server.as_raw_handle().cast();
        if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
            return false;
        }
        let _reverting = Impersonation;
        current_thread_is_elevated()
    }
}

/// Reverts the thread's identity on drop, so no early return leaves it impersonating.
struct Impersonation;

impl Drop for Impersonation {
    fn drop(&mut self) {
        unsafe { RevertToSelf() };
    }
}

fn current_thread_is_elevated() -> bool {
    let mut token: HANDLE = null_mut();
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
        return false;
    }
    let token = OwnedHandle(token);

    // Asking with no buffer reports how large one has to be.
    let mut needed = 0u32;
    unsafe { GetTokenInformation(token.0, TokenIntegrityLevel, null_mut(), 0, &mut needed) };
    if needed == 0 {
        return false;
    }
    // u64-backed so the label (which holds a pointer) is aligned.
    let mut buffer = vec![0u64; needed.div_ceil(8) as usize];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return false;
    }

    // TOKEN_MANDATORY_LABEL is a SID_AND_ATTRIBUTES, whose first field is the SID.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid };
    if sid.is_null() {
        return false;
    }
    let count = unsafe { *GetSidSubAuthorityCount(sid) };
    if count == 0 {
        return false;
    }
    let rid = unsafe { *GetSidSubAuthority(sid, u32::from(count) - 1) };
    PeerIntegrity::is_elevated_rid(rid)
}
