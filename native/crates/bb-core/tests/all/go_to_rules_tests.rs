// Port of tests/BertBrowser.Core.Tests/GoToRulesTests.cs.
use bb_core::commands::{GoToRules, GoToTarget};

const HOME: &str = r"C:\Users\Sam";

fn variable(name: &str) -> Option<String> {
    match name.to_uppercase().as_str() {
        "TEMP" => Some(r"C:\Users\Sam\AppData\Local\Temp".to_string()),
        "SYSTEMDRIVE" => Some("C:".to_string()),
        _ => None,
    }
}

fn parse(text: &str) -> Option<GoToTarget> {
    GoToRules::parse(text, variable, HOME)
}

#[test]
fn a_rooted_path_is_a_destination() {
    for (text, path, folder, prefix) in [
        (r"C:\Windows", r"C:\Windows", r"C:\", "Windows"),
        (r"C:\Windows\", r"C:\Windows\", r"C:\Windows\", ""),
        (r"c:\win", r"c:\win", r"c:\", "win"),
        (
            r"C:\Users\Sam\Do",
            r"C:\Users\Sam\Do",
            r"C:\Users\Sam\",
            "Do",
        ),
        (r"D:/Media/Films", r"D:\Media\Films", r"D:\Media\", "Films"),
        (r"\\nas\share\x", r"\\nas\share\x", r"\\nas\share\", "x"),
        (
            "\"C:\\Program Files\"",
            r"C:\Program Files",
            r"C:\",
            "Program Files",
        ),
    ] {
        assert_eq!(
            Some(GoToTarget::new(path, folder, prefix)),
            parse(text),
            "{text}"
        );
    }
}

#[test]
fn a_bare_drive_is_its_root() {
    // Not whatever folder the process happens to be sitting in on that drive.
    assert_eq!(Some(GoToTarget::new(r"c:\", r"c:\", "")), parse("c:"));
}

#[test]
fn home_and_environment_variables_are_filled_in() {
    for (text, path) in [
        ("~", HOME.to_string()),
        (r"~\Downloads", format!(r"{HOME}\Downloads")),
        ("~/Downloads", format!(r"{HOME}\Downloads")),
        ("%TEMP%", r"C:\Users\Sam\AppData\Local\Temp".to_string()),
        (
            r"%temp%\logs",
            r"C:\Users\Sam\AppData\Local\Temp\logs".to_string(),
        ),
        (r"%SystemDrive%\Windows", r"C:\Windows".to_string()),
    ] {
        assert_eq!(
            path,
            parse(text).unwrap_or_else(|| panic!("{text}")).path,
            "{text}"
        );
    }
}

#[test]
fn anything_else_is_a_search_for_a_command() {
    for text in [
        "",
        "new tab",
        "rename",
        "view:",
        "tabs: close",
        "c",
        "cx:",
        "1:",
        "%TEMP",
        "%NOSUCH%",
        "%%",
        r"\single",
        "~user",
        "100% done",
    ] {
        assert!(parse(text).is_none(), "{text:?}");
    }
}

#[test]
fn a_variable_that_names_something_other_than_a_path_is_not_a_destination() {
    assert!(GoToRules::parse("%GREETING%", |_| Some("hello".to_string()), HOME).is_none());
}
