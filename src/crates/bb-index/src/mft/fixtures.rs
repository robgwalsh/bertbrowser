//! Bytes laid out by hand from the NTFS and USN specs: boot sectors, FILE records, USN_RECORD_V2s,
//! and an in-memory volume built from them, so the reader, the roll-up and the whole volume indexer
//! run without a raw handle or a token. The record builders are the ones `NtfsParsingTests` and
//! `UsnRecordParserTests` lay out; the volume is what `bb-indexer --selftest` drives. Never written
//! by the code under test.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::Mutex;

use super::ntfs_layout::NtfsLayout;
use super::reader::VolumeRead;
use super::volume_indexer::{JournalData, VolumeAccess, VolumeHandle};

/// A `$FILE_NAME` to write: the name, its namespace (1 = Win32, 2 = DOS) and the parent reference.
#[derive(Clone, Debug)]
pub struct FileNameSpec {
    pub name: String,
    pub namespace: u8,
    pub parent: u64,
}

impl FileNameSpec {
    pub fn win32(name: &str, parent: u64) -> FileNameSpec {
        FileNameSpec {
            name: name.to_string(),
            namespace: 1,
            parent,
        }
    }
}

/// One FILE record to lay out: header + `$STANDARD_INFORMATION` + one or more `$FILE_NAME` + an
/// optional non-resident `$DATA` + end marker. USA words are zero (tails already zero) so the fixup
/// is a harmless no-op over the attributes.
#[derive(Clone, Debug)]
pub struct FileRecordSpec {
    pub is_dir: bool,
    pub hidden: bool,
    /// Further FILE_ATTRIBUTE bits beyond Hidden.
    pub extra_attrs: u32,
    pub size: i64,
    pub file_names: Vec<FileNameSpec>,
    /// False for a heavily fragmented file whose `$DATA` lives in extension records.
    pub with_data: bool,
    pub modified_file_time: i64,
    pub created_file_time: i64,
}

impl FileRecordSpec {
    pub fn file(name: &str, parent: u64, size: i64) -> FileRecordSpec {
        FileRecordSpec {
            is_dir: false,
            hidden: false,
            extra_attrs: 0x20,
            size,
            file_names: vec![FileNameSpec::win32(name, parent)],
            with_data: true,
            modified_file_time: 0,
            created_file_time: 0,
        }
    }

    pub fn dir(name: &str, parent: u64) -> FileRecordSpec {
        FileRecordSpec {
            is_dir: true,
            hidden: false,
            extra_attrs: 0x10,
            size: 0,
            file_names: vec![FileNameSpec::win32(name, parent)],
            with_data: false,
            modified_file_time: 0,
            created_file_time: 0,
        }
    }
}

pub const RECORD_SIZE: usize = 1024;

fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u64(buf: &mut [u8], off: usize, v: u64) {
    buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

fn align8(value: usize) -> usize {
    (value + 7) & !7
}

/// Assembles a minimal-but-valid FILE record of `RECORD_SIZE` bytes.
pub fn build_file_record(spec: &FileRecordSpec) -> Vec<u8> {
    let mut rec = vec![0u8; RECORD_SIZE];
    rec[..4].copy_from_slice(b"FILE");
    put_u16(&mut rec, NtfsLayout::REC_UPDATE_SEQ_OFFSET, 0x30);
    put_u16(&mut rec, NtfsLayout::REC_UPDATE_SEQ_SIZE, 3);
    put_u16(&mut rec, NtfsLayout::REC_FIRST_ATTRIBUTE_OFFSET, 0x38);
    let flags = NtfsLayout::REC_FLAG_IN_USE
        | if spec.is_dir {
            NtfsLayout::REC_FLAG_DIRECTORY
        } else {
            0
        };
    put_u16(&mut rec, NtfsLayout::REC_FLAGS, flags);

    let mut off = 0x38;
    let attrs = if spec.hidden {
        NtfsLayout::FILE_ATTRIBUTE_HIDDEN
    } else {
        0
    } | spec.extra_attrs;
    off += write_std_info(
        &mut rec,
        off,
        spec.modified_file_time,
        attrs,
        spec.created_file_time,
    );
    for name in &spec.file_names {
        off += write_file_name(&mut rec, off, name.parent, &name.name, name.namespace);
    }
    if !spec.is_dir && spec.with_data {
        off += write_data_non_resident(&mut rec, off, spec.size);
    }
    put_u32(&mut rec, off, NtfsLayout::ATTR_TYPE_END);
    rec
}

fn write_resident_header(rec: &mut [u8], off: usize, attr_type: u32, content_len: usize) -> usize {
    put_u32(rec, off, attr_type);
    let len = align8(0x18 + content_len);
    put_u32(rec, off + 0x04, len as u32);
    rec[off + 0x08] = 0; // resident
    rec[off + 0x09] = 0; // unnamed
    put_u32(rec, off + 0x10, content_len as u32);
    put_u16(rec, off + 0x14, 0x18); // value offset
    len
}

fn write_std_info(rec: &mut [u8], off: usize, modified: i64, attrs: u32, created: i64) -> usize {
    let content_len = 0x30;
    let len = write_resident_header(rec, off, NtfsLayout::ATTR_STANDARD_INFORMATION, content_len);
    let c = off + 0x18;
    put_u64(rec, c + NtfsLayout::STD_INFO_CREATED, created as u64);
    put_u64(rec, c + NtfsLayout::STD_INFO_MODIFIED, modified as u64);
    put_u32(rec, c + NtfsLayout::STD_INFO_FILE_ATTRIBUTES, attrs);
    len
}

fn write_file_name(rec: &mut [u8], off: usize, parent_ref: u64, name: &str, ns: u8) -> usize {
    let name_units: Vec<u16> = name.encode_utf16().collect();
    let content_len = NtfsLayout::FILE_NAME_NAME + name_units.len() * 2;
    let len = write_resident_header(rec, off, NtfsLayout::ATTR_FILE_NAME, content_len);
    let c = off + 0x18;
    put_u64(rec, c + NtfsLayout::FILE_NAME_PARENT_REF, parent_ref);
    rec[c + NtfsLayout::FILE_NAME_LENGTH] = name_units.len() as u8;
    rec[c + NtfsLayout::FILE_NAME_NAMESPACE] = ns;
    for (i, unit) in name_units.iter().enumerate() {
        put_u16(rec, c + NtfsLayout::FILE_NAME_NAME + i * 2, *unit);
    }
    len
}

fn write_data_non_resident(rec: &mut [u8], off: usize, real_size: i64) -> usize {
    let len = 0x48;
    put_u32(rec, off, NtfsLayout::ATTR_DATA);
    put_u32(rec, off + 0x04, len as u32);
    rec[off + 0x08] = 1; // non-resident
    rec[off + 0x09] = 0; // unnamed
    put_u16(rec, off + NtfsLayout::ATTR_NON_RES_DATA_RUNS_OFFSET, 0x40);
    put_u64(
        rec,
        off + NtfsLayout::ATTR_NON_RES_REAL_SIZE,
        real_size as u64,
    );
    len
}

/// Record 0, the `$MFT` itself: an unnamed non-resident `$DATA` whose runlist is one run of
/// `length_clusters` at `start_lcn`.
pub fn build_mft_record(start_lcn: i64, length_clusters: i64) -> Vec<u8> {
    let mut rec = vec![0u8; RECORD_SIZE];
    rec[..4].copy_from_slice(b"FILE");
    put_u16(&mut rec, NtfsLayout::REC_UPDATE_SEQ_OFFSET, 0x30);
    put_u16(&mut rec, NtfsLayout::REC_UPDATE_SEQ_SIZE, 3);
    put_u16(&mut rec, NtfsLayout::REC_FIRST_ATTRIBUTE_OFFSET, 0x38);
    put_u16(&mut rec, NtfsLayout::REC_FLAGS, NtfsLayout::REC_FLAG_IN_USE);
    let off = 0x38;
    // Four-byte fields for both: header 0x44, length, then the LCN delta from zero.
    let runs_offset = 0x40;
    let len = align8(runs_offset + 1 + 4 + 4 + 1);
    put_u32(&mut rec, off, NtfsLayout::ATTR_DATA);
    put_u32(&mut rec, off + 0x04, len as u32);
    rec[off + 0x08] = 1;
    rec[off + 0x09] = 0;
    put_u16(
        &mut rec,
        off + NtfsLayout::ATTR_NON_RES_DATA_RUNS_OFFSET,
        runs_offset as u16,
    );
    put_u64(
        &mut rec,
        off + NtfsLayout::ATTR_NON_RES_REAL_SIZE,
        (length_clusters * 4096) as u64,
    );
    let r = off + runs_offset;
    rec[r] = 0x44;
    put_u32(&mut rec, r + 1, length_clusters as u32);
    put_u32(&mut rec, r + 5, start_lcn as u32);
    rec[r + 9] = 0;
    put_u32(&mut rec, off + len, NtfsLayout::ATTR_TYPE_END);
    rec
}

/// A boot sector: `bytes_per_sector`, `sectors_per_cluster`, the `$MFT` start and a record size of
/// `2^record_size_log2` bytes.
pub fn build_boot_sector(
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    mft_start_lcn: i64,
    record_size_log2: u8,
) -> Vec<u8> {
    let mut boot = vec![0u8; 512];
    boot[3..11].copy_from_slice(b"NTFS    ");
    put_u16(
        &mut boot,
        NtfsLayout::BOOT_BYTES_PER_SECTOR,
        bytes_per_sector,
    );
    boot[NtfsLayout::BOOT_SECTORS_PER_CLUSTER] = sectors_per_cluster;
    put_u64(
        &mut boot,
        NtfsLayout::BOOT_MFT_START_LCN,
        mft_start_lcn as u64,
    );
    boot[NtfsLayout::BOOT_CLUSTERS_PER_FILE_RECORD] = (-(record_size_log2 as i8)) as u8;
    boot
}

/// One USN_RECORD_V2, 8-byte aligned like the OS.
#[allow(clippy::too_many_arguments)]
pub fn build_usn_record(
    frn: u64,
    parent_frn: u64,
    reason: u32,
    attributes: u32,
    name: &str,
    major: u16,
    usn: i64,
    file_time: i64,
) -> Vec<u8> {
    const NAME_OFFSET: usize = 60;
    let name_units: Vec<u16> = name.encode_utf16().collect();
    let name_bytes = name_units.len() * 2;
    let length = align8(NAME_OFFSET + name_bytes);
    let mut buf = vec![0u8; length];
    put_u32(&mut buf, 0, length as u32);
    put_u16(&mut buf, 4, major);
    put_u64(&mut buf, 8, frn);
    put_u64(&mut buf, 16, parent_frn);
    put_u64(&mut buf, 24, usn as u64);
    put_u64(&mut buf, 32, file_time as u64);
    put_u32(&mut buf, 40, reason);
    put_u32(&mut buf, 52, attributes);
    put_u16(&mut buf, 56, name_bytes as u16);
    put_u16(&mut buf, 58, NAME_OFFSET as u16);
    for (i, unit) in name_units.iter().enumerate() {
        put_u16(&mut buf, NAME_OFFSET + i * 2, *unit);
    }
    buf
}

/// Prepends the 8-byte cursor an FSCTL output carries before its records.
pub fn with_cursor(cursor: u64, records: &[Vec<u8>]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8 + records.iter().map(Vec::len).sum::<usize>());
    buf.extend_from_slice(&cursor.to_le_bytes());
    for r in records {
        buf.extend_from_slice(r);
    }
    buf
}

