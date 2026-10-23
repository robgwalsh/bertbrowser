//! The one object that answers "is an index helper running?", and the thing that stops two of them
//! running at once. Spec: Core/Ipc/IndexerPresenceLock.cs.
//!
//! The helper holds a named mutex for its whole life; the app asks whether that name exists. Both
//! halves matter and they are deliberately the same object. **Presence:** the helper outlives the
//! app, so an app starting up has to find out whether one is already there before deciding to raise
//! a UAC prompt; waiting to see whether somebody connects would answer eventually and wrongly under
//! load, this answers immediately. **Exclusion:** two elevated helpers would tail one volume's
//! journal into one SQLite file and fight over the same pipe name, and nothing else in the design
//! prevents it; the scheduled task and a click on the banner are two independent ways to start one.
//!
//! **The medium-integrity app can open a high-integrity process's mutex** because `SYNCHRONIZE` is
//! not a write right and mandatory policy is no-write-*up*. That is the same asymmetry the pipe's
//! direction rests on, used the same way, and it was verified against a real medium-integrity
//! process rather than assumed. The explicit DACL is still required: without one, the default
//! admits the creator's token and not much else.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Threading::{CreateMutexW, OpenMutexW};

use super::{Error, to_wide};

/// The one right the app is granted on the helper's mutex: enough to answer "does it exist?",
/// not enough to acquire or release it.
const SYNCHRONIZE: u32 = 0x0010_0000;

pub struct IndexerPresenceLock;

impl IndexerPresenceLock {
    /// The mutex name for a user's helper. `Local\`, not `Global\`: the helper and the app live in
    /// one interactive logon session (including when the helper is started by the sign-in task),
    /// and a session-scoped name keeps two users' helpers from ever meeting.
    pub fn name_for(user_sid: &str) -> String {
        format!(r"Local\BertBrowser.Indexer.{user_sid}")
    }

    /// The security descriptor the helper creates the mutex with: this user may synchronise on it,
    /// administrators and the system may do anything. The user gets `SYNCHRONIZE` and nothing more.
    /// Nothing between two processes of one user is a security boundary, but there is no reason to
    /// hand out more than the question needs.
    pub fn sddl(user_sid: &str) -> String {
        format!("D:(A;;0x{SYNCHRONIZE:08X};;;{user_sid})(A;;GA;;;BA)(A;;GA;;;SY)")
    }

    /// Claims the name for this process: `Ok(None)` when another helper already holds it, or when
    /// Windows would not create it. Dropping the claim releases it. The handle is the claim, not
    /// ownership of the mutex: nothing ever waits on it, so there is no lock to acquire and nothing
    /// to abandon if this process is killed. The kernel drops the name when the last handle closes,
    /// however the process ended.
    pub fn try_acquire(user_sid: &str) -> Result<Option<PresenceClaim>, Error> {
        if !Self::is_acceptable_sid(user_sid) {
            return Err(Error::UnacceptableSid(user_sid.to_string()));
        }

        let sddl = to_wide(&Self::sddl(user_sid));
        let mut descriptor: *mut std::ffi::c_void = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Ok(None);
        }
        let descriptor = LocalBlock(descriptor);

        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let name = to_wide(&Self::name_for(user_sid));
        let handle = unsafe { CreateMutexW(&attributes, 0, name.as_ptr()) };
        let error = unsafe { GetLastError() };

        if handle.is_null() {
            return Ok(None);
        }
        if error == ERROR_ALREADY_EXISTS {
            // We opened somebody else's. Let go of it rather than holding a handle that would keep
            // the name alive after they exit.
            unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(PresenceClaim(handle)))
    }

    /// True when a helper is running for this user.
    pub fn is_held(user_sid: &str) -> bool {
        if !Self::is_acceptable_sid(user_sid) {
            return false;
        }
        let name = to_wide(&Self::name_for(user_sid));
        let handle = unsafe { OpenMutexW(SYNCHRONIZE, 0, name.as_ptr()) };
        if handle.is_null() {
            return false;
        }
        unsafe { CloseHandle(handle) };
        true
    }

    /// A SID in the form `ConvertSidToStringSid` produces, and nothing else. This string is
    /// interpolated into both an SDDL string and a kernel object name: a value carrying `)` or `\`
    /// would rewrite the descriptor or name a different object, so it is checked here rather than
    /// trusted, the same discipline `IndexerArguments` applies to everything that reaches the
    /// elevated process.
    pub fn is_acceptable_sid(candidate: &str) -> bool {
        let length = candidate.chars().count();
        if !(3..=184).contains(&length) {
            return false;
        }
        let Some(rest) = candidate.strip_prefix("S-") else {
            return false;
        };
        if rest.ends_with('-') {
            return false;
        }
        // Past the "S-", a SID is only digits and the hyphens between them.
        rest.bytes().all(|b| b == b'-' || b.is_ascii_digit())
    }
}

/// The helper's hold on its name; dropping it lets the next helper start.
pub struct PresenceClaim(HANDLE);

// A kernel object handle is process-wide; which thread closes it does not matter.
unsafe impl Send for PresenceClaim {}
unsafe impl Sync for PresenceClaim {}

impl Drop for PresenceClaim {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
            self.0 = null_mut();
        }
    }
}

/// Memory `ConvertStringSecurityDescriptorToSecurityDescriptorW` allocated, freed on drop.
struct LocalBlock(*mut std::ffi::c_void);

impl Drop for LocalBlock {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A SID no account on this machine has, so a real helper running for the developer cannot be
    /// the one holding the name.
    const SID: &str = "S-1-5-21-424242-424242-424242-1001";

    #[test]
    fn the_sddl_grants_the_user_synchronise_only() {
        assert_eq!(
            "D:(A;;0x00100000;;;S-1-5-21-1-2-3)(A;;GA;;;BA)(A;;GA;;;SY)",
            IndexerPresenceLock::sddl("S-1-5-21-1-2-3")
        );
        assert_eq!(
            r"Local\BertBrowser.Indexer.S-1-5-21-1-2-3",
            IndexerPresenceLock::name_for("S-1-5-21-1-2-3")
        );
    }

    /// Against the real kernel: the claim is visible while held, refused to a second claimant, and
    /// gone when dropped.
    #[test]
    fn a_claim_is_visible_exclusive_and_released_on_drop() {
        assert!(!IndexerPresenceLock::is_held(SID));

        let claim = IndexerPresenceLock::try_acquire(SID)
            .unwrap()
            .expect("the name was free");
        assert!(IndexerPresenceLock::is_held(SID));
        assert!(IndexerPresenceLock::try_acquire(SID).unwrap().is_none());

        drop(claim);
        assert!(!IndexerPresenceLock::is_held(SID));
    }

    #[test]
    fn a_sid_that_could_rewrite_the_descriptor_is_refused() {
        for sid in [
            "",
            "S-",
            "S-1-5-",
            "S-1-5-21-1)(A;;GA;;;WD",
            r"S-1-5\x",
            "Rob",
        ] {
            assert!(!IndexerPresenceLock::is_acceptable_sid(sid), "{sid:?}");
            assert!(IndexerPresenceLock::try_acquire(sid).is_err(), "{sid:?}");
            assert!(!IndexerPresenceLock::is_held(sid), "{sid:?}");
        }
        assert!(IndexerPresenceLock::is_acceptable_sid(SID));
    }
}
