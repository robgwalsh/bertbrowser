//! The administrator-only copy of the index helper that the sign-in task runs. The task is
//! `HighestAvailable`, so whatever its `<Command>` names runs elevated with no prompt: it must live
//! where only administrators can write (`<Program Files>\BertBrowser`), never beside the app, which
//! Velopack installs somewhere a medium-integrity process can replace. The elevated helper in turn
//! never executes, loads or copies anything from the app's folder: it only *reads* the helper file
//! there, to prove this copy is the same build as the app (`check_app_directory`).
//!
//! The folder is resolved through the shell, never an environment variable: the environment of an
//! elevated helper comes from the unelevated process that launched it.

use std::io::Read;
use std::path::{Path, PathBuf};

use bb_paths::PathKey;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

/// The helper's file name, in the app's folder and in the installed one.
pub const HELPER_FILE_NAME: &str = "bb-indexer.exe";
/// The app's executable, which the helper expects to find on the other end of its pipe.
pub const APP_FILE_NAME: &str = "BertBrowser.exe";
/// The folder under Program Files that holds the installed copy.
pub const INSTALL_FOLDER_NAME: &str = "BertBrowser";

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_SHARING_VIOLATION: i32 = 32;

/// Why the helper may not run against an app folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppDirectoryFault {
    /// `BertBrowser.exe` is not there: uninstalled.
    AppMissing,
    /// The helper file in the app folder is missing, or is not byte-identical to this executable:
    /// the app was updated and this copy has not been re-registered.
    HelperDiffers,
}

pub struct InstalledHelper;

