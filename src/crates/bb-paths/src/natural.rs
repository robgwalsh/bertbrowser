//! Explorer-style "logical" string comparison (file2 < file10), through `StrCmpLogicalW` so the
//! order matches what the user sees everywhere else on the machine.
//! Spec: src/BertBrowser.App/Interop/NaturalSort.cs.

use std::cell::RefCell;
use std::cmp::Ordering;

use windows_sys::Win32::UI::Shell::StrCmpLogicalW;

thread_local! {
    /// The two NUL-terminated buffers a comparison encodes into. A sort calls this n log n times,
    /// and two fresh vectors per call were most of what sorting 10,000 rows by name cost.
    static BUFFERS: RefCell<(Vec<u16>, Vec<u16>)> = const { RefCell::new((Vec::new(), Vec::new())) };
}

/// Compares two names the way Explorer sorts them.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    BUFFERS.with(|cell| match cell.try_borrow_mut() {
        Ok(mut buffers) => {
            let (wa, wb) = &mut *buffers;
            fill(wa, a);
            fill(wb, b);
            compare(wa, wb)
        }
        // Only reachable if a comparison ever re-entered itself; never worth failing over.
        Err(_) => compare(&crate::wide(a), &crate::wide(b)),
    })
}

fn fill(buffer: &mut Vec<u16>, s: &str) {
    buffer.clear();
    buffer.extend(s.encode_utf16());
    buffer.push(0);
}

fn compare(a: &[u16], b: &[u16]) -> Ordering {
    // SAFETY: both are NUL-terminated UTF-16 buffers that outlive the call.
    let r = unsafe { StrCmpLogicalW(a.as_ptr(), b.as_ptr()) };
    r.cmp(&0)
}
