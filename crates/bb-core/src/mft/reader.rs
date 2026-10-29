//! Reads the raw NTFS `$MFT` from a volume: parses the boot sector for geometry, resolves the
//! `$MFT`'s own (fragmented) extent map from record 0, then streams every in-use FILE record,
//! applying the update-sequence fixup and pulling name, parent, size, timestamps and attributes.
//! Volume reads are cluster-aligned, a hard requirement for raw `\\.\C:` access. The parsers are
//! pure so they are tested against canned bytes; the volume is a `VolumeRead`, so the whole reader
//! runs over an in-memory image. Spec: Core/Services/Mft/MftReader.cs.

use std::io;

use super::ntfs_layout::{NtfsLayout, i64_at, u16_at, u32_at, u64_at, utf16_at};
use crate::FileTime;
use crate::attributes::HIDDEN;
use crate::cancel::{CancelToken, Cancelled};

/// One file/directory read straight from an NTFS FILE record. Unlike the USN enumeration, this
/// carries the real `size` (unnamed `$DATA` real size) and `modified_utc`, which is what makes
/// instant directory sizing possible. `size` is -1 for a file whose `$DATA` lives in extension
/// records (the base record cannot report it); the caller stats those from disk. Directories are 0.
/// `attributes` is the whole `$STANDARD_INFORMATION` mask, the entry's *own*: the effective,
/// inherited hidden state is the caller's, from the parent chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MftFileRecord {
    pub record_number: u64,
    pub parent_record_number: u64,
    pub name: String,
    pub is_directory: bool,
    pub attributes: u32,
    pub size: i64,
    pub modified_utc: FileTime,
    /// Out of the same attribute as `modified_utc`, four fields along, so it costs no extra read.
    pub created_utc: FileTime,
}

impl MftFileRecord {
    pub fn hidden(&self) -> bool {
        self.attributes & HIDDEN != 0
    }
}

/// What the boot sector says about the volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootGeometry {
    pub bytes_per_sector: usize,
    pub bytes_per_cluster: usize,
    pub mft_start_lcn: i64,
    pub bytes_per_file_record: usize,
}

/// Positioned reads from a volume. The real one is a raw device handle; the fake is a byte image.
pub trait VolumeRead {
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()>;
}

/// Why a read of the whole table stopped short of the end.
#[derive(Debug)]
pub enum ReadError {
    Cancelled,
    Io(io::Error),
}

impl From<Cancelled> for ReadError {
    fn from(_: Cancelled) -> ReadError {
        ReadError::Cancelled
    }
}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> ReadError {
        ReadError::Io(e)
    }
}

/// `DateTime.MaxValue` as a FILETIME: anything past it is not a date.
const MAX_FILE_TIME: i64 = 2_650_467_743_999_999_999;

/// The guard both readers apply: an unset or impossible FILETIME is unknown, not 1601.
pub(crate) fn file_time_or_unknown(raw: i64) -> FileTime {
    if raw <= 0 || raw > MAX_FILE_TIME {
        FileTime::UNKNOWN
    } else {
        FileTime(raw)
    }
}

pub struct MftReader<'a> {
    volume: &'a dyn VolumeRead,
}

impl<'a> MftReader<'a> {
    const CHUNK_TARGET_BYTES: usize = 4 * 1024 * 1024;

