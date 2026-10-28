//! The real `VolumeAccess`: a raw `\\.\C:` handle and the USN FSCTLs. Every entry point here needs
//! an administrator token, which is why nothing here runs in the app itself: it all happens inside
//! `bb-indexer`, the one component shipping a requireAdministrator manifest. Spec:
//! Core/Interop/NtfsNative.cs and the `Open`/`QueryJournal`/`CreateJournal`/IOCTL parts of
//! MftVolumeIndexer.cs.
//!
//! **Untestable without elevation.** It compiles and is kept thin; `bb-indexer --dump-stats` is
//! what exercises it, run elevated by hand.

use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::fs::FileExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDriveTypeW, GetLogicalDrives,
    GetVolumeInformationW, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use super::reader::VolumeRead;
use super::volume_indexer::{JournalData, VolumeAccess, VolumeHandle};

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const DRIVE_FIXED: u32 = 3;

// FSCTL control codes (CTL_CODE, FILE_DEVICE_FILE_SYSTEM).
const FSCTL_QUERY_USN_JOURNAL: u32 = 0x0009_00F4;
const FSCTL_CREATE_USN_JOURNAL: u32 = 0x0009_00E7;
const FSCTL_ENUM_USN_DATA: u32 = 0x0009_00B3;
const FSCTL_READ_USN_JOURNAL: u32 = 0x0009_00BB;

// Error codes worth distinguishing from generic failure.
const ERROR_INVALID_FUNCTION: u32 = 1;
const ERROR_JOURNAL_NOT_ACTIVE: u32 = 1179;

#[repr(C)]
struct UsnJournalDataV0 {
    usn_journal_id: u64,
    first_usn: i64,
    next_usn: i64,
    lowest_valid_usn: i64,
    max_usn: i64,
    maximum_size: u64,
    allocation_delta: u64,
}

/// Input to FSCTL_ENUM_USN_DATA. The output starts with the next reference to resume from.
#[repr(C)]
struct MftEnumDataV0 {
    start_file_reference_number: u64,
    low_usn: i64,
    high_usn: i64,
}

/// Input to FSCTL_READ_USN_JOURNAL. The output starts with the next USN to resume from.
#[repr(C)]
struct ReadUsnJournalDataV0 {
    start_usn: i64,
    reason_mask: u32,
    return_only_on_close: u32,
    timeout: u64,
    bytes_to_wait_for: u64,
    usn_journal_id: u64,
}

#[repr(C)]
struct CreateUsnJournalData {
    maximum_size: u64,
    allocation_delta: u64,
}

/// Finds the fixed NTFS volumes and opens them raw.
pub struct RawVolumeAccess;

impl RawVolumeAccess {
    fn is_ntfs(root: &str) -> bool {
        let wide = to_wide(root);
        let mut fs_name = [0u16; 16];
        let mut serial = 0u32;
        let mut max_component = 0u32;
        let mut flags = 0u32;
        let ok = unsafe {
            GetVolumeInformationW(
                wide.as_ptr(),
                null_mut(),
                0,
                &mut serial,
                &mut max_component,
                &mut flags,
                fs_name.as_mut_ptr(),
                fs_name.len() as u32,
            )
        };
        if ok == 0 {
            return false;
        }
        let len = fs_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(fs_name.len());
        String::from_utf16_lossy(&fs_name[..len]).eq_ignore_ascii_case("NTFS")
    }
}

impl VolumeAccess for RawVolumeAccess {
    fn ntfs_volumes(&self) -> Vec<String> {
        let mask = unsafe { GetLogicalDrives() };
        let mut volumes = Vec::new();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let root = format!("{letter}:\\");
            if unsafe { GetDriveTypeW(to_wide(&root).as_ptr()) } != DRIVE_FIXED {
                continue;
            }
            if Self::is_ntfs(&root) {
                volumes.push(letter.to_string());
            }
        }
        volumes
    }

    fn open(&self, drive_letter: &str) -> Option<Box<dyn VolumeHandle>> {
        RawVolume::open(drive_letter).map(|v| Box::new(v) as Box<dyn VolumeHandle>)
    }
}

/// One open raw volume.
pub struct RawVolume {
    file: File,
    last_error: u32,
}

