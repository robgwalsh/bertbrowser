//! Moving the bytes of one file, with progress and interruption.
//! Spec: Core/Services/Transfer/IFileCopier.cs, Core/Interop/CopyNative.cs.
//!
//! The trait is a seam for the same reason the probe is one: it is what lets the executor tests
//! drive a cancel that lands *in the middle of a file* deterministically and in milliseconds,
//! rather than by writing a multi-gigabyte fixture and racing it.
//!
//! **Why not a read/write loop.** Going through `CopyFileExW` / `MoveFileWithProgressW` is what
//! preserves sparse-file handling, server-side copy on SMB, and the exact attribute and timestamp
//! semantics the rest of the transfer code relies on. **Cancellation is the other half**: the
//! progress routine returns `PROGRESS_CANCEL` to stop mid-file, and both calls then remove the
//! partial destination themselves and leave the source untouched, which is exactly the guarantee
//! a cancelled transfer has to make.

use std::ffi::c_void;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{CopyFileExW, MoveFileWithProgressW};

use super::models::TransferError;
use crate::cancel::CancelToken;

/// Progress as (bytes so far, bytes in total), called as they land.
pub type ProgressFn<'a> = &'a mut dyn FnMut(i64, i64);

pub trait FileCopier: Send + Sync {
    /// Copies one file, never overwriting. A cancelled copy returns `TransferError::Cancelled` and
    /// leaves no partial destination behind.
    fn copy(
        &self,
        source: &str,
        destination: &str,
        progress: Option<ProgressFn<'_>>,
        cancel: &CancelToken,
    ) -> Result<(), TransferError>;

    /// Moves one file, never overwriting. Within a volume this is a rename and reports nothing;
    /// across volumes it copies under the same progress and cancellation contract. A cancelled
    /// move leaves no partial destination and leaves the source where it was.
    fn move_file(
        &self,
        source: &str,
        destination: &str,
        progress: Option<ProgressFn<'_>>,
        cancel: &CancelToken,
    ) -> Result<(), TransferError>;
}

// LPPROGRESS_ROUTINE return values.
const PROGRESS_CONTINUE: u32 = 0;
const PROGRESS_CANCEL: u32 = 1;
// dwCopyFlags.
const COPY_FILE_FAIL_IF_EXISTS: u32 = 0x0000_0001;
// dwFlags for MoveFileWithProgressW.
const MOVEFILE_COPY_ALLOWED: u32 = 0x0000_0002;
const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
// The error a PROGRESS_CANCEL comes back as.
const ERROR_REQUEST_ABORTED: i32 = 1235;

/// The real thing. Both entry points do their own cleanup on a cancel; the destination is deleted
/// defensively afterwards anyway, because "nothing half-written is left where a finished file
/// belongs" is worth more than one wasted stat. `MOVEFILE_COPY_ALLOWED` means the move spans
/// volumes by itself, so there is no not-same-device fallback to write for files.
#[derive(Default)]
pub struct FileSystemFileCopier;

impl FileCopier for FileSystemFileCopier {
    fn copy(
        &self,
        source: &str,
        destination: &str,
        progress: Option<ProgressFn<'_>>,
        cancel: &CancelToken,
    ) -> Result<(), TransferError> {
        let mut reporter = Reporter::new(progress, cancel);
        let mut cancel_flag: i32 = 0;
        let (src, dst) = (wide(&extended(source)), wide(&extended(destination)));
        // SAFETY: both paths are NUL-terminated and outlive the call; `reporter` outlives it too
        // and is only reached through the routine while the call is in progress.
        let ok = unsafe {
            CopyFileExW(
                src.as_ptr(),
                dst.as_ptr(),
                Some(routine),
                (&mut reporter as *mut Reporter<'_, '_>).cast::<c_void>(),
                &mut cancel_flag,
                COPY_FILE_FAIL_IF_EXISTS,
            )
        };
        let failure = io::Error::last_os_error();
        finish(ok != 0, failure, destination, &mut reporter, cancel)
    }

