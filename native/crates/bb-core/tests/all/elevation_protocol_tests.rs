// Port of tests/BertBrowser.Core.Tests/ElevationProtocolTests.cs: the line format the elevated
// file-operation helper speaks. Everything crossing it is untrusted, so the only acceptable
// behaviour for a malformed line is a refusal: never a panic, and never a partial read. A C# `null`
// row is the empty string here: a Rust `&str` has no null, and both are refused the same way.
use bb_core::cli::NavigationRequest;
use bb_core::ipc::{ElevationMessage, ElevationProtocol, ElevationVerb, ElevatorArguments};

#[test]
fn round_trips() {
    for (verb, payload) in [
        (ElevationVerb::Ready, ""),
        (ElevationVerb::Go, ""),
        (ElevationVerb::Cancel, ""),
        (ElevationVerb::Hello, "1"),
        (ElevationVerb::Begin, "{\"Operation\":0}"),
        (ElevationVerb::Fatal, "it went wrong"),
    ] {
        let line = ElevationProtocol::format(&ElevationMessage::new(verb, payload));

        let parsed = ElevationProtocol::try_parse(&line)
            .unwrap_or_else(|| panic!("expected to parse: {line:?}"));
        assert_eq!(verb, parsed.verb, "{verb:?} {payload:?}");
        assert_eq!(payload, parsed.payload, "{verb:?} {payload:?}");
    }
}

#[test]
fn a_payload_containing_a_tab_survives() {
    // The format splits into at most three fields, so a payload may hold a separator without being
    // cut in half, which matters because JSON escapes a tab inside a string rather than removing it.
    let payload = "{\"Name\":\"a\\tb\"}";

    let parsed = ElevationProtocol::try_parse(&ElevationProtocol::format(&ElevationMessage::new(
        ElevationVerb::Item,
        payload,
    )))
    .expect("expected to parse");
    assert_eq!(payload, parsed.payload);
}

#[test]
fn refuses_a_line_that_is_not_one_of_ours() {
    for line in [
        "",
        "OPS",
        "IDX\tReady",
        "ops\tReady",
        "OPS\tSomethingElse",
        "OPS\t3",
        "OPS\tDone, Fault",
    ] {
        assert!(ElevationProtocol::try_parse(line).is_none(), "{line:?}");
    }
}

#[test]
fn the_verbs_that_carry_nothing_carry_nothing() {
    for verb in [
        ElevationVerb::Ready,
        ElevationVerb::Go,
        ElevationVerb::Cancel,
    ] {
        assert!(
            !ElevationProtocol::is_acceptable_payload(verb, "{}"),
            "{verb:?}"
        );
    }
}

#[test]
fn the_verbs_that_carry_a_record_need_one() {
    for verb in [
        ElevationVerb::Begin,
        ElevationVerb::Item,
        ElevationVerb::Done,
        ElevationVerb::Fault,
        ElevationVerb::End,
        ElevationVerb::Progress,
    ] {
        assert!(
            !ElevationProtocol::is_acceptable_payload(verb, ""),
            "{verb:?}"
        );
    }
}

#[test]
fn a_greeting_without_a_version_is_refused() {
    for payload in ["0", "-1", "one", ""] {
        assert!(
            !ElevationProtocol::is_acceptable_payload(ElevationVerb::Hello, payload),
            "{payload:?}"
        );
    }
}

#[test]
fn a_line_longer_than_the_cap_is_refused() {
    assert!(
        ElevationProtocol::try_parse(&format!(
            "OPS\tItem\t{}",
            "x".repeat(NavigationRequest::MAX_LINE_LENGTH)
        ))
        .is_none()
    );
}

#[test]
fn the_item_cap_is_what_bounds_a_request_rather_than_the_line_length() {
    // The two bounds are in different dimensions on purpose: one record per line keeps the line cap
    // where LineChannel put it, and the count keeps the request finite. A cap that has to be raised
    // to fit a big operation is not a cap.
    const {
        assert!(ElevationProtocol::MAX_ITEMS > 0);
        assert!(NavigationRequest::MAX_LINE_LENGTH == 64 * 1024);
    }
}

#[test]
fn a_status_is_flattened_onto_one_line_and_bounded() {
    let summary =
        ElevationProtocol::summarize(&format!("something\r\nbroke\tbadly{}", "!".repeat(500)));

    assert!(!summary.contains('\n'));
    assert!(!summary.contains('\t'));
    assert!(summary.chars().count() <= ElevationProtocol::MAX_STATUS_LENGTH);
    assert!(ElevationProtocol::is_acceptable_payload(
        ElevationVerb::Fatal,
        &summary
    ));
}

#[test]
fn an_empty_status_still_says_something() {
    assert!(!ElevationProtocol::summarize("   ").is_empty());
}

// --- the helper's command line ---

#[test]
fn the_helper_takes_a_pipe_a_parent_and_a_sid() {
    let parsed = ElevatorArguments::try_parse(&[
        "--pipe",
        "BertBrowser.Elevate.ABC",
        "--parent-pid",
        "42",
        "--user-sid",
        "S-1-5-21-1-2-3",
    ])
    .expect("expected to parse");

    assert_eq!("BertBrowser.Elevate.ABC", parsed.pipe_name);
    assert_eq!(42, parsed.parent_process_id);
    assert_eq!("S-1-5-21-1-2-3", parsed.user_sid);
}

#[test]
fn the_helper_refuses_an_unrecognised_argument() {
    assert!(
        ElevatorArguments::try_parse(&[
            "--pipe",
            "BertBrowser.Elevate.ABC",
            "--parent-pid",
            "42",
            "--user-sid",
            "S-1-5-1",
            "--data-dir",
            r"C:\x",
        ])
        .is_err()
    );
}

#[test]
fn the_helper_refuses_an_index_pipe() {
    // Separate prefixes so neither helper can ever be handed the other's endpoint.
    assert!(!ElevatorArguments::is_acceptable_pipe_name(
        "BertBrowser.Index.ABC"
    ));
}

#[test]
fn the_helper_refuses_an_endpoint_it_should_not_touch() {
    for name in [
        "",
        r"BertBrowser.Elevate.\..\evil",
        "BertBrowser.Elevate.a:b",
        "Something.Else",
    ] {
        assert!(
            !ElevatorArguments::is_acceptable_pipe_name(name),
            "{name:?}"
        );
    }
}

#[test]
fn the_helper_refuses_something_that_is_not_a_sid() {
    for sid in ["", "Rob", "S-1-5-21-../x"] {
        assert!(!ElevatorArguments::is_acceptable_sid(sid), "{sid:?}");
    }
}
