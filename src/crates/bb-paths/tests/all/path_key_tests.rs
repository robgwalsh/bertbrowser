// Port of tests/BertBrowser.Core.Tests/PathKeyTests.cs.
use std::collections::HashSet;

use bb_paths::PathKey;

#[test]
fn canonicalize_normalizes_form() {
    for (input, expected) in [
        (r"C:\Foo\Bar", r"C:\FOO\BAR"),
        (r"C:\Foo\Bar\", r"C:\FOO\BAR"),
        (r"C:/Foo/Bar", r"C:\FOO\BAR"),
        (r"C:\Foo\.\Bar", r"C:\FOO\BAR"),
        (r"C:\Foo\Baz\..\Bar", r"C:\FOO\BAR"),
        (r"c:\foo", r"C:\FOO"),
    ] {
        assert_eq!(
            expected,
            PathKey::canonicalize(input).unwrap(),
            "input {input:?}"
        );
    }
}

#[test]
fn canonicalize_drive_root_keeps_trailing_separator() {
    assert_eq!(r"C:\", PathKey::canonicalize(r"C:\").unwrap());
    assert_eq!(r"C:\", PathKey::canonicalize(r"c:\").unwrap());
}

#[test]
fn canonicalize_preserves_special_characters() {
    assert_eq!(
        r"C:\A%B\C_D\E[F]",
        PathKey::canonicalize(r"C:\a%b\c_d\e[f]").unwrap()
    );
}

#[test]
fn canonicalize_unicode_uppercased() {
    assert_eq!(
        r"C:\ÜBUNG\ÉTÉ",
        PathKey::canonicalize(r"C:\übung\été").unwrap()
    );
}

#[test]
fn canonicalize_simple_case_mapping_keeps_sharp_s() {
    // The full mapping would make "straße" and "strasse" one key.
    assert_eq!(r"C:\STRAßE", PathKey::canonicalize(r"C:\straße").unwrap());
}

#[test]
fn canonicalize_empty_throws() {
    assert!(PathKey::canonicalize("").is_err());
    assert!(PathKey::canonicalize("   ").is_err());
}

#[test]
fn normalize_display_keeps_casing() {
    assert_eq!(
        r"C:\Foo\Bar",
        PathKey::normalize_display(r"C:\Foo\Bar\").unwrap()
    );
}

#[test]
fn normalize_display_unc_root_keeps_no_trailing_separator() {
    assert_eq!(
        r"\\server\share",
        PathKey::normalize_display(r"\\server\share\").unwrap()
    );
    assert_eq!(
        r"\\server\share\a",
        PathKey::normalize_display(r"\\server\share\a\").unwrap()
    );
}

#[test]
fn prefix_bounds_simple_directory() {
    let (lo, hi) = PathKey::prefix_bounds(r"C:\Foo").unwrap();
    assert_eq!(r"C:\FOO\", lo);
    assert_eq!(r"C:\FOO]", hi);
}

#[test]
fn prefix_bounds_drive_root_no_doubled_separator() {
    let (lo, hi) = PathKey::prefix_bounds(r"C:\").unwrap();
    assert_eq!(r"C:\", lo);
    assert_eq!(r"C:]", hi);
}

#[test]
fn is_under_respects_subtree_boundaries() {
    for (key, expected) in [
        (r"C:\FOO\FILE.TXT", true),
        (r"C:\FOO\SUB\DEEP\FILE.TXT", true),
        (r"C:\FOO", false),             // the directory itself is not under itself
        (r"C:\FOOBAR\FILE.TXT", false), // sibling with the same name prefix
        (r"C:\OTHER\FILE.TXT", false),
    ] {
        assert_eq!(
            expected,
            PathKey::is_under(key, r"C:\Foo").unwrap(),
            "key {key:?}"
        );
    }
}

#[test]
fn is_under_special_characters_in_directory_name() {
    assert!(PathKey::is_under(r"C:\A%B\X.TXT", r"C:\a%b").unwrap());
    assert!(PathKey::is_under(r"C:\A_B\X.TXT", r"C:\a_b").unwrap());
    assert!(!PathKey::is_under(r"C:\AXB\X.TXT", r"C:\a_b").unwrap()); // '_' is not a wildcard
}

#[test]
fn is_under_drive_root_contains_everything_on_drive() {
    assert!(PathKey::is_under(r"C:\ANY\FILE.TXT", r"C:\").unwrap());
    assert!(!PathKey::is_under(r"D:\ANY\FILE.TXT", r"C:\").unwrap());
}

/// The key-only form has to give `is_under`'s answer for every pair of canonical keys, since it
/// replaces it wherever a loop asks.
#[test]
fn is_under_key_agrees_with_is_under() {
    for (key, dir_key) in [
        (r"C:\FOO\FILE.TXT", r"C:\FOO"),
        (r"C:\FOO\SUB\DEEP\FILE.TXT", r"C:\FOO"),
        (r"C:\FOO", r"C:\FOO"),
        (r"C:\FOOBAR\FILE.TXT", r"C:\FOO"),
        (r"C:\FO", r"C:\FOO"),
        (r"C:\OTHER\FILE.TXT", r"C:\FOO"),
        (r"C:\ANY\FILE.TXT", r"C:\"),
        (r"C:\ANY", r"C:\"),
        (r"C:\", r"C:\"),
        (r"D:\ANY\FILE.TXT", r"C:\"),
        (r"\\SERVER\SHARE\A\B.TXT", r"\\SERVER\SHARE\"),
        (r"\\SERVER\SHARE\A\B.TXT", r"\\SERVER\SHARE\A"),
        (r"\\SERVER\SHARED\A", r"\\SERVER\SHARE\"),
        (r"C:\A]B\X", r"C:\A"),
    ] {
        assert_eq!(
            PathKey::is_under(key, dir_key).unwrap(),
            PathKey::is_under_key(key, dir_key),
            "key {key:?} dir {dir_key:?}"
        );
    }
}

#[test]
fn has_ancestor_key_in_finds_only_real_ancestors() {
    let dirs: HashSet<String> = [r"C:\FOO", r"D:\"].iter().map(|s| s.to_string()).collect();
    for (key, expected) in [
        (r"C:\FOO\FILE.TXT", true),
        (r"C:\FOO\SUB\DEEP\FILE.TXT", true),
        (r"C:\FOO", false), // itself is in the set, and is not its own ancestor
        (r"C:\FOOBAR\FILE.TXT", false),
        (r"D:\DATA\X", true), // under the drive root, whose key keeps its separator
        (r"D:\", false),
        (r"E:\DATA\X", false),
    ] {
        assert_eq!(
            expected,
            PathKey::has_ancestor_key_in(key, &dirs),
            "key {key:?}"
        );
        let by_scan = dirs
            .iter()
            .any(|other| other != key && PathKey::is_under(key, other).unwrap());
        assert_eq!(expected, by_scan, "scan for key {key:?}");
    }
}
