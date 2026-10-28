//! On-disk NTFS structure offsets and the data-run (runlist) decoder: everything needed to walk the
//! raw `$MFT`. Field offsets are the NTFS on-disk format; `MftReader` is where they are used. Spec:
//! Core/Services/Mft/NtfsLayout.cs.

/// One extent of a non-resident attribute: `length_clusters` clusters starting at logical cluster
/// `start_lcn`. A sparse run (a hole) has `start_lcn == -1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataRun {
    pub start_lcn: i64,
    pub length_clusters: i64,
}

pub struct NtfsLayout;

impl NtfsLayout {
    // --- Boot sector (volume boot record) ---
    pub const BOOT_BYTES_PER_SECTOR: usize = 0x0B; // u16
    pub const BOOT_SECTORS_PER_CLUSTER: usize = 0x0D; // u8
    pub const BOOT_MFT_START_LCN: usize = 0x30; // i64
    pub const BOOT_CLUSTERS_PER_FILE_RECORD: usize = 0x40; // i8 (signed)

    // --- FILE record header ---
    pub const FILE_SIGNATURE: u32 = 0x454C_4946; // "FILE" little-endian
    pub const REC_UPDATE_SEQ_OFFSET: usize = 0x04; // u16
    pub const REC_UPDATE_SEQ_SIZE: usize = 0x06; // u16 (count of u16 words)
    pub const REC_BASE_RECORD: usize = 0x20; // u64: non-zero for an extension record
    pub const REC_FIRST_ATTRIBUTE_OFFSET: usize = 0x14; // u16
    pub const REC_FLAGS: usize = 0x16; // u16
    pub const REC_FLAG_IN_USE: u16 = 0x0001;
    pub const REC_FLAG_DIRECTORY: u16 = 0x0002;

    // --- Attribute header (common) ---
    pub const ATTR_TYPE_END: u32 = 0xFFFF_FFFF;
    pub const ATTR_TYPE: usize = 0x00; // u32
    pub const ATTR_LENGTH: usize = 0x04; // u32 (advance by this)
    pub const ATTR_NON_RESIDENT_FLAG: usize = 0x08; // u8
    pub const ATTR_NAME_LENGTH: usize = 0x09; // u8
    // Resident attribute:
    pub const ATTR_RES_VALUE_LENGTH: usize = 0x10; // u32
    pub const ATTR_RES_VALUE_OFFSET: usize = 0x14; // u16
    // Non-resident attribute:
    pub const ATTR_NON_RES_DATA_RUNS_OFFSET: usize = 0x20; // u16
    pub const ATTR_NON_RES_REAL_SIZE: usize = 0x30; // u64

    // --- Attribute type codes ---
    pub const ATTR_STANDARD_INFORMATION: u32 = 0x10;
    pub const ATTR_FILE_NAME: u32 = 0x30;
    pub const ATTR_DATA: u32 = 0x80;

    // --- $STANDARD_INFORMATION content ---
    // The four FILETIMEs run created / modified / MFT-changed / accessed at 0x00, 0x08, 0x10,
    // 0x18. Accessed is deliberately not read: Windows 10+ ships with last-access updates off,
    // so the value on disk is stale for most files.
    pub const STD_INFO_CREATED: usize = 0x00; // u64 FILETIME
    pub const STD_INFO_MODIFIED: usize = 0x08; // u64 FILETIME
    pub const STD_INFO_FILE_ATTRIBUTES: usize = 0x20; // u32

    // --- $FILE_NAME content ---
    pub const FILE_NAME_PARENT_REF: usize = 0x00; // u64 (low 48 bits = record number)
    pub const FILE_NAME_REAL_SIZE: usize = 0x30; // u64
    pub const FILE_NAME_FLAGS: usize = 0x38; // u32
    pub const FILE_NAME_LENGTH: usize = 0x40; // u8 (characters)
    pub const FILE_NAME_NAMESPACE: usize = 0x41; // u8
    pub const FILE_NAME_NAME: usize = 0x42; // WCHAR[]

