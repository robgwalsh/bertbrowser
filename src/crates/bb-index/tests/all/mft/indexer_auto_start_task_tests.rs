// Port of tests/BertBrowser.Core.Tests/IndexerAutoStartTaskTests.cs: the sign-in task definition.
// Several of these elements are silent defaults that would break the feature rather than fail
// loudly, so the tests are the specification. The C# read values back through XDocument; here the
// elements are found by name in the text and unescaped, which is the same question asked of the
// same bytes.
use bb_index::mft::{AutoStartState, IndexerAutoStartTask};

const SID: &str = "S-1-5-21-1845729561-1936007304-1872547691-1001";
const HELPER: &str = r"C:\Program Files\BertBrowser\bb-indexer.exe";
const APP_DIR: &str = r"C:\Users\Rob\AppData\Local\BertBrowser\current";
const DATA_DIR: &str = r"C:\Users\Rob\.bertbrowser";

fn xml() -> String {
    IndexerAutoStartTask::build_xml(HELPER, DATA_DIR, APP_DIR, SID)
}

/// The first `<element>`'s text, unescaped.
fn value(doc: &str, element: &str) -> String {
    IndexerAutoStartTask::unescape(
        IndexerAutoStartTask::first_element_text(doc, element)
            .unwrap_or_else(|| panic!("no <{element}>")),
    )
}

/// Every `<element>`'s text, unescaped.
fn values(doc: &str, element: &str) -> Vec<String> {
    let open = format!("<{element}>");
    let close = format!("</{element}>");
    let mut rest = doc;
    let mut found = Vec::new();
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let end = after.find(&close).expect("closed");
        found.push(IndexerAutoStartTask::unescape(&after[..end]));
        rest = &after[end + close.len()..];
    }
    found
}

/// **The whole point of the task.** Without HighestAvailable it runs unelevated, cannot read the
/// MFT, and the prompt it was meant to abolish comes back.
#[test]
fn runs_elevated() {
    assert_eq!("HighestAvailable", value(&xml(), "RunLevel"));
}

/// As the signed-in user, not SYSTEM. The helper derives its pipe name from its own token, so a
/// SYSTEM helper would listen on a name no app ever looks for.
#[test]
fn runs_as_the_user_whose_index_it_is() {
    let doc = xml();

    assert_eq!("InteractiveToken", value(&doc, "LogonType"));
    let ids = values(&doc, "UserId");
    assert!(!ids.is_empty());
    assert!(ids.iter().all(|id| id == SID), "{ids:?}");
}

#[test]
fn starts_at_sign_in() {
    assert_eq!(1, xml().matches("<LogonTrigger>").count());
}

/// **The default is three days, and it would kill the helper.** PT0S means no limit, which is what
/// a process meant to run until sign-out needs.
#[test]
fn never_times_the_helper_out() {
    assert_eq!("PT0S", value(&xml(), "ExecutionTimeLimit"));
}

/// The helper refuses to run twice anyway, but a task that queued a second one would leave the
/// scheduler reporting failures the user cannot explain.
#[test]
fn does_not_start_a_second_helper() {
    assert_eq!("IgnoreNew", value(&xml(), "MultipleInstancesPolicy"));
}

/// A laptop is exactly where the index matters most, and every one of these defaults would stop or
/// refuse to start the helper on battery or when the machine goes idle.
#[test]
fn keeps_running_on_battery_and_when_idle() {
    let doc = xml();

    assert_eq!("false", value(&doc, "DisallowStartIfOnBatteries"));
    assert_eq!("false", value(&doc, "StopIfGoingOnBatteries"));
    assert_eq!("false", value(&doc, "StopOnIdleEnd"));
    assert_eq!("false", value(&doc, "RunOnlyIfIdle"));
}

#[test]
fn passes_the_data_directory_and_the_app_directory_only() {
    let arguments = value(&xml(), "Arguments");

    assert_eq!(
        format!("--data-dir \"{DATA_DIR}\" --app-dir \"{APP_DIR}\""),
        arguments
    );
    assert!(!arguments.contains("--pipe"));
    assert!(!arguments.contains("--parent-pid"));
}

