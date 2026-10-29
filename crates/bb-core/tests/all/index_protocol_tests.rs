// Port of tests/BertBrowser.Core.Tests/IndexProtocolTests.cs: the wire format an administrator-token
// process reads. Every theory here is a thing a peer must not be able to say; relax
// `IndexProtocol::is_acceptable_root_key` or the verb rule and this file goes red. A C# `null` row
// is the empty string here: a Rust `&str` has no null, and both are refused the same way.
use bb_core::cli::NavigationRequest;
use bb_core::ipc::{IndexMessage, IndexProtocol, IndexVerb};

fn parse(line: &str) -> IndexMessage {
    IndexProtocol::try_parse(line).unwrap_or_else(|| panic!("expected to parse: {line:?}"))
}

fn rejects(line: &str) {
    assert!(IndexProtocol::try_parse(line).is_none(), "{line:?}");
}

#[test]
fn every_verb_round_trips() {
    for (verb, argument) in [
        (IndexVerb::Ready, ""),
        (IndexVerb::Start, ""),
        (IndexVerb::Shutdown, ""),
        (IndexVerb::Ping, ""),
        (IndexVerb::Pong, ""),
        (IndexVerb::Hello, "1"),
        (IndexVerb::Building, "C"),
        (IndexVerb::Idle, "D"),
        (IndexVerb::Complete, r"C:\"),
        (
            IndexVerb::Fatal,
            "The database schema is newer than this helper.",
        ),
        (IndexVerb::Record, "24"),
    ] {
        let message = IndexMessage::new(verb, argument);

        let parsed = parse(&IndexProtocol::format(&message));

        assert_eq!(verb, parsed.verb, "{verb:?} {argument:?}");
        assert_eq!(argument, parsed.argument, "{verb:?} {argument:?}");
    }
}

#[test]
fn formats_as_a_tab_separated_prefixed_line() {
    assert_eq!(
        "IDX\tComplete\tC:\\",
        IndexProtocol::format(&IndexMessage::new(IndexVerb::Complete, r"C:\"))
    );
    assert_eq!(
        "IDX\tStart",
        IndexProtocol::format(&IndexMessage::bare(IndexVerb::Start))
    );
}

#[test]
fn rejects_anything_without_the_right_prefix() {
    for line in [
        "",
        "Start",
        "OPEN\tStart",
        "idx\tStart",
        "IDX",
        " IDX\tStart",
    ] {
        rejects(line);
    }
}

#[test]
fn rejects_an_unknown_verb() {
    for line in [
        "IDX\tReindex\tC:\\",
        "IDX\tDelete\tC:\\",
        "IDX\tstart",
        "IDX\tSTART",
        "IDX\t",
    ] {
        rejects(line);
    }
}

/// `Enum.TryParse` accepted a bare number and a comma-separated list as well as a name. Neither is
/// a verb anyone meant to send, and both would land on a real one.
#[test]
fn rejects_a_numeric_or_composite_verb() {
    for line in ["IDX\t2", "IDX\t0", "IDX\tStart,Shutdown", "IDX\t Start"] {
        rejects(line);
    }
}

#[test]
fn rejects_more_fields_than_a_verb_and_one_argument() {
    rejects("IDX\tComplete\tC:\\\tD:\\");
}

#[test]
fn rejects_an_oversized_line() {
    let line = format!(
        "IDX\tFatal\t{}",
        "a".repeat(NavigationRequest::MAX_LINE_LENGTH)
    );

    rejects(&line);
}

/// The one verb the app may send that carries a value: the change-log retention in hours, 0 for
/// off. Exactly the settings menu and nothing else; this is the elevated end's surface.
#[test]
fn accepts_a_record_with_a_retention_on_the_menu() {
    for hours in ["0", "1", "6", "24", "168"] {
        let message = parse(&format!("IDX\tRecord\t{hours}"));

        assert_eq!(IndexVerb::Record, message.verb, "{hours}");
        assert_eq!(hours, message.argument, "{hours}");
    }
}

#[test]
fn rejects_a_record_off_the_menu() {
    for line in [
        "IDX\tRecord",
        "IDX\tRecord\t",
        "IDX\tRecord\t2",
        "IDX\tRecord\t25",
        "IDX\tRecord\t-1",
        "IDX\tRecord\t024",
        "IDX\tRecord\t 24",
        "IDX\tRecord\t24h",
        "IDX\tRecord\tC:\\",
    ] {
        rejects(line);
    }
}

#[test]
fn accepts_a_bare_drive_letter() {
    for drive in ["C", "Z"] {
        assert!(IndexProtocol::is_acceptable_drive(drive), "{drive}");
    }
}

#[test]
fn rejects_anything_that_is_not_a_bare_drive_letter() {
    for drive in ["", "c", "C:", "C:\\", "CD", "1", "\\"] {
        assert!(!IndexProtocol::is_acceptable_drive(drive), "{drive:?}");
    }
}

#[test]
fn rejects_a_building_or_idle_that_is_not_a_drive_letter() {
    for line in [
        "IDX\tBuilding\tC:",
        "IDX\tBuilding\tc",
        "IDX\tBuilding",
        "IDX\tIdle\tC:\\WINDOWS",
    ] {
        rejects(line);
    }
}

#[test]
fn accepts_a_canonical_volume_root() {
    for root in [r"C:\", r"Z:\"] {
        assert!(IndexProtocol::is_acceptable_root_key(root), "{root}");
    }
}

/// The rule the elevated end's state feeds: anything accepted here becomes a root the search router
/// treats as fully indexed. Deeper paths, relative paths, device paths and UNC shares are all
/// refused; a volume root is the only thing the indexer ever completes.
#[test]
fn rejects_anything_that_is_not_a_volume_root() {
    for root in [
        "",
        r"C:",
        r"C:\WINDOWS",
        r"c:\",
        r"C:/",
        r"\\SERVER\SHARE",
        r"\\.\C:",
        r"\\?\C:\",
        r"..\",
        r"\",
        r"C:\*",
    ] {
        assert!(!IndexProtocol::is_acceptable_root_key(root), "{root:?}");
    }
}

#[test]
fn rejects_a_complete_carrying_a_control_character() {
    rejects("IDX\tComplete\tC:\\\u{0}");
}

#[test]
fn accepts_a_plain_status_line() {
    for status in [
        "Indexing C:…",
        "The database schema is newer than this helper.",
    ] {
        assert!(IndexProtocol::is_acceptable_status(status), "{status}");
    }
}

#[test]
fn rejects_a_status_that_is_empty_or_carries_control_characters() {
    for status in ["", "two\nlines", "a\u{0}b"] {
        assert!(!IndexProtocol::is_acceptable_status(status), "{status:?}");
    }
}

#[test]
fn rejects_an_overlong_status() {
    assert!(!IndexProtocol::is_acceptable_status(
        &"a".repeat(IndexProtocol::MAX_STATUS_LENGTH + 1)
    ));
    assert!(IndexProtocol::is_acceptable_status(
        &"a".repeat(IndexProtocol::MAX_STATUS_LENGTH)
    ));
}

#[test]
fn rejects_a_hello_without_a_positive_version() {
    for line in [
        "IDX\tHello\t0",
        "IDX\tHello\t-1",
        "IDX\tHello\tone",
        "IDX\tHello",
    ] {
        rejects(line);
    }
}

#[test]
fn reads_the_version_off_a_hello() {
    assert_eq!(Some(1), IndexProtocol::version_of(&parse("IDX\tHello\t1")));
    assert_eq!(Some(7), IndexProtocol::version_of(&parse("IDX\tHello\t7")));
    assert_eq!(None, IndexProtocol::version_of(&parse("IDX\tStart")));
}

/// Verbs that say everything by arriving must not smuggle a payload; that is what keeps the
/// elevated surface free of anything an attacker chooses.
#[test]
fn rejects_an_argument_on_a_verb_that_takes_none() {
    for line in [
        "IDX\tStart\tC:\\WINDOWS",
        "IDX\tShutdown\tnow",
        "IDX\tPing\tanything",
        "IDX\tReady\tC:\\",
    ] {
        rejects(line);
    }
}

#[test]
fn the_elevated_surface_is_four_verbs_and_none_of_them_names_a_path() {
    let accepted_by_the_helper = [
        IndexVerb::Hello,
        IndexVerb::Start,
        IndexVerb::Shutdown,
        IndexVerb::Ping,
    ];

    for verb in accepted_by_the_helper {
        assert!(
            !IndexProtocol::is_acceptable_argument(verb, r"C:\Windows\System32"),
            "{verb:?}"
        );
        assert!(
            !IndexProtocol::is_acceptable_argument(verb, r"\\server\share"),
            "{verb:?}"
        );
    }
}