impl InstalledHelper {
    /// `<Program Files>` as Windows says it is, or None.
    pub fn program_files_directory() -> Option<PathBuf> {
        let mut buffer: *mut u16 = std::ptr::null_mut();
        // SAFETY: the out pointer is valid for the call; the returned buffer is freed below.
        let hr = unsafe {
            SHGetKnownFolderPath(
                &FOLDERID_ProgramFiles,
                KF_FLAG_DEFAULT as u32,
                std::ptr::null_mut(),
                &mut buffer,
            )
        };
        if buffer.is_null() {
            return None;
        }
        // SAFETY: a NUL-terminated string the shell allocated, freed exactly once.
        let text = unsafe {
            let mut len = 0usize;
            while *buffer.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(buffer, len));
            CoTaskMemFree(buffer as *const _);
            text
        };
        (hr >= 0 && !text.is_empty()).then(|| PathBuf::from(text))
    }

    /// The folder that holds the installed copy.
    pub fn folder_under(program_files: &Path) -> PathBuf {
        program_files.join(INSTALL_FOLDER_NAME)
    }

    /// The installed copy's path: what the task runs.
    pub fn path_under(program_files: &Path) -> PathBuf {
        Self::folder_under(program_files).join(HELPER_FILE_NAME)
    }

    /// The installed copy's path on this machine.
    pub fn installed_path() -> Option<PathBuf> {
        Self::program_files_directory().map(|pf| Self::path_under(&pf))
    }

    /// True when both files can be read and hold the same bytes. Any failure is "not identical".
    pub fn files_are_identical(a: &Path, b: &Path) -> bool {
        let (Ok(mut fa), Ok(mut fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
            return false;
        };
        match (fa.metadata(), fb.metadata()) {
            (Ok(ma), Ok(mb)) if ma.len() == mb.len() => {}
            _ => return false,
        }
        let mut buf_a = vec![0u8; 64 * 1024];
        let mut buf_b = vec![0u8; 64 * 1024];
        loop {
            let Ok(n) = read_full(&mut fa, &mut buf_a) else {
                return false;
            };
            let Ok(m) = read_full(&mut fb, &mut buf_b) else {
                return false;
            };
            if n != m || buf_a[..n] != buf_b[..m] {
                return false;
            }
            if n == 0 {
                return true;
            }
        }
    }

    /// The same-build gate. Only reads the app folder: `own_exe` is compared byte for byte with
    /// `<app_dir>\bb-indexer.exe` when that is not the folder `own_exe` lives in.
    pub fn check_app_directory(own_exe: &Path, app_dir: &Path) -> Result<(), AppDirectoryFault> {
        if !app_dir.join(APP_FILE_NAME).is_file() {
            return Err(AppDirectoryFault::AppMissing);
        }
        if !Self::same_directory(own_exe.parent(), Some(app_dir))
            && !Self::files_are_identical(own_exe, &app_dir.join(HELPER_FILE_NAME))
        {
            return Err(AppDirectoryFault::HelperDiffers);
        }
        Ok(())
    }

    /// Whether two directories are the same place, by canonical key.
    pub fn same_directory(a: Option<&Path>, b: Option<&Path>) -> bool {
        let (Some(a), Some(b)) = (a, b) else {
            return false;
        };
        matches!(
            (
                PathKey::canonicalize(&a.to_string_lossy()),
                PathKey::canonicalize(&b.to_string_lossy())
            ),
            (Ok(x), Ok(y)) if x == y
        )
    }

    /// Puts a copy of `source` at `<folder>\bb-indexer.exe`, creating the folder. Refuses a folder
    /// or an existing target that is a reparse point. Skips the copy when the target already holds
    /// the same bytes. Otherwise copies beside it under a temporary name and replaces; when the
    /// target is running (its image is locked) the old file is renamed aside to `.old` first.
    pub fn install(source: &Path, folder: &Path) -> Result<PathBuf, String> {
        if !folder.exists() {
            std::fs::create_dir(folder)
                .map_err(|e| format!("Cannot create {}: {e}", folder.display()))?;
        }
        if is_reparse_point(folder) {
            return Err(format!(
                "{} is a link, which will not be followed.",
                folder.display()
            ));
        }
        if !folder.is_dir() {
            return Err(format!("{} is not a folder.", folder.display()));
        }
        let target = folder.join(HELPER_FILE_NAME);
        if is_reparse_point(&target) {
            return Err(format!(
                "{} is a link, which will not be followed.",
                target.display()
            ));
        }
        if target.exists() && Self::files_are_identical(source, &target) {
            return Ok(target);
        }

        let temp = folder.join(format!("{HELPER_FILE_NAME}.{}.new", std::process::id()));
        let old = folder.join(format!("{HELPER_FILE_NAME}.old"));
        let _ = std::fs::remove_file(&temp);
        if let Err(e) = std::fs::copy(source, &temp) {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("Cannot copy the helper to {}: {e}", temp.display()));
        }
        if !Self::files_are_identical(source, &temp) {
            let _ = std::fs::remove_file(&temp);
            return Err("The copied helper does not match its source.".to_string());
        }

        match std::fs::rename(&temp, &target) {
            Ok(()) => Ok(target),
            Err(e)
                if matches!(
                    e.raw_os_error(),
                    Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION)
                ) =>
            {
                // The installed helper is running. A running image cannot be overwritten or
                // deleted, but it can be renamed.
                let _ = std::fs::remove_file(&old);
                if is_reparse_point(&old) {
                    let _ = std::fs::remove_file(&temp);
                    return Err(format!("{} is a link.", old.display()));
                }
                let aside = std::fs::rename(&target, &old);
                let moved = aside.and_then(|()| std::fs::rename(&temp, &target));
                match moved {
                    Ok(()) => {
                        let _ = std::fs::remove_file(&old);
                        Ok(target)
                    }
                    Err(e) => {
                        if !target.exists() && old.exists() {
                            let _ = std::fs::rename(&old, &target);
                        }
                        let _ = std::fs::remove_file(&temp);
                        Err(format!("Cannot replace {}: {e}", target.display()))
                    }
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&temp);
                Err(format!("Cannot replace {}: {e}", target.display()))
            }
        }
    }

    /// Best effort: deletes the installed copy and its `.old`, then the folder if it is empty.
    pub fn remove(folder: &Path) {
        let _ = std::fs::remove_file(folder.join(HELPER_FILE_NAME));
        let _ = std::fs::remove_file(folder.join(format!("{HELPER_FILE_NAME}.old")));
        // Non-recursive: only an empty folder goes.
        let _ = std::fs::remove_dir(folder);
    }
}

fn read_full(file: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

/// True for a symlink or junction. A path that does not exist is not one.
fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}
