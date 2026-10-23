// Port of tests/BertBrowser.Core.Tests/StaticVerbCommandTests.cs: what a static registry verb
// runs, for the folder it was clicked in.
use bb_core::shellmenu::{ShellMenuContext, ShellMenuTarget, StaticVerbCommand};

const GIT_BASH: &str = r"C:\Program Files\Git\git-bash.exe";
const FOLDER: &str = r"C:\Source\app";

fn real(paths: &'static [&'static str]) -> impl Fn(&str) -> bool {
    move |path: &str| paths.iter().any(|p| p.eq_ignore_ascii_case(path))
}

/// Git for Windows' background verb, verbatim — the trailing dot after `%v` is theirs and has to
/// survive.
#[test]
fn git_bash_here_on_the_background() {
    let command = StaticVerbCommand::resolve(
        Some(&format!("\"{GIT_BASH}\" \"--cd=%v.\"")),
        FOLDER,
        FOLDER,
        real(&[GIT_BASH]),
    )
    .unwrap();

    assert_eq!(GIT_BASH, command.executable);
    assert_eq!(format!("\"--cd={FOLDER}.\""), command.arguments);
}

#[test]
fn the_working_directory_placeholder_is_the_folder_not_the_item() {
    let command = StaticVerbCommand::resolve(
        Some(&format!("\"{GIT_BASH}\" \"--cd=%W\" \"%1\"")),
        r"C:\Source\app\readme.md",
        FOLDER,
        real(&[GIT_BASH]),
    )
    .unwrap();

    assert_eq!(
        format!("\"--cd={FOLDER}\" \"C:\\Source\\app\\readme.md\""),
        command.arguments
    );
}

#[test]
fn a_verb_with_no_placeholder_still_runs() {
    let command = StaticVerbCommand::resolve(
        Some(&format!("\"{GIT_BASH}\" --login")),
        FOLDER,
        FOLDER,
        real(&[GIT_BASH]),
    )
    .unwrap();

    assert_eq!("--login", command.arguments);
}

#[test]
fn a_program_that_is_not_there_is_nothing_to_run() {
    assert_eq!(
        None,
        StaticVerbCommand::resolve(
            Some(&format!("\"{GIT_BASH}\" \"%1\"")),
            FOLDER,
            FOLDER,
            real(&[])
        )
    );
}

#[test]
fn a_blank_command_is_nothing_to_run() {
    assert_eq!(
        None,
        StaticVerbCommand::resolve(Some("  "), FOLDER, FOLDER, real(&[GIT_BASH]))
    );
}

#[test]
fn a_background_verb_runs_in_the_folder() {
    assert_eq!(
        FOLDER,
        StaticVerbCommand::working_directory_for(
            &ShellMenuTarget::new(FOLDER, true),
            ShellMenuContext::Background,
            FOLDER
        )
    );
}

#[test]
fn a_file_verb_runs_in_the_files_own_folder() {
    assert_eq!(
        r"C:\Source\app\src",
        StaticVerbCommand::working_directory_for(
            &ShellMenuTarget::new(r"C:\Source\app\src\a.cs", false),
            ShellMenuContext::Items,
            FOLDER
        )
    );
}

#[test]
fn a_folder_verb_runs_in_that_folder() {
    assert_eq!(
        r"C:\Source\app\src",
        StaticVerbCommand::working_directory_for(
            &ShellMenuTarget::new(r"C:\Source\app\src", true),
            ShellMenuContext::Items,
            FOLDER
        )
    );
}