impl RawVolume {
    /// Opens `\\.\C:`. Read/write first, since creating a journal needs write; then read-only,
    /// because querying an existing journal (and reading the MFT) does not, and some volumes deny
    /// write handles.
    pub fn open(drive_letter: &str) -> Option<RawVolume> {
        let path = to_wide(&format!("\\\\.\\{drive_letter}:"));
        for access in [GENERIC_READ | GENERIC_WRITE, GENERIC_READ] {
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    access,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    null_mut(),
                    OPEN_EXISTING,
                    0,
                    null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE && !handle.is_null() {
                let owned = unsafe { OwnedHandle::from_raw_handle(handle.cast()) };
                return Some(RawVolume {
                    file: File::from(owned),
                    last_error: 0,
                });
            }
        }
        None
    }

    fn ioctl(
        &mut self,
        code: u32,
        input: *const c_void,
        input_size: u32,
        output: &mut [u8],
    ) -> Option<usize> {
        let mut returned = 0u32;
        let out_ptr: *mut c_void = if output.is_empty() {
            null_mut()
        } else {
            output.as_mut_ptr().cast()
        };
        let ok = unsafe {
            DeviceIoControl(
                self.file.as_raw_handle().cast(),
                code,
                input,
                input_size,
                out_ptr,
                output.len() as u32,
                &mut returned,
                null_mut(),
            )
        };
        if ok == 0 {
            self.last_error = unsafe { GetLastError() };
            return None;
        }
        self.last_error = 0;
        Some(returned as usize)
    }

    fn ioctl_struct<T>(&mut self, code: u32, input: &T, output: &mut [u8]) -> Option<usize> {
        self.ioctl(
            code,
            (input as *const T).cast(),
            std::mem::size_of::<T>() as u32,
            output,
        )
    }

    fn create_journal(&mut self) -> bool {
        let input = CreateUsnJournalData {
            maximum_size: 32 * 1024 * 1024,
            allocation_delta: 4 * 1024 * 1024,
        };
        self.ioctl_struct(FSCTL_CREATE_USN_JOURNAL, &input, &mut [])
            .is_some()
    }
}

impl VolumeRead for RawVolume {
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        let mut total = 0usize;
        while total < buffer.len() {
            let n = self
                .file
                .seek_read(&mut buffer[total..], offset + total as u64)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!(
                        "Unexpected EOF reading volume at offset {}.",
                        offset + total as u64
                    ),
                ));
            }
            total += n;
        }
        Ok(())
    }
}

impl VolumeHandle for RawVolume {
    fn query_journal(&mut self) -> Option<JournalData> {
        let mut buffer = vec![0u8; std::mem::size_of::<UsnJournalDataV0>()];
        if self
            .ioctl(FSCTL_QUERY_USN_JOURNAL, std::ptr::null(), 0, &mut buffer)
            .is_none()
        {
            let retry = matches!(
                self.last_error,
                ERROR_JOURNAL_NOT_ACTIVE | ERROR_INVALID_FUNCTION
            ) && self.create_journal();
            if !retry {
                return None;
            }
            self.ioctl(FSCTL_QUERY_USN_JOURNAL, std::ptr::null(), 0, &mut buffer)?;
        }
        // SAFETY: the buffer is exactly one UsnJournalDataV0, filled by the FSCTL.
        let data: UsnJournalDataV0 = unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast()) };
        Some(JournalData {
            journal_id: data.usn_journal_id,
            next_usn: data.next_usn,
            max_usn: data.max_usn,
        })
    }

    fn enum_usn_data(&mut self, start_frn: u64, high_usn: i64, buffer: &mut [u8]) -> Option<usize> {
        let input = MftEnumDataV0 {
            start_file_reference_number: start_frn,
            low_usn: 0,
            high_usn,
        };
        self.ioctl_struct(FSCTL_ENUM_USN_DATA, &input, buffer)
    }

    fn read_usn_journal(
        &mut self,
        start_usn: i64,
        journal_id: u64,
        buffer: &mut [u8],
    ) -> Option<usize> {
        let input = ReadUsnJournalDataV0 {
            start_usn,
            reason_mask: 0xFFFF_FFFF,
            return_only_on_close: 0,
            timeout: 0,
            bytes_to_wait_for: 0,
            usn_journal_id: journal_id,
        };
        self.ioctl_struct(FSCTL_READ_USN_JOURNAL, &input, buffer)
    }
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