#[test]
fn runs_the_installed_copy_not_a_file_beside_the_app() {
    let command = value(&xml(), "Command");

    assert_eq!(HELPER, command);
    assert!(!command.starts_with(APP_DIR));
}

/// A profile path may perfectly well contain an ampersand, and an unescaped one makes the whole
/// definition unparseable, which the scheduler reports as a failure with no clue in it.
#[test]
fn escapes_paths_the_xml_would_otherwise_choke_on() {
    let doc = IndexerAutoStartTask::build_xml(
        r"C:\Program Files\A & B\bb-indexer.exe",
        r"C:\Users\Bill & Ted\.bertbrowser",
        r"C:\Users\Bill & Ted\app",
        SID,
    );

    assert!(value(&doc, "Arguments").contains(r"Bill & Ted\.bertbrowser"));
    assert!(value(&doc, "Arguments").contains(r"Bill & Ted\app"));
    assert_eq!(
        r"C:\Program Files\A & B\bb-indexer.exe",
        value(&doc, "Command")
    );
    // The raw text carries the entity, which is what makes the document parse at all.
    assert!(doc.contains("Bill &amp; Ted"));
    assert!(!doc.contains("Bill & Ted"));
}

fn state_of(xml: Option<&str>, copy_matches: bool) -> AutoStartState {
    IndexerAutoStartTask::state_from_task_xml(xml, HELPER, APP_DIR, copy_matches)
}

#[test]
fn no_task_is_off() {
    assert_eq!(AutoStartState::Off, state_of(None, true));
}

#[test]
fn a_registration_that_matches_everything_is_on() {
    assert_eq!(AutoStartState::On, state_of(Some(&xml()), true));
}

#[test]
fn paths_compare_the_way_windows_does() {
    let doc = IndexerAutoStartTask::build_xml(
        &HELPER.to_uppercase(),
        DATA_DIR,
        &format!("{APP_DIR}\\"),
        SID,
    );
    assert_eq!(AutoStartState::On, state_of(Some(&doc), true));
}

/// Every registration made before the installed copy existed pointed beside the app. Those users
/// are offered re-registration, which is the whole point of the fix.
#[test]
fn an_old_style_command_beside_the_app_is_stale() {
    let old = r#"<Task><Actions><Exec><Command>C:\Users\Rob\AppData\Local\BertBrowser\current\bb-indexer.exe</Command><Arguments>--data-dir "C:\Users\Rob\.bertbrowser"</Arguments></Exec></Actions></Task>"#;
    assert_eq!(AutoStartState::Stale, state_of(Some(old), true));
}

#[test]
fn the_installed_command_with_no_app_dir_is_stale() {
    let doc = xml().replace(
        &IndexerAutoStartTask::escape(&format!(" --app-dir \"{APP_DIR}\"")),
        "",
    );
    assert!(!doc.contains("--app-dir"));
    assert_eq!(AutoStartState::Stale, state_of(Some(&doc), true));
}

#[test]
fn a_different_app_directory_is_stale() {
    let doc = IndexerAutoStartTask::build_xml(HELPER, DATA_DIR, r"C:\Elsewhere\app", SID);
    assert_eq!(AutoStartState::Stale, state_of(Some(&doc), true));
}

#[test]
fn a_different_command_is_stale() {
    let doc = IndexerAutoStartTask::build_xml(r"C:\Temp\bb-indexer.exe", DATA_DIR, APP_DIR, SID);
    assert_eq!(AutoStartState::Stale, state_of(Some(&doc), true));
}

/// The installed copy missing or differing from the app's helper (an update has replaced the app)
/// is stale even though the task itself is exactly right.
#[test]
fn an_installed_copy_that_is_missing_or_differs_is_stale() {
    assert_eq!(AutoStartState::Stale, state_of(Some(&xml()), false));
}

#[test]
fn a_flag_that_merely_ends_in_app_dir_is_not_the_argument() {
    let doc = xml().replace("--app-dir", "--not-app-dir");
    assert_eq!(AutoStartState::Stale, state_of(Some(&doc), true));
}
