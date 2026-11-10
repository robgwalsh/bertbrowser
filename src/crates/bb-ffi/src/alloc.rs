//! The global allocator and the byte count behind `bb_allocator_*`.

use super::*;
use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

// The process heap is slow for the many small allocations parsing, planning and listing make:
// measured at twice the time of mimalloc across the Tier A suite. Everything Rust allocates is
// freed by Rust (`bb_str_free`, `bb_event_free`), so the C++ side never sees which heap it is.
#[global_allocator]
pub(crate) static ALLOCATOR: CountedMimalloc = CountedMimalloc;

pub(crate) static COUNTING: AtomicBool = AtomicBool::new(false);
pub(crate) static HELD: AtomicI64 = AtomicI64::new(0);

/// mimalloc, and when a measurement has asked, the bytes Rust holds. mimalloc cannot say: its counters want a
/// stats build, and on Windows `mi_process_info`'s "commit" is the whole process's private bytes. Off, this is
/// one relaxed load per call.
pub(crate) struct CountedMimalloc;

pub(crate) fn note_held(bytes: i64) {
    if COUNTING.load(Ordering::Relaxed) {
        HELD.fetch_add(bytes, Ordering::Relaxed);
    }
}

// SAFETY: every call goes straight to mimalloc; the count beside it touches nothing it allocates.
unsafe impl GlobalAlloc for CountedMimalloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller of `alloc` upholds `GlobalAlloc`'s contract, which mimalloc shares.
        let block = unsafe { mimalloc::MiMalloc.alloc(layout) };
        if !block.is_null() {
            note_held(layout.size() as i64);
        }
        block
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller of `alloc_zeroed` upholds `GlobalAlloc`'s contract, which mimalloc shares.
        let block = unsafe { mimalloc::MiMalloc.alloc_zeroed(layout) };
        if !block.is_null() {
            note_held(layout.size() as i64);
        }
        block
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: the caller of `dealloc` upholds `GlobalAlloc`'s contract, which mimalloc shares.
        unsafe { mimalloc::MiMalloc.dealloc(block, layout) };
        note_held(-(layout.size() as i64));
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller of `realloc` upholds `GlobalAlloc`'s contract, which mimalloc shares.
        let moved = unsafe { mimalloc::MiMalloc.realloc(block, layout, new_size) };
        if !moved.is_null() {
            note_held(new_size as i64 - layout.size() as i64);
        }
        moved
    }
}

pub(crate) fn start_counting_allocations() {
    COUNTING.store(true, Ordering::Relaxed);
}

/// The bytes Rust holds, since counting started. `collect` has mimalloc hand back what this thread can first,
/// which leaves this number alone and is for the process's private bytes read after it.
pub(crate) fn allocator_held(collect: bool) -> i64 {
    if collect {
        // SAFETY: takes nothing and may be called at any time.
        unsafe { libmimalloc_sys::mi_collect(true) };
    }
    HELD.load(Ordering::Relaxed)
}

/// Starts counting the bytes Rust holds, for a measured run. First thing in `main`, since what was allocated
/// before it is not counted and its later free would be.
#[unsafe(no_mangle)]
pub extern "C" fn bb_allocator_start_counting() {
    start_counting_allocations();
}

/// The bytes Rust holds, after mimalloc has handed back what it can when `collect`. Zero unless
/// `bb_allocator_start_counting` was called.
#[unsafe(no_mangle)]
pub extern "C" fn bb_allocator_held(collect: bool) -> i64 {
    guard_value(0, || allocator_held(collect))
}
