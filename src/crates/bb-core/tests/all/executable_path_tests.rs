// Port of tests/BertBrowser.Core.Tests/ExecutablePathTests.cs.
//
// The search order is the point. A launch no longer throws when a program is missing, so a None
// here is the only thing standing between "Windows Terminal is not installed" and a menu item that
// silently does nothing — and an absolute path is what stops an elevated process resolving a bare
// name through a search path someone else can write to.
use std::cell::RefCell;
use std::io;

use bb_core::launch::ExecutablePath;

const PATH_EXT: &str = ".COM;.EXE;.BAT;.CMD";

/// A filesystem that contains exactly what it is told to, compared the way Windows compares paths.
fn existing(paths: &[&str]) -> impl Fn(&str) -> io::Result<bool> {
    let set: Vec<String> = paths.iter().map(|p| p.to_lowercase()).collect();
    move |path: &str| Ok(set.contains(&path.to_lowercase()))
}

/// Compared case-insensitively because Windows paths are: a name resolved by appending a PATHEXT
/// entry carries that entry's casing (`.EXE`), and pinning the case would assert something neither
/// Windows nor this code promises.
fn assert_path(expected: &str, actual: Option<String>) {
    assert_eq!(
        Some(expected.to_lowercase()),
        actual.map(|a| a.to_lowercase())
    );
}

#[test]
fn a_full_path_that_exists_is_taken_as_is() {
    assert_path(
        r"C:\Program Files\Microsoft VS Code\Code.exe",
        ExecutablePath::resolve(
            Some(r"C:\Program Files\Microsoft VS Code\Code.exe"),
            Some(r"C:\Windows"),
            Some(PATH_EXT),
            existing(&[r"C:\Program Files\Microsoft VS Code\Code.exe"]),
        ),
    );
}

#[test]
fn a_full_path_that_is_gone_is_null_and_is_not_looked_for_on_the_path() {
    // The name is on the PATH, but the caller asked for a specific file. Falling back would start
    // a different program than the one named.
    let resolved = ExecutablePath::resolve(
        Some(r"C:\gone\tool.exe"),
        Some(r"C:\Windows"),
        Some(PATH_EXT),
        existing(&[r"C:\Windows\tool.exe"]),
    );

    assert_eq!(None, resolved);
}

#[test]
fn a_relative_path_with_a_separator_is_not_searched_on_the_path() {
    assert_eq!(
        None,
        ExecutablePath::resolve(
            Some(r"sub\tool.exe"),
            Some(r"C:\Windows"),
            Some(PATH_EXT),
            existing(&[r"C:\Windows\tool.exe"]),
        )
    );
}

#[test]
fn a_path_that_is_not_fully_qualified_is_never_probed() {
    // Each of these resolves against the current directory, which for an elevated process is
    // wherever it was started from. Whatever is there must never get to decide what runs.
    for program in [
        r"sub\tool.exe",
        r".\tool.exe",
        r"..\tool.exe",
        "/tool.exe",
        r"\tool.exe",
    ] {
        let probed: RefCell<Vec<String>> = RefCell::new(Vec::new());

        let resolved =
            ExecutablePath::resolve(Some(program), Some(r"C:\Windows"), Some(PATH_EXT), |path| {
                probed.borrow_mut().push(path.to_string());
                Ok(true) // a filesystem that says yes to everything
            });

        assert_eq!(None, resolved, "{program}");
        assert!(probed.borrow().is_empty(), "{program}");
    }
}

#[test]
fn a_bare_name_is_found_on_the_path() {
    assert_path(
        r"C:\Windows\System32\wt.exe",
        ExecutablePath::resolve(
            Some("wt.exe"),
            Some(r"C:\Windows;C:\Windows\System32"),
            Some(PATH_EXT),
            existing(&[r"C:\Windows\System32\wt.exe"]),
        ),
    );
}

#[test]
fn the_path_is_searched_in_order() {
    assert_path(
        r"C:\first\tool.exe",
        ExecutablePath::resolve(
            Some("tool.exe"),
            Some(r"C:\first;C:\second"),
            Some(PATH_EXT),
            existing(&[r"C:\first\tool.exe", r"C:\second\tool.exe"]),
        ),
    );
}

#[test]
fn a_name_with_no_extension_picks_up_one_from_path_ext() {
    assert_path(
        r"C:\tools\code.cmd",
        ExecutablePath::resolve(
            Some("code"),
            Some(r"C:\tools"),
            Some(PATH_EXT),
            existing(&[r"C:\tools\code.cmd"]),
        ),
    );
}

#[test]
fn path_ext_is_tried_in_its_own_order() {
    assert_path(
        r"C:\tools\code.exe",
        ExecutablePath::resolve(
            Some("code"),
            Some(r"C:\tools"),
            Some(PATH_EXT),
            existing(&[r"C:\tools\code.exe", r"C:\tools\code.cmd"]),
        ),
    );
}

