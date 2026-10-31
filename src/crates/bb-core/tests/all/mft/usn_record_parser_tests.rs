// Port of tests/BertBrowser.Core.Tests/UsnRecordParserTests.cs.
use bb_core::FileTime;
use bb_core::mft::UsnRecordParser;
use bb_core::mft::fixtures::{build_usn_record, with_cursor};

/// Builds one USN_RECORD_V2 with the given fields, 8-byte aligned like the OS.
fn record(frn: u64, parent_frn: u64, reason: u32, attributes: u32, name: &str) -> Vec<u8> {
    build_usn_record(frn, parent_frn, reason, attributes, name, 2, 0, 0)
}

#[test]
fn parse_reads_fields_and_name() {
    const DIRECTORY: u32 = 0x10; // FILE_ATTRIBUTE_DIRECTORY
    let buffer = with_cursor(0, &[record(0x111, 0x5, 0x102, DIRECTORY, "My Folder")]);

    let recs = UsnRecordParser::parse(&buffer, 8, buffer.len());
    assert_eq!(1, recs.len());
    let rec = &recs[0];

    assert_eq!(0x111, rec.file_reference_number);
    assert_eq!(0x5, rec.parent_file_reference_number);
    assert_eq!(0x102, rec.reason);
    assert_eq!("My Folder", rec.name);
    assert!(rec.is_directory());
    assert!(!rec.is_hidden());
}

#[test]
fn parse_reads_multiple_records() {
    let buffer = with_cursor(
        0,
        &[
            record(1, 5, 0, 0, "a.txt"),
            record(2, 5, 0, 0x2 /* hidden */, "b.txt"),
        ],
    );

    let recs = UsnRecordParser::parse(&buffer, 8, buffer.len());

    assert_eq!(2, recs.len());
    assert_eq!("a.txt", recs[0].name);
    assert!(!recs[0].is_hidden());
    assert_eq!("b.txt", recs[1].name);
    assert!(recs[1].is_hidden());
}

#[test]
fn parse_skips_non_v2_records() {
    // A V3 record (128-bit refs, different layout) must not be misread as V2.
    let buffer = with_cursor(0, &[build_usn_record(1, 5, 0, 0, "skip.me", 3, 0, 0)]);

    assert!(UsnRecordParser::parse(&buffer, 8, buffer.len()).is_empty());
}

#[test]
fn parse_reads_usn_and_timestamp() {
    let stamp = FileTime::from_ymd_hms(2026, 9, 2, 10, 30, 0);
    let buffer = with_cursor(
        0,
        &[build_usn_record(
            1,
            5,
            0,
            0,
            "a.txt",
            2,
            0x1234_5678,
            stamp.0,
        )],
    );

    let recs = UsnRecordParser::parse(&buffer, 8, buffer.len());
    assert_eq!(1, recs.len());

    assert_eq!(0x1234_5678, recs[0].usn);
    assert_eq!(stamp, recs[0].timestamp_utc);
}

#[test]
fn parse_zero_timestamp_is_min_value() {
    // FSCTL_ENUM_USN_DATA leaves the field 0; it must not become 1601-01-01.
    let buffer = with_cursor(0, &[record(1, 5, 0, 0, "a.txt")]);

    let recs = UsnRecordParser::parse(&buffer, 8, buffer.len());
    assert_eq!(1, recs.len());

    assert_eq!(FileTime::UNKNOWN, recs[0].timestamp_utc);
}
