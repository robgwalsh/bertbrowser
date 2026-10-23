//! Swapping a rewritten file in for the one it replaces, keeping the old one beside it.
//! Spec: Core/Services/StagedReplace.cs.
//!
//! **`ReplaceFile`, not two renames.** A file written fresh beside the original is a new file: it
//! has today's creation time, the folder's inherited permissions instead of its own, none of its
//! alternate streams (the mark that says a download came from the internet is one) and none of its
//! attributes. `ReplaceFile` carries the replaced file's identity onto the replacement and leaves
//! the old content under the backup name in the same step, which is what an undo needs. Other hard
//! links keep pointing at the old content, which is then the held copy.

use std::hash::{BuildHasher, RandomState};
use std::io;

use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW};

pub struct StagedReplace;

impl StagedReplace {
    /// A working name beside `path`, with the marker *before* the extension:
    /// `a.bertbrowser-rewrite-1234abcd.jpg` is still a picture to anything that goes by the name,
    /// and `a.jpg.bertbrowser-rewrite-1234abcd` is not.
    pub fn beside(path: &str, marker: &str) -> String {
        let (directory, file_name) = bb_paths::split_last(path);
        let (stem, extension) = match file_name.rfind('.') {
            Some(dot) if dot > 0 => (&file_name[..dot], &file_name[dot..]),
            _ => (file_name, ""),
        };
        let nonce = RandomState::new().hash_one(0u8) as u32;
        let name = format!("{stem}{marker}{nonce:08x}{extension}");
        if directory.is_empty() {
            name
        } else {
            format!("{directory}\\{name}")
        }
    }

    /// Puts `replacement` where `target` is and leaves what was there at `backup`. On failure the
    /// target is still in place.
    pub fn swap(replacement: &str, target: &str, backup: &str) -> io::Result<()> {
        let ok = unsafe {
            // SAFETY: three NUL-terminated paths, valid for the call.
            ReplaceFileW(
                wide(target).as_ptr(),
                wide(replacement).as_ptr(),
                wide(backup).as_ptr(),
                REPLACEFILE_IGNORE_MERGE_ERRORS,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if ok != 0 {
            return Ok(());
        }
        let failure = io::Error::last_os_error();

        // One of ReplaceFile's failures leaves the target already renamed to the backup and nothing
        // under its own name. Put it back before letting the failure out: a file that has vanished
        // because the second half of a swap failed is the outcome this must not have. The caller's
        // message names the backup if even that fails.
        if !std::fs::symlink_metadata(target).is_ok() && std::fs::symlink_metadata(backup).is_ok() {
            let _ = std::fs::rename(backup, target);
        }
        Err(failure)
    }

    /// Whether a file name carries one of this app's working markers.
    pub fn is_marked(path: &str, marker: &str) -> bool {
        bb_paths::file_name(path).contains(marker)
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
