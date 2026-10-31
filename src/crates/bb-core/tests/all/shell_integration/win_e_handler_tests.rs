// The rule that decides whether Win+E opens BertBrowser. Nothing here touches the registry.
use bb_core::shell_integration::{
    FolderHandlerState, ShellIntegrationError, WinEHandlerReading, WinEHandlerRegistration,
    WinEHandlerRules,
};

const EXE: &str = r"C:\Users\Rob\AppData\Local\BertBrowser\current\BertBrowser.exe";
const MOVED: &str = r"D:\Elsewhere\BertBrowser.exe";
const FOREIGN: &str = r"C:\Program Files\Files\Files.exe";

fn registered(exe: &str) -> WinEHandlerReading {
    WinEHandlerReading::new(Some(&WinEHandlerRegistration::command_for(exe)), Some(""))
}

fn state(reading: &WinEHandlerReading) -> FolderHandlerState {
    WinEHandlerRules::classify(reading, EXE)
}

#[test]
fn the_command_is_the_quoted_program_and_nothing_else() {
    assert_eq!(
        format!("\"{EXE}\""),
        WinEHandlerRegistration::command_for(EXE)
    );
}

#[test]
fn the_command_is_written_before_the_empty_delegate() {
    let values = WinEHandlerRegistration::values_for(EXE).unwrap();
    assert_eq!(2, values.len());
    assert!(
        values
            .iter()
            .all(|v| v.key_path == WinEHandlerRegistration::COMMAND_KEY)
    );
    assert_eq!("", values[0].value_name);
    assert_eq!(WinEHandlerRegistration::command_for(EXE), values[0].data);
    assert_eq!("DelegateExecute", values[1].value_name);
    assert_eq!("", values[1].data);
}

#[test]
fn a_blank_executable_cannot_be_registered() {
    assert_eq!(
        Err(ShellIntegrationError::BlankExecutablePath),
        WinEHandlerRegistration::values_for("  ")
    );
}

#[test]
fn what_is_written_reads_back_as_registered() {
    assert_eq!(
        FolderHandlerState::RegisteredToThisApp,
        state(&registered(EXE))
    );
}

#[test]
fn a_path_differing_only_in_case_is_still_registered() {
    assert_eq!(
        FolderHandlerState::RegisteredToThisApp,
        state(&registered(&EXE.to_uppercase()))
    );
}

#[test]
fn a_stock_machine_is_not_registered() {
    assert_eq!(
        FolderHandlerState::NotRegistered,
        state(&WinEHandlerReading::default())
    );
}

#[test]
fn another_program_is_never_called_ours() {
    assert_eq!(
        FolderHandlerState::RegisteredToAnotherApp,
        state(&registered(FOREIGN))
    );
}

#[test]
fn a_delegate_with_no_command_belongs_to_someone_else() {
    let reading = WinEHandlerReading::new(None, Some("{00000000-0000-0000-0000-000000000000}"));
    assert_eq!(FolderHandlerState::RegisteredToAnotherApp, state(&reading));
}

#[test]
fn an_empty_delegate_with_no_command_is_ours_and_broken() {
    let reading = WinEHandlerReading::new(None, Some(""));
    assert_eq!(
        FolderHandlerState::RegisteredToThisAppStale,
        state(&reading)
    );
}

#[test]
fn our_program_without_the_empty_delegate_is_stale() {
    let command = WinEHandlerRegistration::command_for(EXE);
    assert_eq!(
        FolderHandlerState::RegisteredToThisAppStale,
        state(&WinEHandlerReading::new(Some(&command), None))
    );
    assert_eq!(
        FolderHandlerState::RegisteredToThisAppStale,
        state(&WinEHandlerReading::new(
            Some(&command),
            Some("{00000000-0000-0000-0000-000000000000}")
        ))
    );
}

#[test]
fn our_program_with_arguments_is_stale() {
    let command = format!("{} --new-tab", WinEHandlerRegistration::command_for(EXE));
    assert_eq!(
        FolderHandlerState::RegisteredToThisAppStale,
        state(&WinEHandlerReading::new(Some(&command), Some("")))
    );
}

#[test]
fn a_moved_install_is_stale() {
    assert_eq!(
        FolderHandlerState::RegisteredToThisAppStale,
        state(&registered(MOVED))
    );
}

#[test]
fn a_registration_naming_a_deleted_executable_is_repaired() {
    assert!(WinEHandlerRules::should_repair(
        &registered(MOVED),
        EXE,
        |_| false
    ));
}

#[test]
fn a_stale_path_whose_executable_still_exists_is_left_alone() {
    assert!(!WinEHandlerRules::should_repair(
        &registered(MOVED),
        EXE,
        |_| true
    ));
}

#[test]
fn a_damaged_registration_of_this_executable_is_repaired() {
    let reading = WinEHandlerReading::new(Some(&WinEHandlerRegistration::command_for(EXE)), None);
    assert!(WinEHandlerRules::should_repair(&reading, EXE, |_| true));
}

#[test]
fn absent_intact_and_foreign_registrations_are_never_repaired() {
    for reading in [
        WinEHandlerReading::default(),
        registered(EXE),
        registered(FOREIGN),
    ] {
        assert!(!WinEHandlerRules::should_repair(&reading, EXE, |_| false));
    }
}
