//! The C ABI between the Rust core and the C++ UI. cbindgen reads this crate alone and writes
//! `native/include/bbcore.h`.
//!
//! Rules that every export keeps:
//! - Every `bb_*` call is made from the UI thread, and none blocks on long work. Results arrive as
//!   events through the sink.
//! - The sink is invoked on Rust threads and **may only enqueue**. Rust never re-enters C++ any other
//!   way.
//! - Every export runs under `catch_unwind`: a panic becomes `BB_PANIC`, never an unwind across the
//!   boundary. The message is readable with `bb_last_error` on the same thread.
//! - Strings cross as UTF-16. `BbStr` is a borrowed view valid for the call (in) or for the documented
//!   lifetime of the owner (out). `BbOwnedStr` is freed with `bb_str_free`.
//! - Events are Rust-owned; the receiver frees each with `bb_event_free` once it is done with it.

#![allow(clippy::missing_safety_doc)]

use std::any::Any;
use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::ptr;
use std::sync::OnceLock;

/// The outcome of a call. Only `BB_OK` carries a result.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BbStatus {
    /// The call succeeded.
    BB_OK = 0,
    /// The call failed; `bb_last_error` has the message.
    BB_ERR = 1,
    /// Rust panicked inside the call; `bb_last_error` has the panic message.
    BB_PANIC = 2,
    /// A handle or id named something that no longer exists.
    BB_STALE = 3,
}

/// A borrowed UTF-16 string view. Not NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbStr {
    pub ptr: *const u16,
    pub len: u32,
}

impl BbStr {
    pub const EMPTY: BbStr = BbStr {
        ptr: ptr::null(),
        len: 0,
    };

    fn of(units: &[u16]) -> BbStr {
        BbStr {
            ptr: units.as_ptr(),
            len: units.len() as u32,
        }
    }

    /// Copies the view into a `String`. The view must be readable for the duration of the call.
    unsafe fn to_string_lossy(self) -> String {
        if self.ptr.is_null() || self.len == 0 {
            return String::new();
        }
        let units = unsafe { std::slice::from_raw_parts(self.ptr, self.len as usize) };
        String::from_utf16_lossy(units)
    }
}

/// A UTF-16 string the caller owns. Free it with `bb_str_free`.
#[repr(C)]
pub struct BbOwnedStr {
    pub ptr: *mut u16,
    pub len: u32,
}

impl BbOwnedStr {
    fn from_str(s: &str) -> BbOwnedStr {
        let boxed: Box<[u16]> = s.encode_utf16().collect();
        let len = boxed.len() as u32;
        BbOwnedStr {
            ptr: Box::into_raw(boxed).cast::<u16>(),
            len,
        }
    }
}

/// An event delivered to the sink. Rust owns it; call `bb_event_free` when done. `text` is valid
/// until then.
#[repr(C)]
pub struct BbEvent {
    pub kind: u32,
    pub id: u64,
    pub a: u64,
    pub b: u64,
    pub text: BbStr,
    pub payload: *const c_void,
}

/// The receiver of events. Called on Rust threads: enqueue and return, do nothing else.
pub type BbEventSink = Option<unsafe extern "C" fn(ctx: *mut c_void, evt: *mut BbEvent)>;

#[repr(C)]
struct EventBox {
    event: BbEvent,
    text: Vec<u16>,
}

fn emit(sink: BbEventSink, ctx: usize, kind: u32, id: u64, a: u64, b: u64, text: &str) {
    let Some(sink) = sink else { return };
    let mut boxed = Box::new(EventBox {
        event: BbEvent {
            kind,
            id,
            a,
            b,
            text: BbStr::EMPTY,
            payload: ptr::null(),
        },
        text: text.encode_utf16().collect(),
    });
    boxed.event.text = BbStr::of(&boxed.text);
    let raw = Box::into_raw(boxed);
    // EventBox is repr(C) with `event` first, so a pointer to the box is a pointer to the event.
    unsafe { sink(ctx as *mut c_void, raw.cast::<BbEvent>()) };
}

thread_local! {
    static LAST_ERROR: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
}

fn set_last_error(message: &str) {
    LAST_ERROR.with(|e| {
        let mut e = e.borrow_mut();
        e.clear();
        e.extend(message.encode_utf16());
    });
}

fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".to_string()
    }
}

/// Runs `f` with the panic and error conventions every export follows.
fn guard(f: impl FnOnce() -> Result<(), String>) -> BbStatus {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => BbStatus::BB_OK,
        Ok(Err(message)) => {
            set_last_error(&message);
            BbStatus::BB_ERR
        }
        Err(payload) => {
            set_last_error(&panic_message(&payload));
            BbStatus::BB_PANIC
        }
    }
}

/// The core version, as a view into static storage.
#[unsafe(no_mangle)]
pub extern "C" fn bb_version() -> BbStr {
    static VERSION: OnceLock<Vec<u16>> = OnceLock::new();
    BbStr::of(VERSION.get_or_init(|| env!("CARGO_PKG_VERSION").encode_utf16().collect()))
}

/// The message behind the last `BB_ERR` or `BB_PANIC` on this thread. The view is valid until the next
/// `bb_*` call on the same thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_last_error(out: *mut BbStr) {
    if out.is_null() {
        return;
    }
    let view = LAST_ERROR.with(|e| BbStr::of(&e.borrow()));
    unsafe { out.write(view) };
}

/// Frees a string returned as `BbOwnedStr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_str_free(s: BbOwnedStr) {
    if s.ptr.is_null() {
        return;
    }
    let slice = ptr::slice_from_raw_parts_mut(s.ptr, s.len as usize);
    drop(unsafe { Box::from_raw(slice) });
}

/// Frees an event once the receiver is done with it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_event_free(evt: *mut BbEvent) {
    if evt.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(evt.cast::<EventBox>()) });
}

// ---- Spike S7: proves the link, the sink, the string ownership and the panic rule. Removed after M5.

/// Emits `n` events of kind 1 (id = index, text = "event <index>") from a Rust thread, then one of
/// kind 2 with text "done". Returns at once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_spike_emit(sink: BbEventSink, ctx: *mut c_void, n: u32) -> BbStatus {
    let ctx = ctx as usize;
    guard(move || {
        std::thread::Builder::new()
            .name("bb-spike".into())
            .spawn(move || {
                for i in 0..n {
                    emit(sink, ctx, 1, u64::from(i), 0, 0, &format!("event {i}"));
                }
                emit(sink, ctx, 2, u64::from(n), 0, 0, "done");
            })
            .map(|_| ())
            .map_err(|e| format!("spawn failed: {e}"))
    })
}

/// Upper-cases `input` into a string the caller owns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bb_spike_upper(input: BbStr, out: *mut BbOwnedStr) -> BbStatus {
    guard(|| {
        if out.is_null() {
            return Err("out is null".into());
        }
        let text = unsafe { input.to_string_lossy() };
        unsafe { out.write(BbOwnedStr::from_str(&text.to_uppercase())) };
        Ok(())
    })
}

/// Always fails with a message, to prove `BB_ERR` and `bb_last_error`.
#[unsafe(no_mangle)]
pub extern "C" fn bb_spike_fail() -> BbStatus {
    guard(|| Err("spike: deliberate failure".into()))
}

/// Always panics, to prove `BB_PANIC` and that nothing unwinds across the boundary.
#[unsafe(no_mangle)]
pub extern "C" fn bb_spike_panic() -> BbStatus {
    guard(|| panic!("spike: deliberate panic"))
}
