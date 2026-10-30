//! The name of the index pipe, derived the same way in both processes. Spec:
//! Core/Ipc/IndexEndpoint.cs.
//!
//! **The helper is not told which pipe to call back on; it works it out.** That used to be an
//! argument carrying a random nonce, which was right while the helper died with the app that
//! launched it. It stopped being right once the helper started outliving apps, because a helper
//! from an earlier app has to be able to find the *next* one, and it cannot guess a nonce.
//!
//! Deriving it from the process's own token is a better answer than validating an argument: **the
//! elevated process can no longer be pointed at a name anybody chose.** UAC hands the same user a
//! different token, not a different account, so the SID the helper computes is the SID the app
//! computed, and the two meet without either trusting the other's input.
//!
//! There is deliberately no fallback for a token with no SID. The old one substituted a constant,
//! which was harmless while the name also carried a nonce and is not harmless now: two users whose
//! tokens both failed to report a SID would derive the same name, and the DACL is the only thing
//! that would then keep them apart. Failing loudly is the smaller problem.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::{Error, IndexerPresenceLock, OwnedHandle, last_error};

pub struct IndexEndpoint;

impl IndexEndpoint {
    /// The prefix every name this app generates begins with.
    pub const PREFIX: &'static str = "BertBrowser.Index.";

    /// The pipe name for the current user's helper.
    pub fn for_current_user() -> Result<String, Error> {
        Self::for_user(&Self::current_user_sid()?)
    }

    /// The pipe name for a given user's SID.
    pub fn for_user(user_sid: &str) -> Result<String, Error> {
        if !IndexerPresenceLock::is_acceptable_sid(user_sid) {
            return Err(Error::UnacceptableSid(user_sid.to_string()));
        }
        Ok(format!("{}{user_sid}", Self::PREFIX))
    }

    /// This process's user SID, which is the same under an elevated token as a filtered one.
    pub fn current_user_sid() -> Result<String, Error> {
        let mut token: HANDLE = null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(last_error("OpenProcessToken"));
        }
        let token = OwnedHandle(token);

        // Asking with no buffer reports how large one has to be.
        let mut needed = 0u32;
        unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut needed) };
        if needed == 0 {
            return Err(Error::NoUserSid);
        }
        // u64-backed so the TOKEN_USER (which holds a pointer) is aligned.
        let mut buffer = vec![0u64; needed.div_ceil(8) as usize];
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err(last_error("GetTokenInformation"));
        }

        let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        if sid.is_null() {
            return Err(Error::NoUserSid);
        }

        let mut text: *mut u16 = null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
            return Err(last_error("ConvertSidToStringSidW"));
        }
        let mut len = 0usize;
        while unsafe { *text.add(len) } != 0 {
            len += 1;
        }
        let sid_text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
        unsafe { LocalFree(text.cast()) };
        Ok(sid_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against the real token: what comes back is a SID the name rule accepts, so the two
    /// processes derive one name from it.
    #[test]
    fn the_current_user_sid_is_one_the_endpoint_accepts() {
        let sid = IndexEndpoint::current_user_sid().expect("a user SID");
        assert!(IndexerPresenceLock::is_acceptable_sid(&sid), "{sid}");
        assert_eq!(
            format!("BertBrowser.Index.{sid}"),
            IndexEndpoint::for_current_user().unwrap()
        );
    }

    #[test]
    fn a_sid_this_app_would_not_produce_is_refused() {
        assert_eq!(
            Err(Error::UnacceptableSid("Rob".to_string())),
            IndexEndpoint::for_user("Rob")
        );
    }
}