/// Builds the byte image of a small NTFS volume: 512-byte sectors, 4 KB clusters, 1 KB records,
/// the `$MFT` at cluster 4. Records 1..16 are reserved and left blank; callers add from 16 up.
pub struct VolumeImageBuilder {
    records: HashMap<u64, FileRecordSpec>,
}

impl VolumeImageBuilder {
    pub const BYTES_PER_CLUSTER: usize = 4096;
    pub const MFT_START_LCN: i64 = 4;

    pub fn new() -> VolumeImageBuilder {
        VolumeImageBuilder {
            records: HashMap::new(),
        }
    }

    pub fn add(&mut self, record_number: u64, spec: FileRecordSpec) -> &mut Self {
        self.records.insert(record_number, spec);
        self
    }

    pub fn build(&self) -> Vec<u8> {
        let highest = self.records.keys().copied().max().unwrap_or(16);
        let record_count = (highest + 1) as usize;
        let mft_bytes = record_count * RECORD_SIZE;
        let mft_clusters = mft_bytes.div_ceil(Self::BYTES_PER_CLUSTER).max(1);
        let mft_offset = Self::MFT_START_LCN as usize * Self::BYTES_PER_CLUSTER;
        let mut image = vec![0u8; mft_offset + mft_clusters * Self::BYTES_PER_CLUSTER];
        let boot = build_boot_sector(512, 8, Self::MFT_START_LCN, 10);
        image[..boot.len()].copy_from_slice(&boot);
        let mft = build_mft_record(Self::MFT_START_LCN, mft_clusters as i64);
        image[mft_offset..mft_offset + RECORD_SIZE].copy_from_slice(&mft);
        for (number, spec) in &self.records {
            let at = mft_offset + *number as usize * RECORD_SIZE;
            image[at..at + RECORD_SIZE].copy_from_slice(&build_file_record(spec));
        }
        image
    }
}