    /// The 8.3 alias; any other namespace (Win32, POSIX, both) is preferred over it.
    pub const NAMESPACE_DOS: u8 = 2;

    /// Named FILE_ATTRIBUTE bits for callers wanting one by name; the reader keeps the whole mask.
    pub const FILE_ATTRIBUTE_HIDDEN: u32 = 0x0002;
    pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;

    /// The 48-bit record number of the root directory ("."), always 5.
    pub const ROOT_RECORD_NUMBER: u64 = 5;

    /// The root's full file reference (record 5, sequence 5), as the USN journal carries it.
    pub const ROOT_FILE_REFERENCE_NUMBER: u64 = 0x0005_0000_0000_0005;

    /// Low 48 bits of an NTFS file reference are the MFT record number; the high 16 are a reuse
    /// sequence number path building does not need.
    pub fn record_number(file_reference: u64) -> u64 {
        file_reference & 0x0000_FFFF_FFFF_FFFF
    }

    /// Decodes an NTFS data-run list into absolute cluster runs. Each run is a header byte (low
    /// nibble = length field size, high nibble = LCN-delta field size) followed by an unsigned
    /// length and a signed LCN delta from the previous run's start. A zero header ends the list; a
    /// zero delta-size marks a sparse hole (`start_lcn == -1`). Malformed or truncated input ends
    /// the list rather than failing.
    pub fn decode_data_runs(run_list: &[u8]) -> Vec<DataRun> {
        let mut runs = Vec::new();
        let mut lcn: i64 = 0;
        let mut i = 0usize;
        while i < run_list.len() {
            let header = run_list[i];
            i += 1;
            if header == 0 {
                break;
            }
            let length_size = (header & 0x0F) as usize;
            let offset_size = ((header >> 4) & 0x0F) as usize;
            if length_size == 0 || i + length_size + offset_size > run_list.len() {
                break;
            }
            let length = read_le_unsigned(&run_list[i..i + length_size]);
            i += length_size;
            if offset_size == 0 {
                runs.push(DataRun {
                    start_lcn: -1,
                    length_clusters: length,
                });
                continue;
            }
            lcn = lcn.wrapping_add(read_le_signed(&run_list[i..i + offset_size]));
            i += offset_size;
            runs.push(DataRun {
                start_lcn: lcn,
                length_clusters: length,
            });
        }
        runs
    }
}

fn read_le_unsigned(bytes: &[u8]) -> i64 {
    let mut value: i64 = 0;
    for &b in bytes.iter().rev() {
        value = (value << 8) | i64::from(b);
    }
    value
}

fn read_le_signed(bytes: &[u8]) -> i64 {
    let mut value = read_le_unsigned(bytes);
    let bits = bytes.len() * 8;
    if bits < 64 && (value & (1i64 << (bits - 1))) != 0 {
        value |= -1i64 << bits; // sign-extend
    }
    value
}

/// Little-endian field readers over a record slice, shared by the readers. Each returns 0 for a
/// read that would run off the end, so a truncated record parses as empty rather than panicking.
pub(crate) fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    bytes
        .get(offset..offset + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .unwrap_or(0)
}

pub(crate) fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    bytes
        .get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap_or(0)
}

pub(crate) fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    bytes
        .get(offset..offset + 8)
        .map(|b| u64::from_le_bytes(b.try_into().expect("eight bytes")))
        .unwrap_or(0)
}

pub(crate) fn i64_at(bytes: &[u8], offset: usize) -> i64 {
    u64_at(bytes, offset) as i64
}

/// UTF-16LE text of `chars` characters at `offset`, lossily decoded and clipped to the slice.
pub(crate) fn utf16_at(bytes: &[u8], offset: usize, byte_len: usize) -> String {
    let end = (offset + byte_len).min(bytes.len());
    let slice = bytes.get(offset..end).unwrap_or(&[]);
    let units: Vec<u16> = slice
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16_lossy(&units)
}
