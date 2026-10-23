// Port of tests/BertBrowser.Core.Tests/InstanceEndpointTests.cs: the single-instance endpoint name.
// What is being defended here is not the app against its own user (it cannot be) but one signed-in
// account against another: pipe names are machine-wide and first-come, so a guessable name is one
// somebody else can take. Mutate `InstanceEndpoint::is_acceptable` to wave a name through and the
// theories below go red. A C# `null` row is the empty string here: a Rust `&str` has no null, and
// both are refused the same way.
use bb_core::ipc::InstanceEndpoint;

const SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
const NONCE: &str = "0123456789ABCDEF0123456789ABCDEF";

#[test]
fn a_generated_name_is_accepted() {
    let name = InstanceEndpoint::name(SID, NONCE);

    assert_eq!(format!("BertBrowser.{SID}.{NONCE}"), name);
    assert!(InstanceEndpoint::is_acceptable(&name, SID));
}

/// The whole point of the design: without the nonce the name is derivable from the SID alone, which
/// is public. Accepting a bare prefix would quietly restore the guessable endpoint.
#[test]
fn a_name_carrying_no_nonce_is_refused() {
    assert!(!InstanceEndpoint::is_acceptable(
        &format!("BertBrowser.{SID}"),
        SID
    ));
    assert!(!InstanceEndpoint::is_acceptable(
        &format!("BertBrowser.{SID}."),
        SID
    ));
}

#[test]
fn a_nonce_of_the_wrong_shape_is_refused() {
    for nonce in [
        "0123456789ABCDEF0123456789ABCDE",   // one short
        "0123456789ABCDEF0123456789ABCDEF0", // one long
        "0123456789abcdef0123456789abcdef",  // lower case is not what we emit
        "0123456789ABCDEF0123456789ABCDEG",  // 'G' is not hex
    ] {
        assert!(
            !InstanceEndpoint::is_acceptable(&format!("BertBrowser.{SID}.{nonce}"), SID),
            "{nonce}"
        );
    }
}

/// The name is interpolated into `\\.\pipe\`, so anything that could steer it somewhere else has to
/// fail. These all do already (the nonce rule admits hex and nothing else) and the theory is here so
/// that stays true if the shape is ever loosened.
#[test]
fn a_name_that_could_name_something_else_is_refused() {
    for nonce in [
        r"..\..\evil",
        r"sub\path",
        "wild*card",
        "with space",
        "new\nline",
        "nul\0byte",
    ] {
        assert!(
            !InstanceEndpoint::is_acceptable(&format!("BertBrowser.{SID}.{nonce}"), SID),
            "{nonce:?}"
        );
    }
}

#[test]
fn another_accounts_endpoint_is_refused() {
    let theirs = InstanceEndpoint::name("S-1-5-21-9-9-9-500", NONCE);

    assert!(!InstanceEndpoint::is_acceptable(&theirs, SID));
}

#[test]
fn an_endpoint_this_app_never_produced_is_refused() {
    for candidate in [
        "",
        "   ",
        "BertBrowser.Index.ABC", // the other pipe in this app
        "Something.Else",
    ] {
        assert!(
            !InstanceEndpoint::is_acceptable(candidate, SID),
            "{candidate:?}"
        );
    }
}

#[test]
fn an_unusable_user_key_is_refused_rather_than_matching_everything() {
    let name = InstanceEndpoint::name(SID, NONCE);

    assert!(!InstanceEndpoint::is_acceptable(&name, ""));
}

#[test]
fn an_oversized_name_is_refused() {
    let huge = "A".repeat(InstanceEndpoint::MAX_NAME_LENGTH);

    assert!(!InstanceEndpoint::is_acceptable(
        &format!("BertBrowser.{SID}.{huge}"),
        SID
    ));
}
