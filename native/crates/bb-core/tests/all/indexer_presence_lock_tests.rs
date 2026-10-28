// Port of tests/BertBrowser.Core.Tests/IndexerPresenceLockTests.cs: the name and the descriptor
// are interpolated from a SID, and both are parsed by the kernel, so what a SID is allowed to be is
// the whole of the safety here. The C# `null` row of the refusal theory is the empty string here.
use bb_core::ipc::{Error, IndexEndpoint, IndexerPresenceLock};

const SID: &str = "S-1-5-21-1845729561-1936007304-1872547691-1001";

#[test]
fn names_the_mutex_per_session_and_per_user() {
    // Local\, not Global\: two users signed in at once must not share one helper's name.
    assert_eq!(
        format!(r"Local\BertBrowser.Indexer.{SID}"),
        IndexerPresenceLock::name_for(SID)
    );
}

#[test]
fn grants_the_user_synchronise_and_nothing_more() {
    let sddl = IndexerPresenceLock::sddl(SID);

    // 0x00100000 is SYNCHRONIZE: enough to answer "is it running?", not enough to acquire it.
    assert_eq!(
        format!("D:(A;;0x00100000;;;{SID})(A;;GA;;;BA)(A;;GA;;;SY)"),
        sddl
    );
}

#[test]
fn accepts_a_real_sid() {
    for candidate in ["S-1-5-21-1-2-3-1001", "S-1-1-0"] {
        assert!(
            IndexerPresenceLock::is_acceptable_sid(candidate),
            "{candidate}"
        );
    }
}

/// The SID lands inside an SDDL string and a kernel object name. A value carrying a bracket would
/// rewrite the descriptor; one carrying a separator would name a different object.
#[test]
fn refuses_anything_else() {
    for candidate in [
        "",
        "S-",
        r"S-1-5-21-1001\..\Global\Evil",
        "S-1-5-21-1001)(A;;GA;;;WD",
        "S-1-5-21-1001;GA",
        "NotASid",
        "S-1-5-21-1001 ",
    ] {
        assert!(
            !IndexerPresenceLock::is_acceptable_sid(candidate),
            "{candidate:?}"
        );
    }
}

#[test]
fn refuses_to_build_a_name_from_a_bad_sid() {
    assert!(matches!(
        IndexEndpoint::for_user("S-1-5-21-1001)(A;;GA;;;WD"),
        Err(Error::UnacceptableSid(_))
    ));
}

#[test]
fn the_endpoint_is_the_prefix_and_the_sid() {
    assert_eq!(
        format!("BertBrowser.Index.{SID}"),
        IndexEndpoint::for_user(SID).unwrap()
    );
}

/// The real thing, end to end: claiming the name makes it visible, a second claim is refused, and
/// letting go makes it invisible again. This is what tells the app whether to raise a prompt, and
/// what stops two helpers indexing at once.
#[test]
fn a_claim_is_visible_until_it_is_released() {
    let sid = IndexEndpoint::current_user_sid().unwrap();
    assert!(
        !IndexerPresenceLock::is_held(&sid),
        "nothing should hold it before the test starts"
    );

    {
        let claim = IndexerPresenceLock::try_acquire(&sid).unwrap();
        assert!(claim.is_some());
        assert!(IndexerPresenceLock::is_held(&sid));
        assert!(IndexerPresenceLock::try_acquire(&sid).unwrap().is_none());
    }

    assert!(!IndexerPresenceLock::is_held(&sid));
}
