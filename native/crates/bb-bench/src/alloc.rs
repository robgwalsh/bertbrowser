//! A counting global allocator, so what an operation allocates is a number rather than a sample.
//!
//! There is no collector to report generations, so the two figures the schema carries per operation
//! are exact: bytes requested and allocator calls. Counting is off while timing (one relaxed load per
//! call) and on for a separate pass, which is what keeps the allocation gate deterministic and the
//! timings clean of its bookkeeping.

use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

/// The allocator the shipped app uses (`bb-ffi`), so a benchmark pays what the app pays.
static INNER: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub struct CountingAlloc;

static COUNTING: AtomicBool = AtomicBool::new(false);
static BYTES: AtomicU64 = AtomicU64::new(0);
static CALLS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub bytes: u64,
    pub calls: u64,
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Relaxed) {
            BYTES.fetch_add(layout.size() as u64, Relaxed);
            CALLS.fetch_add(1, Relaxed);
        }
        unsafe { INNER.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Relaxed) {
            BYTES.fetch_add(layout.size() as u64, Relaxed);
            CALLS.fetch_add(1, Relaxed);
        }
        unsafe { INNER.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { INNER.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A growing realloc allocates the difference; a shrinking one allocates nothing.
        if COUNTING.load(Relaxed) {
            BYTES.fetch_add(new_size.saturating_sub(layout.size()) as u64, Relaxed);
            CALLS.fetch_add(1, Relaxed);
        }
        unsafe { INNER.realloc(ptr, layout, new_size) }
    }
}

/// Runs `f` with counting on and returns what it allocated. Not reentrant, and counts every thread:
/// callers measure single-threaded work.
pub fn count<R>(f: impl FnOnce() -> R) -> (R, Counts) {
    // One counted region at a time, so concurrent tests cannot reset each other's counters.
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _gate = GATE.lock().unwrap_or_else(|e| e.into_inner());
    BYTES.store(0, Relaxed);
    CALLS.store(0, Relaxed);
    COUNTING.store(true, Relaxed);
    let result = f();
    COUNTING.store(false, Relaxed);
    let counts = Counts {
        bytes: BYTES.load(Relaxed),
        calls: CALLS.load(Relaxed),
    };
    (result, counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_what_a_closure_allocates() {
        let (v, counts) = count(|| std::hint::black_box(vec![0u8; 4096]));
        assert_eq!(v.len(), 4096);
        assert!(counts.bytes >= 4096 && counts.calls >= 1);
    }
}
