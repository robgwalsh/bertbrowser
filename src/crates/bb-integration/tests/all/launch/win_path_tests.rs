// The `System.IO.Path` answers the launch helpers lean on, checked against .NET's Windows rules.
use bb_integration::launch::win_path::{
    directory_name, extension, is_fully_qualified, is_rooted, replace_ignore_ascii_case,
};

#[test]
fn directory_name_follows_dotnet() {
    assert_eq!(
        Some(r"C:\data\docs".into()),
        directory_name(r"C:\data\docs\report.txt")
    );
    assert_eq!(Some(r"C:\".into()), directory_name(r"C:\report.txt"));
    assert_eq!(None, directory_name(r"C:\"));
    assert_eq!(Some(r"C:\a\b".into()), directory_name(r"C:\a\b\"));
    assert_eq!(Some("".into()), directory_name("report.txt"));
    assert_eq!(None, directory_name(r"\\server\share"));
    assert_eq!(
        Some(r"\\server\share".into()),
        directory_name(r"\\server\share\x")
    );
    assert_eq!(
        Some(r"\\server\share\x".into()),
        directory_name(r"\\server\share\x\y")
    );
}

#[test]
fn extension_follows_dotnet() {
    assert_eq!(".exe", extension(r"C:\tools\wt.exe"));
    assert_eq!("", extension(r"C:\tools\code"));
    assert_eq!("", extension(r"C:\a.b\code"));
    assert_eq!("", extension("file."));
    assert_eq!(".gitignore", extension(".gitignore"));
}

#[test]
fn qualification_follows_dotnet() {
    assert!(is_fully_qualified(r"C:\x"));
    assert!(is_fully_qualified(r"\\server\share"));
    assert!(!is_fully_qualified(r"\tool.exe"));
    assert!(!is_fully_qualified("/tool.exe"));
    assert!(!is_fully_qualified(r"C:tool.exe"));
    assert!(is_rooted(r"\tool.exe"));
    assert!(is_rooted(r"C:tool.exe"));
    assert!(!is_rooted(r"sub\tool.exe"));
}

#[test]
fn replace_is_case_insensitive_and_keeps_the_rest() {
    assert_eq!(
        "a X b X",
        replace_ignore_ascii_case("a {Path} b {PATH}", "{path}", "X")
    );
    assert_eq!("héllo", replace_ignore_ascii_case("héllo", "zzz", "X"));
}
