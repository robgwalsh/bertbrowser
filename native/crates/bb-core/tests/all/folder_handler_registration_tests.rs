// Port of tests/BertBrowser.Core.Tests/FolderHandlerRegistrationTests.cs.
//
// The rule that decides whether the Windows shell opens folders in BertBrowser. Nothing here
// touches the registry — the app half reads and writes raw values, and every decision about what
// they mean lives here so it can be tested without opening a key.
use bb_core::cli::{CommandLine, OpenIn};
use bb_core::shell_integration::{
    FolderHandlerReading, FolderHandlerRegistration, FolderHandlerRules, FolderHandlerState,
};

const EXE: &str = r"C:\Users\Rob\AppData\Local\BertBrowser\current\BertBrowser.exe";

/// Another file manager holding the verb — a real one, since the point of the rule is that
/// BertBrowser must not quietly take a registration it did not make.
const FOREIGN_PROGRAM: &str = r"C:\Program Files\Files\Files.exe";
const FOREIGN_COMMAND: &str = r#""C:\Program Files\Files\Files.exe" "%1""#;

/// A complete registration: both commands, and both shell keys naming the verb as their default
/// action — without which the shell never invokes it.
fn registered(exe: &str) -> FolderHandlerReading {
    FolderHandlerReading::new(
        Some("open"),
        Some(&command(exe)),
        Some("open"),
        Some(&command(exe)),
    )
}

fn command(exe: &str) -> String {
    FolderHandlerRegistration::command_for(exe)
}

fn reading(
    directory_verb: Option<&str>,
    directory_command: Option<&str>,
    drive_verb: Option<&str>,
    drive_command: Option<&str>,
) -> FolderHandlerReading {
    FolderHandlerReading::new(directory_verb, directory_command, drive_verb, drive_command)
}

// --- what gets written ---

#[test]
fn the_command_quotes_both_the_program_and_the_argument() {
    assert_eq!(format!("\"{EXE}\" --new-tab \"%1\""), command(EXE));
}

/// Opening from the shell is a new tab, never a retarget of whichever tab happened to be active.
/// The flag lives on the registration so that `bertbrowser <path>` typed at a prompt keeps its own
/// documented behaviour.
#[test]
fn the_shell_is_told_to_open_a_new_tab() {
    let request = CommandLine::parse(&[FolderHandlerRegistration::NEW_TAB_FLAG, r"C:\Some\Folder"]);

    assert_eq!(OpenIn::NewTab, request.mode);
    assert_eq!(1, request.targets.len());
    assert_eq!(r"C:\Some\Folder", request.targets[0].path);
    assert!(request.errors.is_empty());
}

/// The flag must not be mistaken for a path, nor the path for a flag, once the shell has
/// substituted a real folder into `%1`.
#[test]
fn the_registered_command_parses_back_into_what_it_meant() {
    let command = command(EXE);
    let after = command.find("\" ").expect("program quote") + 2;
    let argv = command[after..].replace("\"%1\"", r"C:\Users\Rob\Documents");

    let request = CommandLine::parse(&argv.split(' ').collect::<Vec<_>>());

    assert_eq!(OpenIn::NewTab, request.mode);
    assert_eq!(1, request.targets.len());
    assert_eq!(r"C:\Users\Rob\Documents", request.targets[0].path);
}

/// The regression test for the worst bug this feature has had. Naming the default verb is what
/// makes the shell invoke it at all — but naming one that is not yet there sends a folder
/// double-click to whatever third-party verb enumerates first, which on a real machine meant
/// NordVPN. So the value lives apart from the verb it names and is written only after the command
/// has been read back.
#[test]
fn the_default_verb_is_not_written_alongside_the_verb_it_names() {
    let values = FolderHandlerRegistration::values_for(EXE).unwrap();
    assert!(!values.iter().any(
        |v| (v.key_path == FolderHandlerRegistration::DIRECTORY_SHELL_KEY
            || v.key_path == FolderHandlerRegistration::DRIVE_SHELL_KEY)
            && v.value_name.is_empty()
    ));

    let guards = FolderHandlerRegistration::guard_values();
    assert_eq!(2, guards.len());
    for v in &guards {
        assert_eq!("", v.value_name);
        assert_eq!(FolderHandlerRegistration::OPEN_VERB, v.data);
    }
}

