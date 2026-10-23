// Port of tests/BertBrowser.Core.Tests/NavigationRequestTests.cs: the one rule standing between an
// elevated process and a path handed to it over a pipe.
use bb_core::cli::{CommandLineRequest, NavigationRequest, OpenIn, OpenTarget};

// --- what is allowed ---

#[test]
fn an_absolute_local_or_network_path_is_accepted() {
    for path in [
        r"C:\",
        r"C:\Dir",
        r"C:\Dir\file.txt",
        r"C:\Program Files\Some App",
        r"\\server\share",
        r"\\server\share\folder",
        r"D:\folder with spaces\and-dashes",
    ] {
        assert!(NavigationRequest::is_acceptable_path(path), "{path}");
    }
}

// --- what is refused ---

#[test]
fn nothing_at_all_is_refused() {
    for path in ["", "   "] {
        assert!(!NavigationRequest::is_acceptable_path(path), "{path:?}");
    }
}

/// A relative path resolves against whatever directory this process happens to be in.
#[test]
fn a_relative_path_is_refused() {
    for path in ["Dir", r"..\..\Windows", r".\Dir", r"\Dir", "C:Dir"] {
        assert!(!NavigationRequest::is_acceptable_path(path), "{path}");
    }
}

/// These do not name files at all, and this process is elevated.
#[test]
fn a_win32_device_path_is_refused() {
    for path in [r"\\.\PhysicalDrive0", r"\\.\C:", r"\\?\C:\Dir"] {
        assert!(!NavigationRequest::is_acceptable_path(path), "{path}");
    }
}

/// A newline would let one message forge a second; a NUL truncates on the way into Win32; a tab
/// would break the format apart from the inside.
#[test]
fn a_control_character_is_refused() {
    for path in [
        "C:\\Dir\nOPEN\tDefault",
        "C:\\Dir\r\n",
        "C:\\Dir\0extra",
        "C:\\Dir\tmore",
    ] {
        assert!(!NavigationRequest::is_acceptable_path(path), "{path:?}");
    }
}

#[test]
fn a_wildcard_or_other_illegal_character_is_refused() {
    for path in [r"C:\Dir\*", r"C:\Dir\?.txt", r"C:\Dir\a|b", "C:\\Dir\\a\"b"] {
        assert!(!NavigationRequest::is_acceptable_path(path), "{path}");
    }
}

#[test]
fn an_implausibly_long_path_is_refused() {
    assert!(!NavigationRequest::is_acceptable_path(&format!(
        r"C:\{}",
        "a".repeat(5000)
    )));
}

// --- the wire format ---

#[test]
fn a_request_survives_the_round_trip() {
    let original = CommandLineRequest {
        targets: vec![
            OpenTarget::new(r"C:\Dir", false),
            OpenTarget::new(r"C:\Other\file.txt", true),
        ],
        mode: OpenIn::NewTab,
        errors: vec![],
    };

    let parsed = NavigationRequest::try_parse(&NavigationRequest::format(&original)).unwrap();

    assert_eq!(OpenIn::NewTab, parsed.mode);
    assert_eq!(2, parsed.targets.len());
    assert_eq!(r"C:\Dir", parsed.targets[0].path);
    assert!(!parsed.targets[0].select);
    assert_eq!(r"C:\Other\file.txt", parsed.targets[1].path);
    assert!(parsed.targets[1].select);
}

#[test]
fn a_path_with_spaces_survives_the_round_trip() {
    let original = CommandLineRequest {
        targets: vec![OpenTarget::new(r"C:\Program Files\Some App", false)],
        mode: OpenIn::Default,
        errors: vec![],
    };

    let parsed = NavigationRequest::try_parse(&NavigationRequest::format(&original)).unwrap();
    assert_eq!(1, parsed.targets.len());
    assert_eq!(r"C:\Program Files\Some App", parsed.targets[0].path);
}

#[test]
fn a_request_with_no_targets_still_round_trips() {
    let parsed =
        NavigationRequest::try_parse(&NavigationRequest::format(&CommandLineRequest::default()))
            .unwrap();
    assert!(parsed.targets.is_empty());
}

/// The formatter is the second line of defence: an unacceptable path must not even go on the wire.
#[test]
fn formatting_drops_a_path_that_would_not_be_accepted() {
    let request = CommandLineRequest {
        targets: vec![
            OpenTarget::new(r"..\..\Windows", false),
            OpenTarget::new(r"C:\Dir", false),
        ],
        mode: OpenIn::Default,
        errors: vec![],
    };

    let parsed = NavigationRequest::try_parse(&NavigationRequest::format(&request)).unwrap();
    assert_eq!(1, parsed.targets.len());
    assert_eq!(r"C:\Dir", parsed.targets[0].path);
}

#[test]
fn a_malformed_line_is_rejected() {
    for line in ["", "garbage", "OPEN", "NOTOPEN\tDefault", "OPEN\tNotAMode"] {
        assert!(NavigationRequest::try_parse(line).is_none(), "{line:?}");
    }
}

#[test]
fn an_oversized_line_is_rejected() {
    let line = format!(
        "OPEN\tDefault\t-{}",
        "a".repeat(NavigationRequest::MAX_LINE_LENGTH)
    );
    assert!(NavigationRequest::try_parse(&line).is_none());
}

/// A half-corrupt request opens the parts that were sound rather than nothing at all.
#[test]
fn a_bad_field_is_dropped_without_costing_the_good_ones() {
    let parsed = NavigationRequest::try_parse("OPEN\tDefault\tXnonsense\t-C:\\Dir").unwrap();
    assert_eq!(1, parsed.targets.len());
    assert_eq!(r"C:\Dir", parsed.targets[0].path);
}

/// The parser re-checks rather than trusting whoever wrote the line.
#[test]
fn an_unacceptable_path_on_the_wire_is_dropped_by_the_parser_too() {
    let parsed = NavigationRequest::try_parse("OPEN\tDefault\t-..\\..\\Windows").unwrap();
    assert!(parsed.targets.is_empty());
}
