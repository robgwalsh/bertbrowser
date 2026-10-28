// Port of tests/BertBrowser.Core.Tests/ChecksumPathTests.cs
//
// The security file. A checksum file is written by whoever made the download, and every name in
// it is about to become a path this app opens — so each of these is a line somebody could put in
// one, and the answer to all of them is "no", returned rather than panicked.
use bb_core::checksums::ChecksumPath;

const FOLDER: &str = r"C:\Downloads\release";

fn resolve(name: &str) -> Option<String> {
    ChecksumPath::resolve(FOLDER, name)
}

// --- what must be refused ---

#[test]
fn an_escaping_path_is_refused() {
    for name in [
        r"..\..\Windows\System32\config\SAM",
        "../../etc/passwd",
        r"sub\..\..\outside.txt",
        "..",
        r"..\sibling.txt",
    ] {
        assert_eq!(None, resolve(name), "{name}");
    }
}

#[test]
fn a_rooted_or_device_path_is_refused() {
    for name in [
        r"C:\Windows\System32\drivers\etc\hosts",
        r"\\server\share\payload.dll",
        r"\Windows\notepad.exe",
        r"C:relative.txt",
        r"\\?\C:\Windows\notepad.exe",
        r"\\.\PhysicalDrive0",
    ] {
        assert_eq!(None, resolve(name), "{name}");
    }
}

#[test]
fn an_invalid_name_is_refused() {
    for name in [
        "bad<name.txt",
        "pipe|name.txt",
        "star*.txt",
        "quote\"name.txt",
    ] {
        assert_eq!(None, resolve(name), "{name}");
    }
}

#[test]
fn an_empty_name_is_refused() {
    assert_eq!(None, resolve(""));
    assert_eq!(None, resolve("   "));
}

#[test]
fn a_current_directory_segment_is_refused() {
    assert_eq!(None, resolve(r".\file.txt"));
}

// --- what must be allowed ---

#[test]
fn a_plain_name_resolves_under_the_folder() {
    assert_eq!(
        Some(r"C:\Downloads\release\setup.exe".to_string()),
        resolve("setup.exe")
    );
}

/// sha256sum files are written on Linux at least as often as on Windows, so a listed subpath
/// arrives with forward slashes and has to work.
#[test]
fn a_forward_slash_sub_path_resolves() {
    assert_eq!(
        Some(r"C:\Downloads\release\bin\tool.exe".to_string()),
        resolve("bin/tool.exe")
    );
}

#[test]
fn a_backslash_sub_path_resolves() {
    assert_eq!(
        Some(r"C:\Downloads\release\bin\tool.exe".to_string()),
        resolve(r"bin\tool.exe")
    );
}

#[test]
fn a_name_with_spaces_resolves() {
    assert_eq!(
        Some(r"C:\Downloads\release\my file.zip".to_string()),
        resolve("my file.zip")
    );
}

// --- writing them back out ---

#[test]
fn relativise_uses_forward_slashes() {
    assert_eq!(
        Some("bin/tool.exe".to_string()),
        ChecksumPath::relativise(FOLDER, r"C:\Downloads\release\bin\tool.exe")
    );
}

#[test]
fn relativise_refuses_something_outside_the_folder() {
    assert_eq!(
        None,
        ChecksumPath::relativise(FOLDER, r"C:\Windows\notepad.exe")
    );
}

/// Every refusal above is a None. Nothing here may panic — one bad line is one row of a report,
/// not the end of the run.
#[test]
fn nothing_throws() {
    let long = "x".repeat(5000);
    for name in ["", "..", "\0", long.as_str(), r"C:\", "|", "con:"] {
        let _ = ChecksumPath::resolve(FOLDER, name);
    }
}