/// The keys whose command must read back before the default verb may name them — the two that
/// the guard values point at.
#[test]
fn the_verified_command_keys_are_the_ones_the_default_verb_names() {
    assert_eq!(
        [
            FolderHandlerRegistration::DIRECTORY_COMMAND_KEY,
            FolderHandlerRegistration::DRIVE_COMMAND_KEY
        ],
        FolderHandlerRegistration::command_keys()
    );
}

/// The command is written before the key that carries the verb's decoration. Creating the `open`
/// key registers a verb; the command is what makes it runnable, so an interrupted write must not
/// be able to leave the first without the second.
#[test]
fn the_command_is_written_before_the_verb_key() {
    let values = FolderHandlerRegistration::values_for(EXE).unwrap();

    let index_of = |key: &str| values.iter().position(|v| v.key_path == key).unwrap();
    assert!(
        index_of(FolderHandlerRegistration::DIRECTORY_COMMAND_KEY)
            < index_of(FolderHandlerRegistration::DIRECTORY_OPEN_KEY)
    );
}

/// Without this the whole feature is inert. `Directory` and `Drive` inherit their `open` verb from
/// `Folder`, which carries CLSID_ExecuteFolder in DelegateExecute — and a DelegateExecute that
/// resolves makes the shell ignore the command line entirely. The empty shadow cuts that
/// inheritance.
#[test]
fn each_command_blanks_the_inherited_delegate_execute() {
    for directory in [true, false] {
        let key = if directory {
            FolderHandlerRegistration::DIRECTORY_COMMAND_KEY
        } else {
            FolderHandlerRegistration::DRIVE_COMMAND_KEY
        };

        assert!(
            FolderHandlerRegistration::values_for(EXE)
                .unwrap()
                .iter()
                .any(|v| v.key_path == key
                    && v.value_name == "DelegateExecute"
                    && v.data.is_empty()),
            "directory={directory}"
        );
    }
}

#[test]
fn both_classes_get_a_command() {
    let values = FolderHandlerRegistration::values_for(EXE).unwrap();

    assert!(values.iter().any(
        |v| v.key_path == FolderHandlerRegistration::DIRECTORY_COMMAND_KEY
            && v.data == command(EXE)
    ));
    assert!(values.iter().any(
        |v| v.key_path == FolderHandlerRegistration::DRIVE_COMMAND_KEY && v.data == command(EXE)
    ));
}

/// The `Folder` class is what This PC, Control Panel, zip browsing and Explorer's own navigation
/// come through. Taking it is out of scope, and writing it by accident is the one mistake here
/// that would break parts of Windows BertBrowser cannot stand in for.
#[test]
fn nothing_is_written_under_the_folder_class() {
    assert!(
        !FolderHandlerRegistration::values_for(EXE)
            .unwrap()
            .iter()
            .any(|v| v.key_path.to_lowercase().contains(r"classes\folder"))
    );
}

/// `Directory\shell` holds other installers' verbs, so unregistering deletes our verb subtree and
/// our one value, never the key they share.
#[test]
fn removal_takes_the_verb_subtree_but_not_the_shared_shell_key() {
    assert_eq!(
        [
            FolderHandlerRegistration::DIRECTORY_OPEN_KEY,
            FolderHandlerRegistration::DRIVE_OPEN_KEY
        ],
        FolderHandlerRegistration::keys_to_remove()
    );

    assert!(
        !FolderHandlerRegistration::keys_to_remove()
            .contains(&FolderHandlerRegistration::DIRECTORY_SHELL_KEY)
    );
    assert!(
        FolderHandlerRegistration::values_to_remove()
            .iter()
            .any(
                |v| v.key_path == FolderHandlerRegistration::DIRECTORY_SHELL_KEY
                    && v.value_name.is_empty()
            )
    );
}

/// The default verb is removed only when it still holds what this app wrote. Those keys are
/// shared, and blanking a verb another program has since pointed elsewhere would break their
/// registration in the name of tidying up after ours.
#[test]
fn only_the_default_verb_this_app_wrote_is_removable() {
    for v in FolderHandlerRegistration::values_to_remove() {
        assert_eq!(FolderHandlerRegistration::OPEN_VERB, v.expected_data);
    }
}

