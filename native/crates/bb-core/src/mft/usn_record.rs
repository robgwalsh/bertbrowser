//! One parsed USN_RECORD_V2, the shape both the initial MFT enumeration (FSCTL_ENUM_USN_DATA) and
//! the live journal tail (FSCTL_READ_USN_JOURNAL) return, and the decoder over the raw buffer an
//! FSCTL fills. Records with an unexpected major version (V3/V4 on ReFS) are skipped by record
//! length rather than misparsed; only NTFS volumes are ever targeted. Spec:
//! Core/Services/Mft/UsnRecord.cs.

use super::ntfs_layout::{i64_at, u16_at, u32_at, u64_at, utf16_at};
use super::reader::file_time_or_unknown;
use crate::FileTime;
use crate::attributes::{DIRECTORY, HIDDEN, REPARSE_POINT};

/// `reason`, `usn` and `timestamp_utc` are only meaningful for journal records; the enumeration
/// leaves them 0 / unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsnRecord {
    pub file_reference_number: u64,
    pub parent_file_reference_number: u64,
    pub reason: u32,
    pub attributes: u32,
    pub name: String,
    pub usn: i64,
    pub timestamp_utc: FileTime,
}

impl UsnRecord {
    pub fn is_directory(&self) -> bool {
        self.attributes & DIRECTORY != 0
    }

    pub fn is_hidden(&self) -> bool {
        self.attributes & HIDDEN != 0
    }

    pub fn is_reparse_point(&self) -> bool {
        self.attributes & REPARSE_POINT != 0
    }
}

pub struct UsnRecordParser;

impl UsnRecordParser {
    // USN_RECORD_V2 field offsets (bytes from the start of each record).
    const RECORD_LENGTH: usize = 0;
    const MAJOR_VERSION: usize = 4;
    const FILE_REFERENCE_NUMBER: usize = 8;
    const PARENT_FILE_REFERENCE_NUMBER: usize = 16;
    const USN: usize = 24;
    const TIME_STAMP: usize = 32;
    const REASON: usize = 40;
    const FILE_ATTRIBUTES: usize = 52;
    const FILE_NAME_LENGTH: usize = 56;
    const FILE_NAME_OFFSET: usize = 58;
    const MIN_RECORD_LENGTH: usize = 60;

    /// The records in `buffer[start..end]`. FSCTL_ENUM_USN_DATA / FSCTL_READ_USN_JOURNAL prefix
    /// their output with an 8-byte "next reference / next USN" cursor, so callers pass `start = 8`
    /// and `end = bytes_returned`. A truncated or corrupt tail stops the walk rather than misreading.
    pub fn parse(buffer: &[u8], start: usize, end: usize) -> Vec<UsnRecord> {
        let mut records = Vec::new();
        let end = end.min(buffer.len());
        let mut offset = start;
        while offset + Self::MIN_RECORD_LENGTH <= end {
            let span = &buffer[offset..end];
            let record_length = u32_at(span, Self::RECORD_LENGTH) as usize;
            if record_length < Self::MIN_RECORD_LENGTH || offset + record_length > end {
                break;
            }
            let major = u16_at(span, Self::MAJOR_VERSION);
            if major == 2 {
                let frn = u64_at(span, Self::FILE_REFERENCE_NUMBER);
                let parent_frn = u64_at(span, Self::PARENT_FILE_REFERENCE_NUMBER);
                let usn = i64_at(span, Self::USN);
                let stamp = file_time_or_unknown(i64_at(span, Self::TIME_STAMP));
                let reason = u32_at(span, Self::REASON);
                let attributes = u32_at(span, Self::FILE_ATTRIBUTES);
                let name_length = u16_at(span, Self::FILE_NAME_LENGTH) as usize;
                let name_offset = u16_at(span, Self::FILE_NAME_OFFSET) as usize;
                if name_offset + name_length <= record_length && name_length > 0 {
                    records.push(UsnRecord {
                        file_reference_number: frn,
                        parent_file_reference_number: parent_frn,
                        reason,
                        attributes,
                        name: utf16_at(span, name_offset, name_length),
                        usn,
                        timestamp_utc: stamp,
                    });
                }
            }
            offset += record_length;
        }
        records
    }
}
