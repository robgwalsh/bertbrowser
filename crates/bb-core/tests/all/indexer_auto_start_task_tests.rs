// Port of tests/BertBrowser.Core.Tests/IndexerAutoStartTaskTests.cs: the sign-in task definition.
// Several of these elements are silent defaults that would break the feature rather than fail
// loudly, so the tests are the specification. The C# read values back through XDocument; here the
// elements are found by name in the text and unescaped, which is the same question asked of the
// same bytes.
use bb_core::mft::IndexerAutoStartTask;

const SID: &str = "S-1-5-21-1845729561-1936007304-1872547691-1001";
const HELPER: &str = r"C:\Users\Rob\AppData\Local\BertBrowser\current\BertBrowser.Indexer.exe";
const DATA_DIR: &str = r"C:\Users\Rob\.bertbrowser";

fn xml() -> String {
    IndexerAutoStartTask::build_xml(HELPER, DATA_DIR, SID)
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
fn passes_only_the_data_directory() {
    let arguments = value(&xml(), "Arguments");

    assert_eq!(format!("--data-dir \"{DATA_DIR}\""), arguments);
    assert!(!arguments.contains("--pipe"));
    assert!(!arguments.contains("--parent-pid"));
}

#[test]
fn points_at_the_helper() {
    assert_eq!(HELPER, value(&xml(), "Command"));
}

/// A profile path may perfectly well contain an ampersand, and an unescaped one makes the whole
/// definition unparseable, which the scheduler reports as a failure with no clue in it.
#[test]
fn escapes_paths_the_xml_would_otherwise_choke_on() {
    let doc = IndexerAutoStartTask::build_xml(HELPER, r"C:\Users\Bill & Ted\.bertbrowser", SID);

    assert!(value(&doc, "Arguments").contains("Bill & Ted"));
    // The raw text carries the entity, which is what makes the document parse at all.
    assert!(doc.contains("Bill &amp; Ted"));
    assert!(!doc.contains("Bill & Ted"));
}

/// An update moves the install directory, leaving the task aimed at an executable that is gone. It
/// cannot be repaired without another prompt, so the settings page has to be able to say so.
#[test]
fn knows_when_a_registered_task_points_somewhere_else() {
    for (registered, current, expected) in [
        (
            r"C:\Old\BertBrowser.Indexer.exe",
            r"C:\New\BertBrowser.Indexer.exe",
            true,
        ),
        (
            r"C:\App\BertBrowser.Indexer.exe",
            r"C:\App\BertBrowser.Indexer.exe",
            false,
        ),
        (
            r"c:\app\bertbrowser.indexer.exe",
            r"C:\App\BertBrowser.Indexer.exe",
            false,
        ),
        (
            "\"C:\\App\\BertBrowser.Indexer.exe\"",
            r"C:\App\BertBrowser.Indexer.exe",
            false,
        ),
    ] {
        assert_eq!(
            expected,
            IndexerAutoStartTask::needs_reregistration(Some(registered), current),
            "{registered} vs {current}"
        );
    }
}

#[test]
fn says_nothing_is_wrong_when_there_is_no_task() {
    assert!(!IndexerAutoStartTask::needs_reregistration(None, HELPER));
    assert!(!IndexerAutoStartTask::needs_reregistration(
        Some(""),
        HELPER
    ));
}
