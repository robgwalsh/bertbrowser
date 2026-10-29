//! Handing back what this process created, so the unelevated app can still finish with it.
//! Spec: src/BertBrowser.Elevator/StagingAcl.cs.
//!
//! **The one piece of the elevated path with no test behind it and a silent failure mode.** A
//! staged delete puts the user's data in `<volume root>\.bertbrowser-trash\delete-<id>` and a
//! Replace puts the displaced entry in `.bertbrowser-replaced-*`. A folder created at a volume root
//! inherits that root's ACL, which grants ordinary users read and create but not delete, so a
//! holding folder this process makes is one the app cannot later commit, purge, or move an item
//! back out of on Ctrl+Z. Every staging folder the run created therefore gets an inheritable
//! full-control grant for the account that asked. Verify by hand with `icacls` after an elevated
//! staged delete: the interactive user must have `(F)`, inherited.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, SE_FILE_OBJECT, SetEntriesInAclW,
    SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    ACL, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE,
    PSECURITY_DESCRIPTOR, PSID,
};

const FILE_ALL_ACCESS: u32 = 0x001F_01FF;

/// A grant for the given SID, ready to hand to `ElevationHost::with_grant_access`. Never panics: a
/// failure here costs the app the ability to tidy a hidden folder, which must not be allowed to
/// take down an operation that has already moved the user's data.
pub fn grant_to(user_sid: &str) -> impl Fn(&str) + '_ {
    move |directory: &str| {
        if !std::path::Path::new(directory).is_dir() {
            return;
        }
        if let Err(message) = grant(directory, user_sid) {
            eprintln!("Could not hand back '{directory}': {message}");
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn grant(directory: &str, user_sid: &str) -> Result<(), String> {
    let sid_text = wide(user_sid);
    let mut sid: PSID = null_mut();
    // SAFETY: NUL-terminated input; the SID is freed with LocalFree below.
    if unsafe { ConvertStringSidToSidW(sid_text.as_ptr(), &mut sid) } == 0 {
        return Err("not a SID".to_string());
    }

    let path = wide(directory);
    let mut old_dacl: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: out-pointers are valid; the descriptor is freed with LocalFree below.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut old_dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    let mut outcome = if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("GetNamedSecurityInfo failed with error {status}"))
    };

    let mut new_dacl: *mut ACL = null_mut();
    if outcome.is_ok() {
        // SAFETY: a plain-old-data struct, filled below.
        let mut access: EXPLICIT_ACCESS_W = unsafe { std::mem::zeroed() };
        access.grfAccessPermissions = FILE_ALL_ACCESS;
        access.grfAccessMode = GRANT_ACCESS;
        access.grfInheritance = OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE;
        access.Trustee = TRUSTEE_W {
            pMultipleTrustee: null_mut(),
            MultipleTrusteeOperation: 0,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: sid.cast(),
        };
        // SAFETY: one valid entry; `old_dacl` belongs to `descriptor`, still alive.
        let status = unsafe { SetEntriesInAclW(1, &access, old_dacl, &mut new_dacl) };
        if status != ERROR_SUCCESS {
            outcome = Err(format!("SetEntriesInAcl failed with error {status}"));
        }
    }
    if outcome.is_ok() {
        // SAFETY: `new_dacl` was just built; the path is NUL-terminated.
        let status = unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr().cast_mut(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                new_dacl,
                null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            outcome = Err(format!("SetNamedSecurityInfo failed with error {status}"));
        }
    }

    // SAFETY: each pointer is either null or what the matching call allocated.
    unsafe {
        if !new_dacl.is_null() {
            LocalFree(new_dacl.cast());
        }
        if !descriptor.is_null() {
            LocalFree(descriptor);
        }
        LocalFree(sid);
    }
    outcome
}
