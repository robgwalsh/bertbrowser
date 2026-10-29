// Port of tests/BertBrowser.Core.Tests/RunAsVerbRulesTests.cs.
//
// Whether "Run as administrator" can mean anything for a given file. The bug this exists to
// prevent: the menu item was enabled for every file, so choosing it on a text file produced
// ERROR_NO_ASSOCIATION and a status-bar line about a file that opens fine on a double-click.
use std::cell::Cell;

use bb_core::shell_integration::{ElevatedOpen, ElevatedOpenKind, RunAsVerbRules};

fn registry_says_no(_: &str) -> Option<bool> {
    Some(false)
}
fn registry_says_yes(_: &str) -> Option<bool> {
    Some(true)
}
fn registry_cannot_tell(_: &str) -> Option<bool> {
    None
}

// --- the extension to ask about ---

#[test]
fn the_extension_is_what_the_shell_would_ask_about() {
    for (path, expected) in [
        (r"C:\tools\setup.exe", ".exe"),
        (r"C:\tools\SETUP.EXE", ".exe"),
        (r"C:\notes\report.md", ".md"),
        (r"C:\a\archive.tar.gz", ".gz"),
    ] {
        assert_eq!(expected, RunAsVerbRules::extension_of(path), "{path}");
    }
}

#[test]
fn something_with_no_type_has_nothing_to_ask_about() {
    for path in [
        r"C:\src\.gitignore", // a dotfile is a name, not a type
        r"C:\bin\Makefile",   // no extension at all
        r"C:\a\trailing.",    // nothing after the dot
        "",
    ] {
        assert_eq!("", RunAsVerbRules::extension_of(path), "{path}");
    }
}

// --- the decision ---

#[test]
fn a_program_can_be_run_as_administrator() {
    assert!(RunAsVerbRules::can_run_elevated(
        r"C:\tools\setup.exe",
        false,
        false,
        registry_says_yes
    ));
}

#[test]
fn a_document_cannot() {
    // The reported bug, in one line.
    assert!(!RunAsVerbRules::can_run_elevated(
        r"C:\notes\notes.txt",
        false,
        false,
        registry_says_no
    ));
}

#[test]
fn a_folder_cannot() {
    assert!(!RunAsVerbRules::can_run_elevated(
        r"C:\tools",
        true,
        false,
        registry_says_yes
    ));
}

#[test]
fn something_inside_an_archive_cannot() {
    // It has no path another process could be pointed at; it has to be extracted first.
    assert!(!RunAsVerbRules::can_run_elevated(
        r"C:\a\bundle.zip\setup.exe",
        false,
        true,
        registry_says_yes
    ));
}

#[test]
fn something_with_no_extension_cannot() {
    assert!(!RunAsVerbRules::can_run_elevated(
        r"C:\bin\Makefile",
        false,
        false,
        registry_says_yes
    ));
}

#[test]
fn a_file_type_something_else_registered_a_verb_for_can() {
    // The reason the registry is asked rather than an extension list consulted: greying the item
    // out on something that would have worked is the worse failure of the two.
    assert!(RunAsVerbRules::can_run_elevated(
        r"C:\vendor\thing.customapp",
        false,
        false,
        registry_says_yes
    ));
}

#[test]
fn a_shortcut_can_even_though_the_registry_says_otherwise() {
    // The one place the registry is not the authority. A .lnk carries no verbs of its own — the
    // shell resolves it and applies the target's — so lnkfile has no runas key, and trusting the
    // probe would grey the item out on a shortcut to a program Windows will happily elevate.
    // Measured: runas on a shortcut to notepad.exe starts an elevated Notepad.
    assert!(RunAsVerbRules::can_run_elevated(
        r"C:\Desktop\Notepad.lnk",
        false,
        false,
        registry_says_no
    ));
}

#[test]
fn a_shortcut_is_not_even_asked_about() {
    let asked = Cell::new(false);
    RunAsVerbRules::can_run_elevated(r"C:\Desktop\Notepad.lnk", false, false, |_| {
        asked.set(true);
        Some(false)
    });

    assert!(
        !asked.get(),
        "a shortcut is settled by the shell, not by the registry."
    );
}

