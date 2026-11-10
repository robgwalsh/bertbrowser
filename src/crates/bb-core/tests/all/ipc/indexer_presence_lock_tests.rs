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
/// what stops two helpers indexing at once. Everyone's SID rather than this user's, whose name a real
/// helper may well be holding, and one the descriptor still grants to an unelevated test run.
#[test]
fn a_claim_is_visible_until_it_is_released() {
    let sid = "S-1-1-0";
    assert!(
        !IndexerPresenceLock::is_held(sid),
        "nothing should hold it before the test starts"
    );

    {
        let claim = IndexerPresenceLock::try_acquire(sid).unwrap();
        assert!(claim.is_some());
        assert!(IndexerPresenceLock::is_held(sid));
        assert!(IndexerPresenceLock::try_acquire(sid).unwrap().is_none());
    }

    assert!(!IndexerPresenceLock::is_held(sid));
}

// Formerly the crate's own tests of IndexerPresenceLock and IndexEndpoint.

/// Authenticated Users: no helper runs under it, so a real one running for the developer cannot
/// be the one holding the name, and the descriptor still grants it to an unelevated test run.
const AUTHENTICATED_USERS: &str = "S-1-5-11";

#[test]
fn the_sddl_grants_the_user_synchronise_only() {
    assert_eq!(
        "D:(A;;0x00100000;;;S-1-5-21-1-2-3)(A;;GA;;;BA)(A;;GA;;;SY)",
        IndexerPresenceLock::sddl("S-1-5-21-1-2-3")
    );
    assert_eq!(
        r"Local\BertBrowser.Indexer.S-1-5-21-1-2-3",
        IndexerPresenceLock::name_for("S-1-5-21-1-2-3")
    );
}

/// Against the real kernel: the claim is visible while held, refused to a second claimant, and
/// gone when dropped.
#[test]
fn a_claim_is_visible_exclusive_and_released_on_drop() {
    assert!(!IndexerPresenceLock::is_held(AUTHENTICATED_USERS));

    let claim = IndexerPresenceLock::try_acquire(AUTHENTICATED_USERS)
        .unwrap()
        .expect("the name was free");
    assert!(IndexerPresenceLock::is_held(AUTHENTICATED_USERS));
    assert!(
        IndexerPresenceLock::try_acquire(AUTHENTICATED_USERS)
            .unwrap()
            .is_none()
    );

    drop(claim);
    assert!(!IndexerPresenceLock::is_held(AUTHENTICATED_USERS));
}

#[test]
fn a_sid_that_could_rewrite_the_descriptor_is_refused() {
    for sid in [
        "",
        "S-",
        "S-1-5-",
        "S-1-5-21-1)(A;;GA;;;WD",
        r"S-1-5\x",
        "Rob",
    ] {
        assert!(!IndexerPresenceLock::is_acceptable_sid(sid), "{sid:?}");
        assert!(IndexerPresenceLock::try_acquire(sid).is_err(), "{sid:?}");
        assert!(!IndexerPresenceLock::is_held(sid), "{sid:?}");
    }
    assert!(IndexerPresenceLock::is_acceptable_sid(AUTHENTICATED_USERS));
}

/// Against the real token: what comes back is a AUTHENTICATED_USERS the name rule accepts, so the two
/// processes derive one name from it.
#[test]
fn the_current_user_sid_is_one_the_endpoint_accepts() {
    let sid = IndexEndpoint::current_user_sid().expect("a user AUTHENTICATED_USERS");
    assert!(IndexerPresenceLock::is_acceptable_sid(&sid), "{sid}");
    assert_eq!(
        format!("BertBrowser.Index.{sid}"),
        IndexEndpoint::for_current_user().unwrap()
    );
}

#[test]
fn a_sid_this_app_would_not_produce_is_refused() {
    assert_eq!(
        Err(Error::UnacceptableSid("Rob".to_string())),
        IndexEndpoint::for_user("Rob")
    );
}