// --- what a reading means ---

#[test]
fn nothing_registered_reads_as_not_registered() {
    assert_eq!(
        FolderHandlerState::NotRegistered,
        FolderHandlerRules::classify(&FolderHandlerReading::none(), EXE)
    );
}

#[test]
fn a_complete_registration_naming_this_exe_is_current() {
    assert_eq!(
        FolderHandlerState::RegisteredToThisApp,
        FolderHandlerRules::classify(&registered(EXE), EXE)
    );
}

/// Paths are compared through `PathKey`, not by string equality — otherwise a registration
/// differing only in casing reads as stale and the startup self-heal rewrites it on every launch.
#[test]
fn casing_alone_does_not_make_a_registration_stale() {
    assert_eq!(
        FolderHandlerState::RegisteredToThisApp,
        FolderHandlerRules::classify(&registered(&EXE.to_uppercase()), EXE)
    );
}

/// An older build's command line — right program, outdated arguments — still launches the app and
/// no longer means what this version means by it, so it is stale and the startup repair rewrites
/// it.
#[test]
fn an_outdated_argument_list_is_stale() {
    let old = format!("\"{EXE}\" \"%1\"");
    assert_eq!(
        FolderHandlerState::RegisteredToThisAppStale,
        FolderHandlerRules::classify(
            &reading(Some("open"), Some(&old), Some("open"), Some(&old)),
            EXE
        )
    );
}

