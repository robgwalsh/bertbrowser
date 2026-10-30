//! Explorer-style "logical" string comparison (file2 < file10), through `StrCmpLogicalW` so the
//! order matches what the user sees everywhere else on the machine.
//! Spec: src/BertBrowser.App/Interop/NaturalSort.cs.

use std::cmp::Ordering;

use windows_sys::Win32::UI::Shell::StrCmpLogicalW;

/// Compares two names the way Explorer sorts them.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a = crate::wide(a);
    let b = crate::wide(b);
    // SAFETY: both are NUL-terminated UTF-16 buffers that outlive the call.
    let r = unsafe { StrCmpLogicalW(a.as_ptr(), b.as_ptr()) };
    r.cmp(&0)
}