    fn move_file(
        &self,
        source: &str,
        destination: &str,
        progress: Option<ProgressFn<'_>>,
        cancel: &CancelToken,
    ) -> Result<(), TransferError> {
        let mut reporter = Reporter::new(progress, cancel);
        let (src, dst) = (wide(&extended(source)), wide(&extended(destination)));
        // SAFETY: as for `copy`.
        let ok = unsafe {
            MoveFileWithProgressW(
                src.as_ptr(),
                dst.as_ptr(),
                Some(routine),
                (&mut reporter as *mut Reporter<'_, '_>).cast::<c_void>(),
                MOVEFILE_COPY_ALLOWED | MOVEFILE_WRITE_THROUGH,
            )
        };
        let failure = io::Error::last_os_error();
        finish(ok != 0, failure, destination, &mut reporter, cancel)
    }
}

fn finish(
    ok: bool,
    failure: io::Error,
    destination: &str,
    reporter: &mut Reporter<'_, '_>,
    cancel: &CancelToken,
) -> Result<(), TransferError> {
    if ok {
        reporter.report_completion();
        return Ok(());
    }
    if failure.raw_os_error() == Some(ERROR_REQUEST_ABORTED) || cancel.is_cancelled() {
        // Both calls remove the partial destination themselves; make sure of it either way.
        try_delete(destination);
        return Err(TransferError::Cancelled);
    }
    // A raw OS error keeps its code, so `AccessDenied::caused` and the not-found kinds classify
    // it exactly as the translated .NET exception was.
    Err(TransferError::Io(failure))
}

/// Best effort: the source is intact, which is what matters.
fn try_delete(path: &str) {
    let _ = std::fs::remove_file(path);
}

/// Prefixes a long path with `\\?\`. The standard library's file APIs do this for us; the raw
/// Win32 calls do not, so without it a copy into a deep tree fails at MAX_PATH. The prefix turns
/// off path normalization, which is safe here because every path reaching this is already fully
/// qualified. It is two backslashes, a question mark and a backslash: written with one backslash
/// for a while, it made every long copy fail with "filename syntax is incorrect" and no short one.
pub(crate) fn extended(path: &str) -> String {
    if path.len() < 260 || path.starts_with(r"\\?\") {
        return path.to_string();
    }
    if let Some(rest) = path.strip_prefix(r"\\") {
        return format!(r"\\?\UNC\{rest}");
    }
    let bytes = path.as_bytes();
    let qualified =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    if qualified {
        format!(r"\\?\{path}")
    } else {
        path.to_string()
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Bridges the native callback to the closure the executor passes, and is where cancellation is
/// actually noticed: the routine is called per chunk, so returning `PROGRESS_CANCEL` is what
/// stops a copy part-way through a file.
struct Reporter<'p, 'c> {
    progress: Option<ProgressFn<'p>>,
    cancel: &'c CancelToken,
    total: i64,
    transferred: i64,
}

impl<'p, 'c> Reporter<'p, 'c> {
    fn new(progress: Option<ProgressFn<'p>>, cancel: &'c CancelToken) -> Reporter<'p, 'c> {
        Reporter {
            progress,
            cancel,
            total: 0,
            transferred: 0,
        }
    }

    /// A small file can finish having reported nothing, or having reported a figure short of the
    /// total. Either way the caller is owed the full count exactly once.
    fn report_completion(&mut self) {
        if self.total > self.transferred
            && let Some(progress) = self.progress.as_mut()
        {
            progress(self.total, self.total);
        }
    }

    fn on_progress(&mut self, total: i64, transferred: i64) -> u32 {
        if self.cancel.is_cancelled() {
            return PROGRESS_CANCEL;
        }
        self.total = total;
        self.transferred = transferred;
        if let Some(progress) = self.progress.as_mut() {
            progress(transferred, total);
        }
        PROGRESS_CONTINUE
    }
}

/// The callback both entry points take. Only the first two figures are used; the rest of the
/// signature has to be present for the stack to line up. The body runs under `catch_unwind`
/// because a panic must never unwind across the boundary into a call Windows is still inside: it
/// becomes a cancel, which both calls clean up after.
unsafe extern "system" fn routine(
    total: i64,
    transferred: i64,
    _stream_size: i64,
    _stream_transferred: i64,
    _stream_number: u32,
    _callback_reason: u32,
    _source: HANDLE,
    _destination: HANDLE,
    data: *const c_void,
) -> u32 {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `data` is the `Reporter` the calling `copy`/`move_file` passed, which outlives
        // the native call this routine is invoked from, and nothing else touches it meanwhile.
        let reporter = unsafe { &mut *(data as *mut Reporter<'_, '_>) };
        reporter.on_progress(total, transferred)
    }))
    .unwrap_or(PROGRESS_CANCEL)
}