#[test]
fn the_arguments_are_read_off_the_command() {
    for (command, expected) in [
        (
            Some(r#""C:\a b\app.exe" --new-tab "%1""#),
            r#"--new-tab "%1""#,
        ),
        (
            Some(r#"C:\dir\app.exe --new-tab "%1""#),
            r#"--new-tab "%1""#,
        ),
        (Some(r#""C:\a b\app.exe""#), ""),
        (Some(r"C:\dir\app.exe"), ""),
        (None, ""),
    ] {
        assert_eq!(
            expected,
            FolderHandlerRules::arguments_in(command),
            "{command:?}"
        );
    }
}

#[test]
fn an_old_install_path_is_stale() {
    assert_eq!(
        FolderHandlerState::RegisteredToThisAppStale,
        FolderHandlerRules::classify(&registered(r"D:\Old\BertBrowser.exe"), EXE)
    );
}

#[test]
fn only_half_the_registration_is_stale() {
    assert_eq!(
        FolderHandlerState::RegisteredToThisAppStale,
        FolderHandlerRules::classify(&reading(Some("open"), Some(&command(EXE)), None, None), EXE)
    );
}

/// A command the default verb does not name is never invoked: with the stock "none" still in
/// place the shell uses its own folder navigation and the registration sits there unread.
#[test]
fn a_command_the_default_verb_does_not_name_is_stale() {
    for verb in [None, Some("none")] {
        assert_eq!(
            FolderHandlerState::RegisteredToThisAppStale,
            FolderHandlerRules::classify(
                &reading(verb, Some(&command(EXE)), verb, Some(&command(EXE))),
                EXE
            ),
            "{verb:?}"
        );
    }
}

/// The exact wreckage the first version left: a default verb named with nothing behind it. It
/// must not read as "unregistered", or the toggle would show off and leave it there.
#[test]
fn a_named_verb_with_no_command_is_not_mistaken_for_unregistered() {
    assert_eq!(
        FolderHandlerState::RegisteredToThisAppStale,
        FolderHandlerRules::classify(&reading(Some("open"), None, Some("open"), None), EXE)
    );
}

#[test]
fn another_file_manager_is_never_mistaken_for_ours() {
    assert_eq!(
        FolderHandlerState::RegisteredToAnotherApp,
        FolderHandlerRules::classify(
            &reading(
                Some("open"),
                Some(FOREIGN_COMMAND),
                Some("open"),
                Some(FOREIGN_COMMAND)
            ),
            EXE
        )
    );
}

/// A half-and-half reading must not be repaired into ours: the foreign arm wins, so the self-heal
/// leaves it alone rather than taking the other half from another program.
#[test]
fn a_foreign_half_wins_over_ours() {
    assert_eq!(
        FolderHandlerState::RegisteredToAnotherApp,
        FolderHandlerRules::classify(
            &reading(
                Some("open"),
                Some(&command(EXE)),
                Some("open"),
                Some(r#"C:\Other\Opus.exe "%1""#)
            ),
            EXE
        )
    );
}

/// `%1` on a drive root expands to `C:\`, and that trailing backslash escapes the closing quote of
/// `"%1"` — so the shell hands argv a path with a quote stuck on the end. The parser already
/// repairs it, which is why the registration can use the ordinary `"%1"` for drives as well.
#[test]
fn a_drive_root_survives_the_quote_its_trailing_backslash_escapes() {
    let mangled = CommandLine::parse(&[r#"C:\""#]);

    assert_eq!(1, mangled.targets.len());
    assert_eq!(r"C:\", mangled.targets[0].path);
    assert!(mangled.errors.is_empty());
}

// --- what the startup repair may touch ---

fn missing(_: &str) -> bool {
    false
}
fn present(_: &str) -> bool {
    true
}

/// The failure this rule exists for: the registered program is gone, so every folder double-click
/// fails until something puts a live path back.
#[test]
fn a_registration_pointing_at_something_gone_is_repaired() {
    assert!(FolderHandlerRules::should_repair(
        &registered(r"D:\Old\BertBrowser.exe"),
        EXE,
        missing
    ));
}

/// A debug build run beside a working install must not repoint the shell at `bin\Debug` — the
/// installed exe is still there and still opens folders fine.
#[test]
fn another_live_install_is_left_alone() {
    assert!(!FolderHandlerRules::should_repair(
        &registered(r"D:\Old\BertBrowser.exe"),
        EXE,
        present
    ));
}

/// Already pointing here, just incomplete — the drive half missing, or the default verb never
/// written. Repairing that costs nothing and is what finishes a half-done write.
#[test]
fn a_half_written_registration_for_this_exe_is_repaired() {
    assert!(FolderHandlerRules::should_repair(
        &reading(Some("open"), Some(&command(EXE)), None, None),
        EXE,
        present
    ));
}

#[test]
fn an_absent_registration_is_never_created_by_the_repair() {
    assert!(!FolderHandlerRules::should_repair(
        &FolderHandlerReading::none(),
        EXE,
        missing
    ));
}

/// Another program's registration is not ours to fix, even if its exe has gone.
#[test]
fn another_apps_registration_is_never_repaired() {
    assert!(!FolderHandlerRules::should_repair(
        &reading(
            Some("open"),
            Some(FOREIGN_COMMAND),
            Some("open"),
            Some(FOREIGN_COMMAND)
        ),
        EXE,
        missing
    ));
}

#[test]
fn a_working_registration_is_not_rewritten_on_every_launch() {
    assert!(!FolderHandlerRules::should_repair(
        &registered(EXE),
        EXE,
        present
    ));
}

// --- reading a command line back ---

#[test]
fn the_program_token_is_read_off_the_command() {
    for (command, expected) in [
        (
            r#""C:\dir\BertBrowser.exe" "%1""#,
            r"C:\dir\BertBrowser.exe",
        ),
        (r#"C:\dir\app.exe "%1""#, r"C:\dir\app.exe"),
        (r"C:\dir\app.exe", r"C:\dir\app.exe"),
        (r#"  "C:\a b\app.exe" %1  "#, r"C:\a b\app.exe"),
    ] {
        assert_eq!(
            Some(expected.to_string()),
            FolderHandlerRules::program_in(Some(command)),
            "{command}"
        );
    }
}

#[test]
fn an_unusable_command_names_no_program() {
    for command in [None, Some(""), Some("   "), Some("\"unterminated")] {
        assert_eq!(None, FolderHandlerRules::program_in(command), "{command:?}");
    }
}

#[test]
fn the_registered_program_is_reported_for_a_message() {
    assert_eq!(
        Some(FOREIGN_PROGRAM.to_string()),
        FolderHandlerRules::registered_program(&reading(
            Some("open"),
            Some(FOREIGN_COMMAND),
            None,
            None
        ))
    );
}
