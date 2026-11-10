//! Statuses, strings and the guard every export runs under, plus the safe accessors that hold the
//! crate's raw-pointer `unsafe`.

use super::*;

// ---- statuses and strings ----

/// The outcome of a call. Only `BB_OK` carries a result.
#[repr(C)]
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
    /// Another operation holds the app; try again once it finishes.
    BB_BUSY = 4,
}

/// A borrowed UTF-16 string view. Not NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbStr {
    pub ptr: *const u16,
    pub len: u32,
}

impl BbStr {
    /// cbindgen:ignore
    pub(crate) const EMPTY: BbStr = BbStr {
        ptr: ptr::null(),
        len: 0,
    };

    pub(crate) fn of(units: &[u16]) -> BbStr {
        BbStr {
            ptr: units.as_ptr(),
            len: units.len() as u32,
        }
    }

    /// Copies the view into a `String`. A view handed in across the C ABI is readable for the
    /// duration of the call (the contract on `BbStr`), which is what makes this safe to call.
    pub(crate) fn to_string_lossy(self) -> String {
        String::from_utf16_lossy(slice_of(self.ptr, self.len as usize))
    }
}

/// A UTF-16 string the caller owns. Free it with `bb_str_free`. `ptr == NULL` means none.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BbOwnedStr {
    pub ptr: *mut u16,
    pub len: u32,
}

impl BbOwnedStr {
    pub(crate) fn none() -> BbOwnedStr {
        BbOwnedStr {
            ptr: ptr::null_mut(),
            len: 0,
        }
    }

    pub(crate) fn from_str(s: &str) -> BbOwnedStr {
        let boxed: Box<[u16]> = s.encode_utf16().collect();
        let len = boxed.len() as u32;
        BbOwnedStr {
            ptr: Box::into_raw(boxed).cast::<u16>(),
            len,
        }
    }
}

// ---- errors and the guard every export runs under ----

thread_local! {
    pub(crate) static LAST_ERROR: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn set_last_error(message: &str) {
    LAST_ERROR.with(|e| {
        let mut e = e.borrow_mut();
        e.clear();
        e.extend(message.encode_utf16());
    });
}

pub(crate) fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".to_string()
    }
}

/// Why a call failed, before it becomes a status.
pub(crate) enum Fail {
    Err(String),
    Stale,
    Busy,
}

impl From<AppError> for Fail {
    fn from(e: AppError) -> Fail {
        match e {
            AppError::Stale => Fail::Stale,
            AppError::Busy => Fail::Busy,
            AppError::Invalid(m) => Fail::Err(m),
        }
    }
}

impl From<String> for Fail {
    fn from(s: String) -> Fail {
        Fail::Err(s)
    }
}

impl From<&str> for Fail {
    fn from(s: &str) -> Fail {
        Fail::Err(s.to_string())
    }
}

/// Runs `f` with the panic and error conventions every export follows.
pub(crate) fn guard(f: impl FnOnce() -> Result<(), Fail>) -> BbStatus {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => BbStatus::BB_OK,
        Ok(Err(Fail::Err(message))) => {
            set_last_error(&message);
            BbStatus::BB_ERR
        }
        Ok(Err(Fail::Stale)) => {
            set_last_error("stale id: the thing it names is gone");
            BbStatus::BB_STALE
        }
        Ok(Err(Fail::Busy)) => {
            set_last_error("busy: another operation is running");
            BbStatus::BB_BUSY
        }
        Err(payload) => {
            set_last_error(&panic_message(&payload));
            BbStatus::BB_PANIC
        }
    }
}

/// For the few exports that return a plain value rather than a status.
pub(crate) fn guard_value<T>(default: T, f: impl FnOnce() -> T) -> T {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(payload) => {
            set_last_error(&panic_message(&payload));
            default
        }
    }
}

/// The app handle. One per process.
pub struct BbApp {
    pub(crate) app: App,
    pub(crate) sink: SinkSlot,
}

/// The boundary accessors. Every pointer an export is given is the C++ caller's, and the ABI contract
/// (UI thread only, valid for the call) is what each `SAFETY` comment below leans on. They are safe
/// functions so an export body holds no `unsafe` of its own: the audited sites are these.
///
/// The app handle behind `p`, or an error for NULL.
pub(crate) fn app_of<'a>(p: *mut BbApp) -> Result<&'a BbApp, Fail> {
    // SAFETY: a non-null `BbApp*` was made by `bb_app_create` and stays valid until `bb_app_destroy`;
    // the caller makes no call after that.
    unsafe { p.as_ref() }.ok_or_else(|| Fail::Err("null app".into()))
}

/// A required out-parameter, or an error naming it for NULL.
pub(crate) fn out<'a, T>(p: *mut T, what: &str) -> Result<&'a mut T, Fail> {
    // SAFETY: the caller passes a pointer to writable storage of `T` that nothing else touches for the
    // call.
    unsafe { p.as_mut() }.ok_or_else(|| Fail::Err(format!("{what} is null")))
}

/// An optional out-parameter: `None` when the caller passed NULL.
pub(crate) fn opt_mut<'a, T>(p: *mut T) -> Option<&'a mut T> {
    // SAFETY: as `out`; NULL is allowed here.
    unsafe { p.as_mut() }
}

/// A shared reference through an optional pointer the caller owns (`None` for NULL).
pub(crate) fn opt_ref<'a, T>(p: *const T) -> Option<&'a T> {
    // SAFETY: a non-null pointer is a live `T` for the call.
    unsafe { p.as_ref() }
}

/// `n` elements at `ptr`, or an empty slice for NULL or zero.
pub(crate) fn slice_of<'a, T>(ptr: *const T, n: usize) -> &'a [T] {
    if ptr.is_null() || n == 0 {
        return &[];
    }
    // SAFETY: the caller passes `n` readable, initialised elements at `ptr` for the call.
    unsafe { std::slice::from_raw_parts(ptr, n) }
}

/// Copies `src` to the caller's buffer at `dst`, which holds at least `src.len()` elements.
pub(crate) fn copy_to<T: Copy>(src: &[T], dst: *mut T) {
    if dst.is_null() || src.is_empty() {
        return;
    }
    // SAFETY: the caller's buffer holds `src.len()` elements (each export clamps to its `cap`) and
    // cannot overlap Rust-owned `src`.
    unsafe { ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len()) };
}

/// Reads the NUL-terminated UTF-16 string at `p` (empty for NULL).
pub(crate) fn wide_cstr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    // SAFETY: the caller's string is NUL-terminated, so every read up to and including the NUL is in
    // bounds.
    while unsafe { *p.add(len) } != 0 {
        len += 1;
    }
    String::from_utf16_lossy(slice_of(p, len))
}

/// `n` strings at `ptr`.
pub(crate) fn strings(ptr: *const BbStr, n: u32) -> Vec<String> {
    slice_of(ptr, n as usize)
        .iter()
        .map(|s| s.to_string_lossy())
        .collect()
}

/// Writes `v` through an optional out-parameter.
pub(crate) fn set_opt<T>(p: *mut T, v: T) {
    if let Some(o) = opt_mut(p) {
        *o = v;
    }
}

/// Writes `text` into the required out-parameter `p` as an owned string.
pub(crate) fn put(p: *mut BbOwnedStr, text: &str) -> Result<(), Fail> {
    *out(p, "out_json")? = BbOwnedStr::from_str(text);
    Ok(())
}
