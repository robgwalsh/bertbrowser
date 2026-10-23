//! Whether a failure was Windows refusing permission: the one bit that separates a failure an
//! administrator token could fix from every other kind. Spec: Core/Services/AccessDenied.cs.
//!
//! Until this existed every executor turned every failure into its message, which is localised, so
//! nothing downstream could ever match on it: "this needs a token" and "this file is open in Word"
//! left the executor as the same thing. **Two arms, and no more.** The Win32 code
//! `ERROR_ACCESS_DENIED` is what the file calls report; a `PermissionDenied` error with no code is
//! what a copier manufactures for it. Nothing else counts, not a message that says "denied" and not
//! a kind that merely sounds like one, because every false positive costs the user a UAC prompt
//! that cannot possibly help. The not-same-device code, the one other Win32 code this codebase
//! branches on, has its own test asserting it is not mistaken for this.

use std::io;

pub struct AccessDenied;

impl AccessDenied {
    /// `ERROR_ACCESS_DENIED`.
    pub const WIN32_CODE: i32 = 5;

    /// The HRESULT form of `ERROR_ACCESS_DENIED`, spelled from the Win32 code rather than written
    /// out.
    pub const HRESULT: i32 = (0x8007_0000_u32 | Self::WIN32_CODE as u32) as i32;

    /// True when `err` is Windows refusing permission.
    pub fn caused(err: Option<&io::Error>) -> bool {
        match err {
            None => false,
            Some(e) => match e.raw_os_error() {
                Some(code) => code == Self::WIN32_CODE,
                None => e.kind() == io::ErrorKind::PermissionDenied,
            },
        }
    }
}
