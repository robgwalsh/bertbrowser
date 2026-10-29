//! Where Windows says its known folders are today. `%USERPROFILE%\Documents` is only the default:
//! OneDrive's folder backup and a relocated Downloads move them, and the shell is the one that knows.
//! Spec: `Environment.GetFolderPath`, `Interop/KnownFolders`.

use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Profile,
    SHGetKnownFolderPath,
};
use windows_sys::core::GUID;

pub struct KnownFolders;

impl KnownFolders {
    pub fn profile() -> Option<String> {
        Self::path_of(&FOLDERID_Profile)
    }

    pub fn desktop() -> Option<String> {
        Self::path_of(&FOLDERID_Desktop)
    }

    pub fn documents() -> Option<String> {
        Self::path_of(&FOLDERID_Documents)
    }

    pub fn downloads() -> Option<String> {
        Self::path_of(&FOLDERID_Downloads)
    }

    /// The folder's current path, or None when Windows has none for it.
    pub fn path_of(id: &GUID) -> Option<String> {
        let mut buffer: *mut u16 = std::ptr::null_mut();
        // SAFETY: the out pointer is valid for the call; the returned buffer is freed below.
        let hr = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut buffer) };
        if hr < 0 || buffer.is_null() {
            if !buffer.is_null() {
                // SAFETY: allocated by the shell even on failure, and freed exactly once.
                unsafe { CoTaskMemFree(buffer as *const _) };
            }
            return None;
        }
        // SAFETY: a NUL-terminated string the shell allocated with CoTaskMemAlloc.
        let path = unsafe {
            let mut len = 0usize;
            while *buffer.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(buffer, len));
            CoTaskMemFree(buffer as *const _);
            text
        };
        (!path.is_empty()).then_some(path)
    }
}