// --- when the registry cannot be read ---

#[test]
fn an_unreadable_registry_still_lets_a_program_run() {
    // Not a refusal: being unable to read the registry must not take the feature away from an
    // .exe, and everything on the fallback list is a program by any reading.
    for extension in [".exe", ".bat", ".cmd", ".msc"] {
        assert!(
            RunAsVerbRules::can_run_elevated(
                &format!(r"C:\tools\thing{extension}"),
                false,
                false,
                registry_cannot_tell
            ),
            "{extension}"
        );
    }
}

#[test]
fn an_unreadable_registry_does_not_guess_about_a_document() {
    assert!(!RunAsVerbRules::can_run_elevated(
        r"C:\notes\notes.txt",
        false,
        false,
        registry_cannot_tell
    ));
}

#[test]
fn the_extension_is_asked_about_in_the_form_the_registry_uses() {
    // Lower-cased and carrying its dot, or the lookup misses on a file named in capitals.
    let asked: Cell<Option<String>> = Cell::new(None);
    RunAsVerbRules::can_run_elevated(r"C:\tools\SETUP.EXE", false, false, |extension| {
        asked.set(Some(extension.to_string()));
        Some(true)
    });

    assert_eq!(Some(".exe".to_string()), asked.into_inner());
}

// --- the second way: elevating the file's handler ---

#[test]
fn a_type_with_no_verb_but_a_handler_elevates_the_handler() {
    // The .sln case. No progid under it carries a runas verb, but it does have an open command,
    // and starting VSLauncher elevated with the solution is what the user meant.
    let exe = r"C:\VS\VSLauncher.exe";

    let open = RunAsVerbRules::decide(
        r"C:\Source\app.sln",
        false,
        false,
        |_| Some(false),
        |_| Some(format!("\"{exe}\" \"%1\"")),
        |p| p == exe,
    );

    assert_eq!(ElevatedOpenKind::Handler, open.kind());
    match open {
        ElevatedOpen::Handler {
            executable,
            arguments,
        } => {
            assert_eq!(exe, executable);
            assert_eq!(r#""C:\Source\app.sln""#, arguments);
        }
        other => panic!("expected a handler, got {other:?}"),
    }
}

#[test]
fn a_registered_verb_is_preferred_over_the_handler() {
    // An .exe has a real runas verb; handing it to the shell is what Windows means by running it
    // as administrator, and second-guessing that with its own open command would be wrong.
    let open = RunAsVerbRules::decide(
        r"C:\tools\setup.exe",
        false,
        false,
        |_| Some(true),
        |_| Some(r#""C:\other\thing.exe" "%1""#.to_string()),
        |_| true,
    );

    assert_eq!(ElevatedOpenKind::Verb, open.kind());
}

#[test]
fn a_type_with_neither_is_still_refused() {
    let open = RunAsVerbRules::decide(
        r"C:\notes\notes.txt",
        false,
        false,
        |_| Some(false),
        |_| None,
        |_| true,
    );

    assert_eq!(ElevatedOpenKind::None, open.kind());
}

#[test]
fn a_handler_that_cannot_be_resolved_is_not_guessed_at() {
    // Greying the item out is the right failure. Starting an approximation with a token is not.
    assert_eq!(
        ElevatedOpenKind::None,
        RunAsVerbRules::decide(
            r"C:\Source\app.sln",
            false,
            false,
            |_| Some(false),
            |_| Some(r#""C:\gone\missing.exe" "%1""#.to_string()),
            |_| false,
        )
        .kind()
    );
}

#[test]
fn a_folder_has_no_handler_to_elevate_either() {
    assert_eq!(
        ElevatedOpenKind::None,
        RunAsVerbRules::decide(
            r"C:\Source",
            true,
            false,
            |_| Some(true),
            |_| Some(r#""C:\VS\VSLauncher.exe" "%1""#.to_string()),
            |_| true,
        )
        .kind()
    );
}

// --- what it says when it was asked for anyway ---

#[test]
fn the_message_names_the_file_and_says_why() {
    let message = RunAsVerbRules::cannot_run_message("notes.txt");

    assert!(message.contains("notes.txt"));
    assert!(!message.to_lowercase().contains("associated"));
}