impl Default for VolumeImageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// An in-memory volume: a byte image for the `$MFT` reader, a canned journal, and a queue of USN
/// batches the tail drains in order. Once the batches are gone every read is idle (the same next
/// USN, no records) until `lose_journal` makes the next one fail.
pub struct FakeVolume {
    image: Vec<u8>,
    journal: Option<JournalData>,
    batches: VecDeque<Vec<u8>>,
    next_usn: i64,
    lost: bool,
    enum_records: Vec<Vec<u8>>,
}

impl FakeVolume {
    pub fn new(image: Vec<u8>) -> FakeVolume {
        FakeVolume {
            image,
            journal: Some(JournalData {
                journal_id: 0x1234,
                next_usn: 100,
                max_usn: 1 << 40,
            }),
            batches: VecDeque::new(),
            next_usn: 100,
            lost: false,
            enum_records: Vec::new(),
        }
    }

    /// A volume whose journal cannot be had, so `open` fails.
    pub fn without_journal(mut self) -> FakeVolume {
        self.journal = None;
        self
    }

    /// What FSCTL_ENUM_USN_DATA hands back, for the names-only fallback; one call's worth.
    pub fn with_enum_records(mut self, records: Vec<Vec<u8>>) -> FakeVolume {
        self.enum_records = records;
        self
    }

    /// Queues one drained batch: the records, and the USN the journal reports after them.
    pub fn push_batch(&mut self, next_usn: i64, records: &[Vec<u8>]) {
        self.batches
            .push_back(with_cursor(next_usn as u64, records));
    }

    /// The next read fails, as an overflowed or deleted journal does.
    pub fn lose_journal(&mut self) {
        self.lost = true;
    }
}

impl VolumeRead for FakeVolume {
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        let start = offset as usize;
        let end = start + buffer.len();
        if end > self.image.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "past the image",
            ));
        }
        buffer.copy_from_slice(&self.image[start..end]);
        Ok(())
    }
}

impl VolumeHandle for FakeVolume {
    fn query_journal(&mut self) -> Option<JournalData> {
        self.journal
    }

    fn enum_usn_data(
        &mut self,
        start_frn: u64,
        _high_usn: i64,
        buffer: &mut [u8],
    ) -> Option<usize> {
        if start_frn != 0 || self.enum_records.is_empty() {
            return None;
        }
        let out = with_cursor(u64::MAX, &self.enum_records);
        if out.len() > buffer.len() {
            return None;
        }
        buffer[..out.len()].copy_from_slice(&out);
        Some(out.len())
    }

    fn read_usn_journal(
        &mut self,
        start_usn: i64,
        _journal_id: u64,
        buffer: &mut [u8],
    ) -> Option<usize> {
        if self.lost {
            return None;
        }
        let _ = start_usn;
        let out = match self.batches.pop_front() {
            Some(batch) => {
                self.next_usn = i64::from_le_bytes(batch[..8].try_into().expect("cursor"));
                batch
            }
            None => (self.next_usn as u64).to_le_bytes().to_vec(),
        };
        if out.len() > buffer.len() {
            return None;
        }
        buffer[..out.len()].copy_from_slice(&out);
        Some(out.len())
    }
}

/// Hands out fake volumes by drive letter, once each.
pub struct FakeVolumeAccess {
    volumes: Mutex<HashMap<String, Option<FakeVolume>>>,
}

impl FakeVolumeAccess {
    pub fn new() -> FakeVolumeAccess {
        FakeVolumeAccess {
            volumes: Mutex::new(HashMap::new()),
        }
    }

    pub fn add(&self, drive_letter: &str, volume: FakeVolume) {
        self.volumes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(drive_letter.to_string(), Some(volume));
    }
}

impl Default for FakeVolumeAccess {
    fn default() -> Self {
        Self::new()
    }
}

impl VolumeAccess for FakeVolumeAccess {
    fn ntfs_volumes(&self) -> Vec<String> {
        let mut letters: Vec<String> = self
            .volumes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .cloned()
            .collect();
        letters.sort();
        letters
    }

    fn open(&self, drive_letter: &str) -> Option<Box<dyn VolumeHandle>> {
        self.volumes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(drive_letter)
            .and_then(Option::take)
            .map(|v| Box::new(v) as Box<dyn VolumeHandle>)
    }
}
