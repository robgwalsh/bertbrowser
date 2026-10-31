// Port of tests/BertBrowser.Core.Tests/ShellOpenCommandTests.cs.
//
// Reading the command Windows would run to open a file, so it can be started with an administrator
// token instead. Everything here ends in starting a program elevated, so the bias is towards
// refusing: a command this cannot read confidently must come back None and grey the menu item out.
use bb_core::shell_integration::ShellOpenCommandParser;

const FILE: &str = r"C:\Source\app\app.sln";

/// Every path any of these tests names as real.
fn real(paths: &[&str]) -> impl Fn(&str) -> bool {
    let paths: Vec<String> = paths.iter().map(|p| p.to_lowercase()).collect();
    move |path: &str| paths.contains(&path.to_lowercase())
}

#[test]
fn a_quoted_program_with_a_placeholder_is_the_ordinary_case() {
    // What .sln actually registers.
    let exe = r"C:\Program Files (x86)\Common Files\Microsoft Shared\MSEnv\VSLauncher.exe";

    let parsed =
        ShellOpenCommandParser::parse(Some(&format!("\"{exe}\" \"%1\"")), FILE, real(&[exe]));

    let parsed = parsed.expect("parsed");
    assert_eq!(exe, parsed.executable);
    assert_eq!(format!("\"{FILE}\""), parsed.arguments);
}

#[test]
fn every_placeholder_the_shell_substitutes_is_substituted() {
    let exe = r"C:\tools\app.exe";
    for placeholder in ["%1", "%L", "%l", "%V", "%D", "%*"] {
        let parsed = ShellOpenCommandParser::parse(
            Some(&format!("\"{exe}\" \"{placeholder}\"")),
            FILE,
            real(&[exe]),
        );

        assert_eq!(
            format!("\"{FILE}\""),
            parsed.expect("parsed").arguments,
            "{placeholder}"
        );
    }
}

#[test]
fn an_unquoted_program_with_no_spaces_is_read() {
    let exe = r"C:\Windows\notepad.exe";

    let parsed = ShellOpenCommandParser::parse(Some(&format!("{exe} %1")), FILE, real(&[exe]));

    assert_eq!(exe, parsed.expect("parsed").executable);
}

#[test]
fn an_unquoted_program_with_spaces_is_resolved_the_way_windows_resolves_it() {
    // "C:\Program" is not the program, however much the first space suggests it. Windows probes
    // each candidate; so does this, longest first.
    let exe = r"C:\Program Files\Vendor App\run.exe";

    let parsed = ShellOpenCommandParser::parse(Some(&format!("{exe} %1")), FILE, real(&[exe]))
        .expect("parsed");

    assert_eq!(exe, parsed.executable);
    assert_eq!(FILE, parsed.arguments);
}

// --- the refusals ---

#[test]
fn a_command_naming_a_program_that_is_not_there_is_refused() {
    assert_eq!(
        None,
        ShellOpenCommandParser::parse(Some(r#""C:\gone\app.exe" "%1""#), FILE, real(&[]))
    );
}

#[test]
fn a_command_that_never_says_where_the_file_goes_is_refused() {
    // Appending the path anyway would hand a program an argument it never asked for, elevated.
    let exe = r"C:\tools\app.exe";

    assert_eq!(
        None,
        ShellOpenCommandParser::parse(Some(&format!("\"{exe}\"")), FILE, real(&[exe]))
    );
}

#[test]
fn an_unquoted_program_no_prefix_of_which_exists_is_refused() {
    assert_eq!(
        None,
        ShellOpenCommandParser::parse(
            Some(r"C:\Program Files\Vendor App\run.exe %1"),
            FILE,
            real(&[r"C:\something\else.exe"])
        )
    );
}

#[test]
fn an_unterminated_quote_is_refused() {
    assert_eq!(
        None,
        ShellOpenCommandParser::parse(Some(r#""C:\tools\app.exe %1"#), FILE, real(&[]))
    );
}

#[test]
fn nothing_is_refused() {
    for command in [None, Some(""), Some("   ")] {
        assert_eq!(
            None,
            ShellOpenCommandParser::parse(command, FILE, real(&[])),
            "{command:?}"
        );
    }
}

#[test]
fn an_environment_variable_is_not_mistaken_for_a_placeholder() {
    // %SystemRoot% is expanded by the caller before it gets here; what must not happen is the
    // parser treating the % pairs around it as somewhere to put the file.
    let exe = r"C:\Windows\system32\app.exe";

    let parsed = ShellOpenCommandParser::parse(
        Some(&format!("\"{exe}\" -config %SystemRoot%\\x.ini \"%1\"")),
        FILE,
        real(&[exe]),
    )
    .expect("parsed");

    assert_eq!(
        format!("-config C:\\Windows\\x.ini \"{FILE}\""),
        parsed.arguments.replace("%SystemRoot%", "C:\\Windows")
    );
}
