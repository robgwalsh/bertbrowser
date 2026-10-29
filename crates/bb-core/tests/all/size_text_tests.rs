// Port of tests/BertBrowser.Core.Tests/SizeTextTests.cs.
use bb_core::format::byte_size;
use bb_core::search::SizeText;

#[test]
fn parses_sizes_with_units() {
    for (text, expected) in [
        ("0", 0i64),
        ("512", 512),
        ("512b", 512),
        ("1k", 1024),
        ("1kb", 1024),
        ("1KB", 1024),
        ("  2 mb ", 2 * 1024 * 1024),
        ("1.5mb", (1.5 * 1024.0 * 1024.0) as i64),
        ("3gb", 3 * 1024 * 1024 * 1024),
        ("1tb", 1 << 40),
    ] {
        assert_eq!(Some(expected), SizeText::try_parse(text), "{text:?}");
    }
}

#[test]
fn refuses_what_is_not_a_size() {
    for text in ["", "   ", "mb", "kb", "-5mb", "abc", "10 apples", "1e400"] {
        assert_eq!(None, SizeText::try_parse(text), "{text:?}");
    }
}

/// The units have to agree with the Size column: at 1000-based multiples a 104,857,600-byte file is
/// "over 100mb" to the filter and "100 MB" to the list. This pins 1024.
#[test]
fn units_are_the_same_ones_the_size_column_prints() {
    let hundred_mb = SizeText::try_parse("100mb").unwrap();
    assert_eq!("100 MB", byte_size(hundred_mb));

    let one_gb = SizeText::try_parse("1gb").unwrap();
    assert!(byte_size(one_gb).starts_with('1'));
    assert!(byte_size(one_gb).ends_with("GB"));
}
