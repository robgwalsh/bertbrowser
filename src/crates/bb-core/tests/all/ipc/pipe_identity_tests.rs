// Port of tests/BertBrowser.Core.Tests/PipeIdentityTests.cs: the comparison that fails *closed*
// when it is wrong: a pipe that accepts every connection and drops it, rather than an error anyone
// would notice. A C# `null` name has no Rust form; the unknown-identity test uses the empty string
// for the one-sided rows, which the C# refuses the same way.
use bb_core::ipc::PipeIdentity;

/// The exact pair the two Windows APIs actually hand back.
#[test]
fn the_two_forms_of_the_same_account_match() {
    for (left, right) in [
        ("Rob", "DESKTOP-K0BI3BS\\Rob"),
        ("DESKTOP-K0BI3BS\\Rob", "Rob"),
        ("Rob", "Rob"),
        ("CONTOSO\\Rob", "DESKTOP-K0BI3BS\\Rob"),
    ] {
        assert!(PipeIdentity::same_account(left, right), "{left} vs {right}");
    }
}

#[test]
fn different_accounts_do_not_match() {
    for (left, right) in [
        ("Rob", "Sam"),
        ("DESKTOP\\Rob", "DESKTOP\\Sam"),
        ("Rob", "DESKTOP\\Robert"),
        ("Rob", ""),
    ] {
        assert!(
            !PipeIdentity::same_account(left, right),
            "{left} vs {right}"
        );
    }
}

/// Windows account names are case-insensitive.
#[test]
fn comparison_ignores_case() {
    assert!(PipeIdentity::same_account("rob", "DESKTOP\\ROB"));
}

#[test]
fn an_unknown_identity_does_not_match() {
    assert!(!PipeIdentity::same_account("", "DESKTOP\\Rob"));
    assert!(!PipeIdentity::same_account("Rob", ""));
}

#[test]
fn account_part_drops_any_domain_prefix() {
    for (name, expected) in [
        ("Rob", "Rob"),
        ("DESKTOP\\Rob", "Rob"),
        ("CONTOSO\\sub\\Rob", "Rob"),
        ("", ""),
    ] {
        assert_eq!(expected, PipeIdentity::account_part(name), "{name}");
    }
}
