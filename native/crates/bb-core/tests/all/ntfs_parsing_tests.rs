// Port of tests/BertBrowser.Core.Tests/NtfsParsingTests.cs. The record bytes come from
// `bb_core::mft::fixtures`, laid out by hand from the on-disk format (the C# built them in this
// file); the boot sector for the geometry test is laid out here, field by field, as it was.
use bb_core::FileTime;
use bb_core::attributes::{ARCHIVE, HIDDEN, READONLY, SYSTEM};
use bb_core::mft::fixtures::{FileNameSpec, FileRecordSpec, build_file_record};
use bb_core::mft::{MftFileRecord, MftReader, NtfsLayout};

type Reader<'a> = MftReader<'a>;

// --- Data-run (runlist) decoding ---

#[test]
fn decode_data_runs_single_run() {
    // header 0x21: 1 length byte, 2 offset bytes. length=0x18=24, lcn=0x5634=22068.
    let runs = NtfsLayout::decode_data_runs(&[0x21, 0x18, 0x34, 0x56, 0x00]);

    assert_eq!(1, runs.len());
    assert_eq!(22068, runs[0].start_lcn);
    assert_eq!(24, runs[0].length_clusters);
}

#[test]
fn decode_data_runs_multi_run_with_negative_delta() {
    // Second run's offset 0xE0 is a signed byte (-32): lcn = 22068 - 32 = 22036.
    let runs = NtfsLayout::decode_data_runs(&[0x21, 0x18, 0x34, 0x56, 0x11, 0x10, 0xE0, 0x00]);

    assert_eq!(2, runs.len());
    assert_eq!(22068, runs[0].start_lcn);
    assert_eq!(24, runs[0].length_clusters);
    assert_eq!(22036, runs[1].start_lcn);
    assert_eq!(16, runs[1].length_clusters);
}

#[test]
fn decode_data_runs_sparse_hole_has_no_lcn() {
    // header 0x01: 1 length byte, 0 offset bytes -> sparse.
    let runs = NtfsLayout::decode_data_runs(&[0x01, 0x05, 0x00]);

    assert_eq!(1, runs.len());
    assert_eq!(-1, runs[0].start_lcn);
    assert_eq!(5, runs[0].length_clusters);
}

// --- Boot sector ---

#[test]
fn parse_boot_sector_reads_geometry() {
    let mut boot = vec![0u8; 512];
    boot[3..11].copy_from_slice(b"NTFS    ");
    boot[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes()); // bytes/sector
    boot[0x0D] = 8; // sectors/cluster -> 4096
    boot[0x30..0x38].copy_from_slice(&786432i64.to_le_bytes()); // $MFT start LCN
    boot[0x40] = (-10i8) as u8; // record size 2^10 = 1024

    let geometry = Reader::try_parse_boot_sector(&boot).expect("NTFS");
    assert_eq!(512, geometry.bytes_per_sector);
    assert_eq!(4096, geometry.bytes_per_cluster);
    assert_eq!(786432, geometry.mft_start_lcn);
    assert_eq!(1024, geometry.bytes_per_file_record);
}

#[test]
fn parse_boot_sector_rejects_non_ntfs() {
    let mut boot = vec![0u8; 512];
    boot[3..11].copy_from_slice(b"FAT32   ");
    assert!(Reader::try_parse_boot_sector(&boot).is_none());
}

// --- Update-sequence-array fixup ---

#[test]
fn apply_fixup_restores_sector_tails() {
    let mut rec = vec![0u8; 1024];
    rec[NtfsLayout::REC_UPDATE_SEQ_OFFSET..][..2].copy_from_slice(&0x30u16.to_le_bytes()); // USA at 0x30
    rec[NtfsLayout::REC_UPDATE_SEQ_SIZE..][..2].copy_from_slice(&3u16.to_le_bytes()); // USN + 2 fixups
    rec[0x30..0x32].copy_from_slice(&0xAAAAu16.to_le_bytes()); // USN signature
    rec[0x32..0x34].copy_from_slice(&0x1234u16.to_le_bytes()); // real bytes for sector 0 tail
    rec[0x34..0x36].copy_from_slice(&0x5678u16.to_le_bytes()); // real bytes for sector 1 tail

    Reader::apply_fixup(&mut rec, 0, 1024, 512);

    assert_eq!(0x34, rec[510]);
    assert_eq!(0x12, rec[511]); // 0x1234 LE
    assert_eq!(0x78, rec[1022]);
    assert_eq!(0x56, rec[1023]); // 0x5678 LE
}

// --- FILE record parsing ---

fn modified() -> FileTime {
    FileTime::from_ymd_hms(2020, 1, 2, 3, 4, 5)
}