    pub fn new(volume: &'a dyn VolumeRead) -> MftReader<'a> {
        MftReader { volume }
    }

    /// Streams every in-use file/directory record to `on_record`. `Ok(false)` (rather than an
    /// error) when the volume geometry cannot be parsed, so the caller can fall back to the USN
    /// enumeration. Reserved system metafiles (record < 16: `$MFT`, `$LogFile`, the root's ".")
    /// are skipped so sizes match the user-visible tree.
    pub fn try_read_all(
        &self,
        cancel: &CancelToken,
        mut on_record: impl FnMut(MftFileRecord),
    ) -> Result<bool, ReadError> {
        let mut boot = vec![0u8; 8192];
        self.volume.read_exact_at(0, &mut boot)?;
        let Some(geometry) = Self::try_parse_boot_sector(&boot) else {
            return Ok(false);
        };
        let BootGeometry {
            bytes_per_sector,
            bytes_per_cluster,
            mft_start_lcn,
            bytes_per_file_record,
        } = geometry;

        // Record 0 is the $MFT itself; its unnamed $DATA runlist is the full extent map.
        let mut first = vec![0u8; bytes_per_cluster.max(bytes_per_file_record)];
        self.volume
            .read_exact_at(mft_start_lcn as u64 * bytes_per_cluster as u64, &mut first)?;
        Self::apply_fixup(&mut first, 0, bytes_per_file_record, bytes_per_sector);
        let mft_runs = Self::read_mft_data_runs(&first);
        if mft_runs.is_empty() {
            return Ok(false);
        }

        let chunk_clusters = (Self::CHUNK_TARGET_BYTES / bytes_per_cluster).max(1) as i64;
        let mut chunk = vec![0u8; chunk_clusters as usize * bytes_per_cluster];
        let mut pending = vec![0u8; bytes_per_file_record];
        let mut pending_len = 0usize;
        let mut record_index: u64 = 0;

        for run in mft_runs {
            if run.start_lcn < 0 {
                // Sparse hole in the $MFT (rare): advance the record counter past it.
                record_index += (run.length_clusters as u64 * bytes_per_cluster as u64)
                    / bytes_per_file_record as u64;
                continue;
            }
            let mut clusters_left = run.length_clusters;
            let mut lcn = run.start_lcn;
            while clusters_left > 0 {
                cancel.check()?;
                let take = clusters_left.min(chunk_clusters);
                let bytes = take as usize * bytes_per_cluster;
                self.volume
                    .read_exact_at(lcn as u64 * bytes_per_cluster as u64, &mut chunk[..bytes])?;
                Self::feed_records(
                    &chunk[..bytes],
                    &mut pending,
                    &mut pending_len,
                    &mut record_index,
                    bytes_per_file_record,
                    bytes_per_sector,
                    &mut on_record,
                );
                lcn += take;
                clusters_left -= take;
            }
        }
        Ok(true)
    }

    /// Slices whole FILE records out of a freshly read chunk, carrying a partial record across
    /// chunk/run boundaries (needed only when a record spans clusters: small clusters).
    #[allow(clippy::too_many_arguments)]
    fn feed_records(
        data: &[u8],
        pending: &mut [u8],
        pending_len: &mut usize,
        record_index: &mut u64,
        record_length: usize,
        bytes_per_sector: usize,
        on_record: &mut impl FnMut(MftFileRecord),
    ) {
        let mut pos = 0usize;
        if *pending_len > 0 {
            let need = record_length - *pending_len;
            pending[*pending_len..record_length].copy_from_slice(&data[..need]);
            let mut owned = pending[..record_length].to_vec();
            if let Some(parsed) = Self::try_parse_file_record(
                &mut owned,
                *record_index,
                record_length,
                bytes_per_sector,
            ) {
                on_record(parsed);
            }
            *record_index += 1;
            *pending_len = 0;
            pos = need;
        }

        while pos + record_length <= data.len() {
            // The fixup mutates in place; a copy keeps the shared chunk buffer untouched.
            let mut owned = data[pos..pos + record_length].to_vec();
            if let Some(parsed) = Self::try_parse_file_record(
                &mut owned,
                *record_index,
                record_length,
                bytes_per_sector,
            ) {
                on_record(parsed);
            }
            *record_index += 1;
            pos += record_length;
        }

        let leftover = data.len() - pos;
        if leftover > 0 {
            pending[..leftover].copy_from_slice(&data[pos..]);
            *pending_len = leftover;
        }
    }

    /// Walks record 0's attributes for the unnamed non-resident `$DATA` and decodes its runlist.
    fn read_mft_data_runs(record: &[u8]) -> Vec<super::DataRun> {
        let mut offset = u16_at(record, NtfsLayout::REC_FIRST_ATTRIBUTE_OFFSET) as usize;
        while offset + 8 <= record.len() {
            let attr_type = u32_at(record, offset + NtfsLayout::ATTR_TYPE);
            if attr_type == NtfsLayout::ATTR_TYPE_END {
                break;
            }
            let length = u32_at(record, offset + NtfsLayout::ATTR_LENGTH) as usize;
            if length == 0 || offset + length > record.len() {
                break;
            }
            let name_len = record[offset + NtfsLayout::ATTR_NAME_LENGTH];
            let non_resident = record[offset + NtfsLayout::ATTR_NON_RESIDENT_FLAG] != 0;
            if attr_type == NtfsLayout::ATTR_DATA && name_len == 0 && non_resident {
                let runs_offset =
                    u16_at(record, offset + NtfsLayout::ATTR_NON_RES_DATA_RUNS_OFFSET) as usize;
                if runs_offset > length {
                    break;
                }
                return NtfsLayout::decode_data_runs(
                    &record[offset + runs_offset..offset + length],
                );
            }
            offset += length;
        }
        Vec::new()
    }

    // --- Pure parsers (unit-tested) ---

    /// The volume geometry, or None for anything that is not an NTFS boot sector.
    pub fn try_parse_boot_sector(boot: &[u8]) -> Option<BootGeometry> {
        if boot.len() < 512 || &boot[3..11] != b"NTFS    " {
            return None;
        }
        let bytes_per_sector = u16_at(boot, NtfsLayout::BOOT_BYTES_PER_SECTOR) as usize;
        let sectors_per_cluster = boot[NtfsLayout::BOOT_SECTORS_PER_CLUSTER] as usize;
        if bytes_per_sector < 256 || !bytes_per_sector.is_power_of_two() || sectors_per_cluster == 0
        {
            return None;
        }
        let bytes_per_cluster = bytes_per_sector * sectors_per_cluster;
        let mft_start_lcn = i64_at(boot, NtfsLayout::BOOT_MFT_START_LCN);
        let raw = boot[NtfsLayout::BOOT_CLUSTERS_PER_FILE_RECORD] as i8;
        let bytes_per_file_record = if raw >= 0 {
            raw as usize * bytes_per_cluster
        } else {
            1usize << (-(raw as i32)) as usize
        };
        if bytes_per_file_record < 512 || mft_start_lcn <= 0 {
            return None;
        }
        Some(BootGeometry {
            bytes_per_sector,
            bytes_per_cluster,
            mft_start_lcn,
            bytes_per_file_record,
        })
    }

    /// Restores the last two bytes of each sector from the update-sequence array. NTFS stamps a
    /// signature word into those positions on disk to detect torn writes; without undoing it, any
    /// field straddling a sector boundary is corrupt.
    pub fn apply_fixup(
        record: &mut [u8],
        record_offset: usize,
        record_length: usize,
        bytes_per_sector: usize,
    ) {
        let usa_offset = record_offset
            + u16_at(record, record_offset + NtfsLayout::REC_UPDATE_SEQ_OFFSET) as usize;
        let usa_count = u16_at(record, record_offset + NtfsLayout::REC_UPDATE_SEQ_SIZE) as usize;
        if usa_count == 0 {
            return;
        }
        for sector in 0..usa_count - 1 {
            let tail = record_offset + (sector + 1) * bytes_per_sector - 2;
            let usa_entry = usa_offset + 2 + sector * 2;
            if tail + 2 > record_offset + record_length || usa_entry + 2 > record.len() {
                break;
            }
            record[tail] = record[usa_entry];
            record[tail + 1] = record[usa_entry + 1];
        }
    }

    /// Parses one FILE record (applying its fixup in place). None for a reserved metafile, a
    /// record that is not in use, an extension record, or one with no name.
    pub fn try_parse_file_record(
        record: &mut [u8],
        record_number: u64,
        record_length: usize,
        bytes_per_sector: usize,
    ) -> Option<MftFileRecord> {
        if record_number < 16 {
            return None; // reserved NTFS metafiles ($MFT, $LogFile, root ".", $Extend, ...)
        }
        if record.len() < record_length || u32_at(record, 0) != NtfsLayout::FILE_SIGNATURE {
            return None;
        }

        Self::apply_fixup(record, 0, record_length, bytes_per_sector);

        let flags = u16_at(record, NtfsLayout::REC_FLAGS);
        if flags & NtfsLayout::REC_FLAG_IN_USE == 0 {
            return None; // deleted
        }
        // An extension record (non-zero base file reference): its attributes belong to a base
        // record parsed separately.
        if u64_at(record, NtfsLayout::REC_BASE_RECORD) != 0 {
            return None;
        }

        let is_directory = flags & NtfsLayout::REC_FLAG_DIRECTORY != 0;
        let mut name: Option<String> = None;
        let mut chosen_namespace: u8 = 255;
        let mut parent_record: u64 = 0;
        let mut modified = FileTime::UNKNOWN;
        let mut created = FileTime::UNKNOWN;
        let mut attributes: u32 = 0;
        let mut size: i64 = 0;
        let mut have_data = false;

        let mut offset = u16_at(record, NtfsLayout::REC_FIRST_ATTRIBUTE_OFFSET) as usize;
        while offset + 8 <= record_length {
            let attr_type = u32_at(record, offset + NtfsLayout::ATTR_TYPE);
            if attr_type == NtfsLayout::ATTR_TYPE_END {
                break;
            }
            let length = u32_at(record, offset + NtfsLayout::ATTR_LENGTH) as usize;
            if length < 8 || offset + length > record_length {
                break;
            }
            let non_resident = record[offset + NtfsLayout::ATTR_NON_RESIDENT_FLAG] != 0;
            let attr_name_len = record[offset + NtfsLayout::ATTR_NAME_LENGTH];

            if attr_type == NtfsLayout::ATTR_STANDARD_INFORMATION && !non_resident {
                let content =
                    offset + u16_at(record, offset + NtfsLayout::ATTR_RES_VALUE_OFFSET) as usize;
                modified =
                    file_time_or_unknown(i64_at(record, content + NtfsLayout::STD_INFO_MODIFIED));
                created =
                    file_time_or_unknown(i64_at(record, content + NtfsLayout::STD_INFO_CREATED));
                attributes = u32_at(record, content + NtfsLayout::STD_INFO_FILE_ATTRIBUTES);
            } else if attr_type == NtfsLayout::ATTR_FILE_NAME && !non_resident {
                let content =
                    offset + u16_at(record, offset + NtfsLayout::ATTR_RES_VALUE_OFFSET) as usize;
                let ns = record
                    .get(content + NtfsLayout::FILE_NAME_NAMESPACE)
                    .copied()
                    .unwrap_or(255);
                // Prefer any non-DOS (Win32/POSIX) name over the 8.3 DOS alias.
                if name.is_none()
                    || (chosen_namespace == NtfsLayout::NAMESPACE_DOS
                        && ns != NtfsLayout::NAMESPACE_DOS)
                {
                    let name_chars = record
                        .get(content + NtfsLayout::FILE_NAME_LENGTH)
                        .copied()
                        .unwrap_or(0) as usize;
                    name = Some(utf16_at(
                        record,
                        content + NtfsLayout::FILE_NAME_NAME,
                        name_chars * 2,
                    ));
                    parent_record = NtfsLayout::record_number(u64_at(
                        record,
                        content + NtfsLayout::FILE_NAME_PARENT_REF,
                    ));
                    chosen_namespace = ns;
                }
            } else if attr_type == NtfsLayout::ATTR_DATA && attr_name_len == 0 && !have_data {
                size = if non_resident {
                    i64_at(record, offset + NtfsLayout::ATTR_NON_RES_REAL_SIZE)
                } else {
                    i64::from(u32_at(record, offset + NtfsLayout::ATTR_RES_VALUE_LENGTH))
                };
                have_data = true;
            }

            offset += length;
        }

        let name = name?;

        // A file with no unnamed $DATA in its base record has that attribute in extension records
        // (heavy fragmentation): -1 signals "unknown" so the caller stats it.
        let file_size = if is_directory {
            0
        } else if have_data {
            size.max(0)
        } else {
            -1
        };
        Some(MftFileRecord {
            record_number,
            parent_record_number: parent_record,
            name,
            is_directory,
            attributes,
            size: file_size,
            modified_utc: modified,
            created_utc: created,
        })
    }
}
