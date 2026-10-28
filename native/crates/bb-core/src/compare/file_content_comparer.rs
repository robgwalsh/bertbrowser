//! Comparing two files by their bytes. Spec: Core/Services/Compare/FileContentComparer.cs.
//!
//! This module already owns "are these the same", and the folder comparison is the caller that
//! matters most: a timestamp says two files are probably different, and only this can say they
//! are not. So the rule it has to keep is the one `CompareVerdict::Same` states: every doubt
//! resolves away from a match. A read that fails is `Unreadable`, never a verdict. Opening goes
//! through `ReadOnlyFile`, the one opener: this is the third caller of the share-flag and
//! placeholder rules, and a third copy of them is a third chance for one to drift.

use std::fs::File;
use std::io::Read;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
};

use crate::cancel::{CancelToken, Cancelled};
use crate::fs::ReadOnlyFile;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContentVerdict {
    /// Byte for byte the same.
    Identical,
    /// Not the same. [`ContentComparison::first_difference_offset`] says where.
    Differs,
    /// A side could not be read. Never `Identical` by omission: this is the verdict that must not
    /// be mistaken for a match, because a match is what authorises a delete.
    Unreadable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentComparison {
    pub verdict: ContentVerdict,
    /// Where the two first disagree, or, when one is a prefix of the other, where the shorter
    /// ends. None only when they are identical.
    pub first_difference_offset: Option<i64>,
    pub left_bytes: i64,
    pub right_bytes: i64,
}

impl ContentComparison {
    pub fn identical(&self) -> bool {
        self.verdict == ContentVerdict::Identical
    }

    fn unreadable(left_bytes: i64, right_bytes: i64) -> ContentComparison {
        ContentComparison {
            verdict: ContentVerdict::Unreadable,
            first_difference_offset: None,
            left_bytes,
            right_bytes,
        }
    }
}

/// Comparing two files by their bytes. `progress` is told each chunk's byte count as it is
/// compared; a cancelled token ends the comparison with `Err(Cancelled)`.
pub trait ContentComparer {
    fn compare(
        &self,
        left_path: &str,
        right_path: &str,
        progress: Option<&dyn Fn(i64)>,
        cancel: &CancelToken,
    ) -> Result<ContentComparison, Cancelled>;
}

/// The real thing, over [`ReadOnlyFile`].
#[derive(Default)]
pub struct FileContentComparer;

impl FileContentComparer {
    const BUFFER_BYTES: usize = 1024 * 1024;

    fn same_file(left: &File, right: &File) -> bool {
        match (Self::identity(left), Self::identity(right)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    /// The volume serial and file index a handle already knows, or None when the filesystem will
    /// not say, in which case the file is treated as its own thing and read.
    fn identity(file: &File) -> Option<(u32, u64)> {
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is open for the file's lifetime and the out pointer is valid.
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
        if ok == 0 {
            return None;
        }
        Some((
            info.dwVolumeSerialNumber,
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ))
    }

    fn length(file: &File) -> i64 {
        file.metadata().map(|m| m.len() as i64).unwrap_or(0)
    }

    fn compare_bytes(
        mut left: File,
        mut right: File,
        progress: Option<&dyn Fn(i64)>,
        cancel: &CancelToken,
    ) -> Result<ContentComparison, Cancelled> {
        let left_length = Self::length(&left);
        let right_length = Self::length(&right);

        let mut a = vec![0u8; Self::BUFFER_BYTES];
        let mut b = vec![0u8; Self::BUFFER_BYTES];
        let mut offset: i64 = 0;

        loop {
            cancel.check()?;

            // Failing part-way through cannot produce a verdict either way: the bytes read so far
            // being equal is not evidence that the rest are.
            let (Ok(got_a), Ok(got_b)) = (
                Self::read_at_least(&mut left, &mut a),
                Self::read_at_least(&mut right, &mut b),
            ) else {
                return Ok(ContentComparison::unreadable(left_length, right_length));
            };

            let common = got_a.min(got_b);
            let span_a = &a[..common];
            let span_b = &b[..common];

            if let Some(first) = span_a.iter().zip(span_b).position(|(x, y)| x != y) {
                return Ok(ContentComparison {
                    verdict: ContentVerdict::Differs,
                    first_difference_offset: Some(offset + first as i64),
                    left_bytes: left_length,
                    right_bytes: right_length,
                });
            }

            // One ran out before the other: a prefix. The offset is where the shorter ended, which
            // is a more useful thing to say than "the sizes differ".
            if got_a != got_b {
                return Ok(ContentComparison {
                    verdict: ContentVerdict::Differs,
                    first_difference_offset: Some(offset + common as i64),
                    left_bytes: left_length,
                    right_bytes: right_length,
                });
            }

            if got_a == 0 {
                break;
            }

            offset += got_a as i64;
            if let Some(report) = progress {
                report(got_a as i64);
            }
        }

        Ok(ContentComparison {
            verdict: ContentVerdict::Identical,
            first_difference_offset: None,
            left_bytes: left_length,
            right_bytes: right_length,
        })
    }

    /// Fills `buffer` unless the file ends first; the count read.
    fn read_at_least(file: &mut File, buffer: &mut [u8]) -> std::io::Result<usize> {
        let mut filled = 0;
        while filled < buffer.len() {
            match file.read(&mut buffer[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(filled)
    }
}

impl ContentComparer for FileContentComparer {
    fn compare(
        &self,
        left_path: &str,
        right_path: &str,
        progress: Option<&dyn Fn(i64)>,
        cancel: &CancelToken,
    ) -> Result<ContentComparison, Cancelled> {
        cancel.check()?;

        let Some(left) = ReadOnlyFile::try_open(left_path, false) else {
            return Ok(ContentComparison::unreadable(0, 0));
        };

        let Some(right) = ReadOnlyFile::try_open(right_path, false) else {
            return Ok(ContentComparison::unreadable(Self::length(&left), 0));
        };

        // The same file under two names: a hard link, or the same path twice. A handle already
        // knows this, and confirming it by reading two identical gigabytes would be minutes spent
        // learning what Windows was willing to say immediately.
        if Self::same_file(&left, &right) {
            return Ok(ContentComparison {
                verdict: ContentVerdict::Identical,
                first_difference_offset: None,
                left_bytes: Self::length(&left),
                right_bytes: Self::length(&right),
            });
        }

        Self::compare_bytes(left, right, progress, cancel)
    }
}