/// Deliberately unlike `modified`, and written to a different offset in the same attribute.
/// Reading created from modified's 0x08 instead of 0x00 is a one-character mistake that no other
/// test could see.
fn created() -> FileTime {
    FileTime::from_ymd_hms(2015, 11, 12, 13, 14, 15)
}

fn names(list: &[(&str, u8, u64)]) -> Vec<FileNameSpec> {
    list.iter()
        .map(|(name, ns, parent)| FileNameSpec {
            name: name.to_string(),
            namespace: *ns,
            parent: *parent,
        })
        .collect()
}

/// A minimal-but-valid FILE record, as the C# `BuildRecord` laid it out.
fn build_record(
    is_dir: bool,
    hidden: bool,
    size: i64,
    file_names: Vec<FileNameSpec>,
    with_data: bool,
    extra_attrs: u32,
) -> Vec<u8> {
    build_file_record(&FileRecordSpec {
        is_dir,
        hidden,
        extra_attrs,
        size,
        file_names,
        with_data,
        modified_file_time: modified().0,
        created_file_time: created().0,
    })
}

fn parse(rec: &mut [u8], record_number: u64) -> Option<MftFileRecord> {
    Reader::try_parse_file_record(rec, record_number, 1024, 512)
}

#[test]
fn parse_file_record_reads_name_parent_size_and_date() {
    let mut rec = build_record(
        false,
        false,
        1_000_000,
        names(&[("report.txt", 1, 5)]),
        true,
        0,
    );

    let parsed = parse(&mut rec, 40).expect("parses");
    assert_eq!("report.txt", parsed.name);
    assert_eq!(5, parsed.parent_record_number);
    assert!(!parsed.is_directory);
    assert!(!parsed.hidden());
    assert_eq!(1_000_000, parsed.size);
    assert_eq!(modified(), parsed.modified_utc);
}

#[test]
fn parse_file_record_prefers_win32_name_over_dos() {
    // DOS 8.3 alias listed first; the Win32 name must win.
    let mut rec = build_record(
        false,
        false,
        10,
        names(&[("REPORT~1.TXT", 2, 5), ("report.txt", 1, 5)]),
        true,
        0,
    );

    let parsed = parse(&mut rec, 40).expect("parses");
    assert_eq!("report.txt", parsed.name);
}

#[test]
fn parse_file_record_reads_hidden_and_directory_flags() {
    let mut rec = build_record(true, true, 0, names(&[("Secret", 1, 5)]), true, 0);

    let parsed = parse(&mut rec, 40).expect("parses");
    assert!(parsed.is_directory);
    assert!(parsed.hidden());
    assert_eq!(0, parsed.size); // directories carry no $DATA size
}

/// The whole `$STANDARD_INFORMATION` attribute mask reaches the record, not just the Hidden bit it
/// used to be reduced to: this is what `fs_entry.attributes` is filled from.
#[test]
fn parse_file_record_keeps_the_whole_attribute_mask() {
    let mut rec = build_record(
        false,
        true,
        10,
        names(&[("boot.ini", 1, 5)]),
        true,
        READONLY | SYSTEM | ARCHIVE,
    );

    let parsed = parse(&mut rec, 40).expect("parses");
    assert_eq!(READONLY | SYSTEM | ARCHIVE | HIDDEN, parsed.attributes);
    assert!(parsed.hidden()); // still readable by name, off the same mask
}

/// Creation time comes from `$STANDARD_INFORMATION + 0x00`, not modified's `+ 0x08`. The two
/// fixtures differ, so reading the wrong one fails here instead of shipping a `dc:` filter that
/// silently answers `dm:`'s question.
#[test]
fn parse_file_record_reads_created_from_its_own_field() {
    let mut rec = build_record(false, false, 10, names(&[("report.txt", 1, 5)]), true, 0);

    let parsed = parse(&mut rec, 40).expect("parses");
    assert_eq!(created(), parsed.created_utc);
    assert_eq!(modified(), parsed.modified_utc);
    assert_ne!(parsed.created_utc, parsed.modified_utc);
}

#[test]
fn parse_file_record_skips_reserved_metafiles() {
    let mut rec = build_record(false, false, 100, names(&[("$MFT", 1, 5)]), true, 0);
    assert!(parse(&mut rec, 0).is_none());
}

#[test]
fn parse_file_record_no_base_data_yields_unknown_size() {
    // A heavily-fragmented file's base record has no unnamed $DATA (it lives in extension
    // records): the parser signals -1 so the caller stats it from disk.
    let mut rec = build_record(
        false,
        false,
        0,
        names(&[("fragmented.pack", 1, 5)]),
        false,
        0,
    );

    let parsed = parse(&mut rec, 40).expect("parses");
    assert_eq!(-1, parsed.size);
}
