//! The questions the planner and executor ask about what is there, behind traits so the rules that
//! stop a delete reaching the wrong thing can be tested against layouts nobody should create for
//! real. Spec: Core/Services/Delete/IDeleteProbe.cs, IRecycleProbe.cs.

use windows_sys::Win32::Storage::FileSystem::{
    GetFileAttributesExW, GetFileExInfoStandard, WIN32_FILE_ATTRIBUTE_DATA,
};

use crate::attributes::DIRECTORY;

/// The filesystem questions the delete planner and executor ask about what is there.
pub trait DeleteProbe {
    fn directory_exists(&self, path: &str) -> bool;

    fn file_exists(&self, path: &str) -> bool;
}

/// Real-filesystem [`DeleteProbe`]. The entry's own attributes, as `Directory.Exists` read them:
/// a junction is a directory whether or not its target is.
pub struct FileSystemDeleteProbe;

impl DeleteProbe for FileSystemDeleteProbe {
    fn directory_exists(&self, path: &str) -> bool {
        EntryAttributes::of(path).is_some_and(|e| e.is_directory())
    }

    fn file_exists(&self, path: &str) -> bool {
        EntryAttributes::of(path).is_some_and(|e| !e.is_directory())
    }
}

/// What `GetFileAttributesEx` says about an entry, which is what `File.Exists`, `FileInfo.Length`
/// and `Directory.Exists` were built on. **Asked without opening the entry**, and that is the
/// point: `std::fs::metadata` opens a handle first, and a file whose folder carries a Deny ACE
/// answers that open with "access denied" — so the one file the elevated retry exists for would
/// be planned as "no longer exists" instead. The entry's own attributes, never its target's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EntryAttributes {
    pub attributes: u32,
    pub size: u64,
}

impl EntryAttributes {
    pub(crate) fn of(path: &str) -> Option<EntryAttributes> {
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: a NUL-terminated path and an out struct of the size the level names.
        let data = unsafe {
            let mut data: WIN32_FILE_ATTRIBUTE_DATA = std::mem::zeroed();
            if GetFileAttributesExW(
                wide.as_ptr(),
                GetFileExInfoStandard,
                (&mut data as *mut WIN32_FILE_ATTRIBUTE_DATA).cast(),
            ) == 0
            {
                return None;
            }
            data
        };
        Some(EntryAttributes {
            attributes: data.dwFileAttributes,
            size: ((data.nFileSizeHigh as u64) << 32) | data.nFileSizeLow as u64,
        })
    }

    pub(crate) fn is_directory(self) -> bool {
        self.attributes & DIRECTORY != 0
    }
}

/// Whether a path's volume has a Recycle Bin that will actually take something.
///
/// Asked by the planner so the confirmation can be honest, and again by the executor against live
/// state. It is a separate trait from [`DeleteProbe`] because the answer comes from the shell
/// rather than the filesystem, and because the rule that routes an item to the holding folder
/// instead needs testing against volumes — network shares, media with the bin disabled — that
/// cannot be conjured up in a unit test.
pub trait RecycleProbe {
    /// False for a network share, a volume whose bin is turned off, or anything else the shell
    /// would silently erase rather than hold.
    fn can_recycle(&self, path: &str) -> bool;
}

/// The answer when there is no Recycle Bin to ask about — Core on its own, and any test that has
/// not opted in. Everything routes to the holding folder, which is the behaviour that predates
/// the bin and never loses data.
pub struct NoRecycleProbe;

impl NoRecycleProbe {
    pub const INSTANCE: &'static NoRecycleProbe = &NoRecycleProbe;
}

impl RecycleProbe for NoRecycleProbe {
    fn can_recycle(&self, _path: &str) -> bool {
        false
    }
}