#[test]
fn extensions_are_not_appended_to_a_name_that_already_has_one() {
    // Otherwise "wt.exe" could be answered by "wt.exe.bat" sitting earlier on the PATH.
    let resolved = ExecutablePath::resolve(
        Some("wt.exe"),
        Some(r"C:\tools"),
        Some(PATH_EXT),
        existing(&[r"C:\tools\wt.exe.bat"]),
    );

    assert_eq!(None, resolved);
}

#[test]
fn a_program_that_is_not_there_is_null() {
    assert_eq!(
        None,
        ExecutablePath::resolve(
            Some("wt.exe"),
            Some(r"C:\Windows;C:\tools"),
            Some(PATH_EXT),
            existing(&[]),
        )
    );
}

#[test]
fn nothing_resolves_to_nothing() {
    for program in [None, Some(""), Some("   "), Some("\"\"")] {
        assert_eq!(
            None,
            ExecutablePath::resolve(
                program,
                Some(r"C:\Windows"),
                Some(PATH_EXT),
                existing(&[r"C:\Windows\tool.exe"]),
            ),
            "{program:?}"
        );
    }
}

#[test]
fn quoted_path_entries_are_understood() {
    assert_path(
        r"C:\Program Files\tools\tool.exe",
        ExecutablePath::resolve(
            Some("tool.exe"),
            Some("\"C:\\Program Files\\tools\""),
            Some(PATH_EXT),
            existing(&[r"C:\Program Files\tools\tool.exe"]),
        ),
    );
}

#[test]
fn a_quoted_program_is_understood() {
    assert_path(
        r"C:\tools\tool.exe",
        ExecutablePath::resolve(
            Some("\"C:\\tools\\tool.exe\""),
            Some(r"C:\Windows"),
            Some(PATH_EXT),
            existing(&[r"C:\tools\tool.exe"]),
        ),
    );
}

#[test]
fn empty_and_padded_path_entries_are_skipped_rather_than_ending_the_search() {
    assert_path(
        r"C:\tools\tool.exe",
        ExecutablePath::resolve(
            Some("tool.exe"),
            Some(r";; C:\nope ;;C:\tools;"),
            Some(PATH_EXT),
            existing(&[r"C:\tools\tool.exe"]),
        ),
    );
}

#[test]
fn an_unusable_path_entry_does_not_stop_the_search() {
    assert_path(
        r"C:\tools\tool.exe",
        ExecutablePath::resolve(
            Some("tool.exe"),
            Some("C:\\bad|entry;C:\\tools"),
            Some(PATH_EXT),
            existing(&[r"C:\tools\tool.exe"]),
        ),
    );
}

#[test]
fn an_empty_path_finds_nothing_rather_than_throwing() {
    assert_eq!(
        None,
        ExecutablePath::resolve(
            Some("tool.exe"),
            Some(""),
            Some(PATH_EXT),
            existing(&[r"C:\tools\tool.exe"]),
        )
    );
}

#[test]
fn a_blank_path_ext_falls_back_to_the_windows_default() {
    for path_ext in [None, Some(""), Some("   ")] {
        assert_path(
            r"C:\tools\code.cmd",
            ExecutablePath::resolve(
                Some("code"),
                Some(r"C:\tools"),
                path_ext,
                existing(&[r"C:\tools\code.cmd"]),
            ),
        );
    }
}

#[test]
fn path_ext_entries_may_omit_their_leading_dot() {
    assert_path(
        r"C:\tools\code.cmd",
        ExecutablePath::resolve(
            Some("code"),
            Some(r"C:\tools"),
            Some("COM;EXE;BAT;CMD"),
            existing(&[r"C:\tools\code.cmd"]),
        ),
    );
}

#[test]
fn a_probe_that_throws_costs_that_candidate_and_not_the_search() {
    let resolved = ExecutablePath::resolve(
        Some("tool.exe"),
        Some(r"C:\bad;C:\tools"),
        Some(PATH_EXT),
        |path| {
            if path.to_lowercase().starts_with(r"c:\bad") {
                Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "device not ready",
                ))
            } else {
                Ok(path.eq_ignore_ascii_case(r"C:\tools\tool.exe"))
            }
        },
    );

    assert_eq!(Some(r"C:\tools\tool.exe".to_string()), resolved);
}

#[test]
fn meta_the_fake_filesystem_really_can_fail() {
    // If existing() answered true for everything the search-order tests above would pass no
    // matter what resolve did.
    assert!(!existing(&[r"C:\tools\tool.exe"])(r"C:\tools\other.exe").unwrap());
}
