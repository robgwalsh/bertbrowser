//! Listing a directory, probing it for subfolders, and the ready drives.
//! Spec: Core/Services/FileSystemService.cs.

use std::io;
use std::os::windows::fs::MetadataExt;

use bb_paths::PathKey;
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

use crate::FileTime;
use crate::attributes::{DIRECTORY, HIDDEN};
use crate::models::FileEntry;

/// What a single subdirectory scan found: whether there is any subdirectory at all, and whether
/// there is one that isn't hidden. The folder tree needs both from one probe, because its expander
/// follows the "Show hidden items" setting and has to flip without re-reading disk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubdirectoryPresence {
    pub any: bool,
    pub any_visible: bool,
}

/// What `GetDriveType` says a root is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveType {
    Unknown,
    NoRootDirectory,
    Removable,
    Fixed,
    Network,
    CdRom,
    Ram,
}

/// A ready drive, as `DriveInfo` described it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriveInfo {
    /// The root, `C:\`.
    pub name: String,
    pub drive_type: DriveType,
    pub volume_label: String,
    /// The file system's name, `NTFS`.
    pub drive_format: String,
    pub total_size: u64,
    pub total_free_space: u64,
    pub available_free_space: u64,
}

pub trait FileSystemService: Send + Sync {
    /// Enumerates the direct children of a directory. Fails on access denied.
    fn list_directory(&self, path: &str) -> io::Result<Vec<FileEntry>>;

    /// Cheap probe for tree expanders: does this directory have subdirectories, hidden or
    /// otherwise. Stops at the first non-hidden one; only an all-hidden directory is scanned
    /// through. Inaccessible directories report nothing rather than failing.
    fn probe_subdirectories(&self, path: &str) -> SubdirectoryPresence;

    /// The drives that are ready.
    fn drives(&self) -> Vec<DriveInfo>;
}

/// The real one, over the local disk.
pub struct LocalFileSystem;

impl FileSystemService for LocalFileSystem {
    fn list_directory(&self, path: &str) -> io::Result<Vec<FileEntry>> {
        // Fully qualified, as the enumerator's `ToFullPath` gave it.
        let dir = PathKey::normalize_display(path)
            .unwrap_or_else(|_| path.trim_end_matches(['\\', '/']).to_string());
        let prefix = if dir.ends_with('\\') {
            dir
        } else {
            format!("{dir}\\")
        };
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            // Creation and access times come out of the same find data as everything else here,
            // so the columns that show them cost nothing over a listing that does not. Hidden and
            // system entries are shown, like a power tool should.
            let meta = entry.metadata()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            // The attribute bit: a junction is a directory, as the enumerator's `IsDirectory` said.
            let is_dir = meta.file_attributes() & DIRECTORY != 0;
            entries.push(FileEntry {
                full_path: format!("{prefix}{name}"),
                name,
                is_directory: is_dir,
                size_bytes: if is_dir { -1 } else { meta.file_size() as i64 },
                modified_utc: FileTime(meta.last_write_time() as i64),
                attributes: meta.file_attributes(),
                created_utc: FileTime(meta.creation_time() as i64),
                accessed_utc: FileTime(meta.last_access_time() as i64),
            });
        }
        Ok(entries)
    }

    fn probe_subdirectories(&self, path: &str) -> SubdirectoryPresence {
        let mut any = false;
        // The attributes come pre-populated from the scan, so the hidden test costs no extra stat
        // per subdirectory.
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries {
                let Ok(entry) = entry else {
                    break;
                };
                let Ok(meta) = entry.metadata() else {
                    break;
                };
                if meta.file_attributes() & DIRECTORY == 0 {
                    continue;
                }
                any = true;
                if meta.file_attributes() & HIDDEN == 0 {
                    return SubdirectoryPresence {
                        any: true,
                        any_visible: true,
                    };
                }
            }
        }
        SubdirectoryPresence {
            any,
            any_visible: false,
        }
    }

    fn drives(&self) -> Vec<DriveInfo> {
        // SAFETY: no arguments.
        let mask = unsafe { GetLogicalDrives() };
        let mut drives = Vec::new();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let name = format!("{}:\\", (b'A' + i as u8) as char);
            if let Some(drive) = describe(&name) {
                drives.push(drive);
            }
        }
        drives
    }
}

/// A drive that is ready: its volume answers. A CD drive with no disc, or a disconnected network
/// drive, is left out, as `DriveInfo.IsReady` left it out.
fn describe(root: &str) -> Option<DriveInfo> {
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut label = [0u16; 261];
    let mut format = [0u16; 261];
    let mut serial = 0u32;
    let mut max_component = 0u32;
    let mut flags = 0u32;
    // SAFETY: every pointer is valid for the call and the buffer lengths are passed with them.
    let ready = unsafe {
        GetVolumeInformationW(
            wide.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            &mut serial,
            &mut max_component,
            &mut flags,
            format.as_mut_ptr(),
            format.len() as u32,
        )
    };
    if ready == 0 {
        return None;
    }
    // SAFETY: a NUL-terminated root.
    let drive_type = match unsafe { GetDriveTypeW(wide.as_ptr()) } {
        1 => DriveType::NoRootDirectory,
        2 => DriveType::Removable,
        3 => DriveType::Fixed,
        4 => DriveType::Network,
        5 => DriveType::CdRom,
        6 => DriveType::Ram,
        _ => DriveType::Unknown,
    };
    let (mut available, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: out pointers are valid for the call.
    unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
    Some(DriveInfo {
        name: root.to_string(),
        drive_type,
        volume_label: wide_text(&label),
        drive_format: wide_text(&format),
        total_size: total,
        total_free_space: free,
        available_free_space: available,
    })
}

fn wide_text(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}
