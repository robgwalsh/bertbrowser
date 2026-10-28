// Port of tests/BertBrowser.Core.Tests/MftIndexClientTests.cs: the app's half of the split, driven
// over an in-memory pipe. The claims worth holding: state is mirrored exactly, a malformed message
// does not end a session, losing the helper is not silently survived, and nothing retries without
// being asked.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use bb_core::changes::ChangeLogPolicy;
use bb_core::ipc::{IndexProtocol, IndexVerb};
use bb_core::mft::{
    IndexHostLaunchResult, IndexHostLauncher, IndexListener, IndexStartMode, IndexStream,
    IndexTransportFactory, IndexerPresence, MftIndex, MftIndexClient,
};
use bb_test_support::pipe_fakes::{
    DuplexPair, FakeIndexHostLauncher, FakeIndexTransportFactory, ProtocolPeer,
};

const PATIENCE: Duration = Duration::from_secs(5);

fn client(
    launcher: Arc<dyn IndexHostLauncher>,
    transports: Arc<dyn IndexTransportFactory>,
) -> MftIndexClient {
    MftIndexClient::new(launcher, transports, None)
}

/// Starts a client attached to a helper that was already running: the ordinary case now, and the
/// one that costs no prompt.
fn connected() -> (MftIndexClient, ProtocolPeer, Arc<FakeIndexHostLauncher>) {
    let (app_end, helper_end) = DuplexPair::create();
    let launcher = Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::started(
        4242,
    )));
    let client = client(
        launcher.clone(),
        Arc::new(FakeIndexTransportFactory::always(Some(app_end))),
    );
    let helper = ProtocolPeer::new(helper_end);

    client.start().unwrap();
    greet(&helper);
    (client, helper, launcher)
}

/// Starts a client that found nothing to attach to and had to launch a helper: the path that
/// raises the prompt.
fn launched() -> (MftIndexClient, ProtocolPeer, Arc<FakeIndexHostLauncher>) {
    let (app_end, helper_end) = DuplexPair::create();
    let launcher = Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::started(
        4242,
    )));
    // Nothing answers the attach look; a stream appears only once one has been launched.
    let stream: Arc<dyn IndexStream> = app_end;
    let client = client(
        launcher.clone(),
        Arc::new(FakeIndexTransportFactory::new(move |pid| {
            pid.map(|_| stream.clone())
        })),
    );
    let helper = ProtocolPeer::new(helper_end);

    client.start().unwrap();
    greet(&helper);
    (client, helper, launcher)
}

/// The client greets first; answer as the helper does so it sends Start.
fn greet(helper: &ProtocolPeer) {
    assert_eq!(IndexVerb::Hello, helper.receive().unwrap().verb);
    helper.send(
        IndexVerb::Hello,
        &IndexProtocol::PROTOCOL_VERSION.to_string(),
    );
    helper.send(IndexVerb::Ready, "");
    assert_eq!(
        IndexVerb::Start,
        helper.receive_one_of(&[IndexVerb::Start]).unwrap().verb
    );
}

fn eventually(condition: impl Fn() -> bool, because: &str) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        sleep(Duration::from_millis(10));
    }
    panic!("{because}");
}

fn none_factory() -> Arc<FakeIndexTransportFactory> {
    Arc::new(FakeIndexTransportFactory::always(None))
}

/// **The behaviour this whole change exists for.** A helper left running by a previous app is
/// picked up without starting anything, so reopening BertBrowser costs no elevation prompt.
#[test]
fn attaches_to_a_running_helper_without_prompting() {
    let (client, helper, launcher) = connected();

    assert_eq!(0, launcher.launches());
    assert_eq!(IndexerPresence::Running, client.presence());
    helper.close();
}

/// And when there is nothing to attach to, asking does start one.
#[test]
fn launches_only_when_nothing_is_already_listening() {
    let (client, helper, launcher) = launched();

    assert_eq!(1, launcher.launches());
    assert_eq!(IndexerPresence::Running, client.presence());
    helper.close();
}

/// Startup's default: look for a helper, and if there is none, say nothing and start nothing. The
/// empty status line is the assertion that matters: the banner is what offers to start one.
#[test]
fn an_attach_that_finds_nothing_starts_nothing_and_says_nothing() {
    let launcher = Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::started(
        1,
    )));
    let transports = none_factory();
    let client = client(launcher.clone(), transports.clone());

    client.start_with(IndexStartMode::AttachOnly).unwrap();

    // Waiting on can_start, not on presence: presence begins at NotRunning, so waiting for that
    // would be satisfied before the session had run at all.
    eventually(|| client.can_start(), "the banner needs something to offer");
    assert_eq!(IndexerPresence::NotRunning, client.presence());
    assert_eq!(0, launcher.launches());
    assert_eq!("", client.status_text());
    assert_eq!(1, transports.attaches());
}

/// A second copy of the app cannot have the endpoint, and no prompt would change that, so the
/// offer to start one is withdrawn rather than made and disappointed.
#[test]
fn a_second_app_is_told_another_copy_owns_the_index() {
    let launcher = Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::started(
        1,
    )));
    let client = client(
        launcher.clone(),
        Arc::new(FakeIndexTransportFactory::unavailable(
            "another copy of BertBrowser is using it",
        )),
    );

    client.start_with(IndexStartMode::AttachOnly).unwrap();

    eventually(
        || !client.status_text().is_empty(),
        "it should say why there is no index",
    );
    assert!(client.status_text().to_lowercase().contains("another copy"));
    assert!(
        !client.can_start(),
        "a prompt cannot help, so nothing should offer one"
    );
    assert_eq!(0, launcher.launches());
}

/// The helper starts with recording off and learns otherwise only from the app: once after Start
/// on every session, and again whenever the setting changes.
#[test]
fn tells_the_helper_the_recording_policy_after_start_and_on_every_change() {
    let (client, helper, _) = connected();

    let initial = helper.receive_one_of(&[IndexVerb::Record]).unwrap();
    assert_eq!("0", initial.argument);

    client.set_change_log(ChangeLogPolicy::from_hours(6).unwrap());

    let changed = helper.receive_one_of(&[IndexVerb::Record]).unwrap();
    assert_eq!("6", changed.argument);
    assert_eq!(ChangeLogPolicy::from_hours(6).unwrap(), client.change_log());
}

#[test]
fn sends_the_stored_policy_to_a_helper_that_connects_later() {
    let (app_end, helper_end) = DuplexPair::create();
    let launcher = Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::started(
        4242,
    )));
    let client = client(
        launcher,
        Arc::new(FakeIndexTransportFactory::always(Some(app_end))),
    );
    let helper = ProtocolPeer::new(helper_end);
    client.set_change_log(ChangeLogPolicy::from_hours(168).unwrap()); // before there is anyone to tell

    client.start().unwrap();
    helper.receive();
    helper.send(
        IndexVerb::Hello,
        &IndexProtocol::PROTOCOL_VERSION.to_string(),
    );
    helper.send(IndexVerb::Ready, "");

    assert_eq!(
        IndexVerb::Start,
        helper.receive_one_of(&[IndexVerb::Start]).unwrap().verb
    );
    assert_eq!(
        "168",
        helper
            .receive_one_of(&[IndexVerb::Record])
            .unwrap()
            .argument
    );
}

#[test]
fn mirrors_building_and_idle_into_is_building_and_the_status_line() {
    let (client, helper, _) = connected();

    helper.send(IndexVerb::Building, "C");
    eventually(|| client.is_building(), "the client should mirror Building");
    eventually(
        || client.status_text() == "Indexing C:…",
        "the client formats its own status line",
    );

    helper.send(IndexVerb::Building, "D");
    eventually(
        || client.status_text() == "Indexing C:, D:…",
        "both drives should show",
    );

    helper.send(IndexVerb::Idle, "C");
    helper.send(IndexVerb::Idle, "D");
    eventually(|| !client.is_building(), "the client should mirror Idle");
    assert_eq!("", client.status_text());
}

struct Refreshed(Mutex<Vec<String>>);

impl IndexListener for Refreshed {
    fn index_refreshed(&self, root_key: &str) {
        self.0.lock().unwrap().push(root_key.to_string());
    }
}

#[test]
fn mirrors_complete_into_is_indexed_and_raises_index_refreshed() {
    let (client, helper, _) = connected();
    let refreshed = Arc::new(Refreshed(Mutex::new(Vec::new())));
    client.subscribe(refreshed.clone());

    helper.send(IndexVerb::Complete, r"C:\");

    eventually(
        || client.any_indexed(),
        "Complete should mark the root indexed",
    );
    assert!(client.is_indexed(r"C:\WINDOWS\SYSTEM32"));
    assert!(!client.is_indexed(r"D:\DATA"));
    eventually(
        || refreshed.0.lock().unwrap().iter().any(|r| r == r"C:\"),
        "IndexRefreshed should fire",
    );
}

struct Counting(AtomicUsize);

impl IndexListener for Counting {
    fn status_changed(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn raises_status_changed_as_state_arrives() {
    let (client, helper, _) = connected();
    let changes = Arc::new(Counting(AtomicUsize::new(0)));
    client.subscribe(changes.clone());

    helper.send(IndexVerb::Building, "C");

    eventually(
        || changes.0.load(Ordering::SeqCst) > 0,
        "StatusChanged should fire for a state push",
    );
}

/// One bad line must not cost the session. A peer that can end the conversation by sending
/// garbage is a peer that can turn the index off.
#[test]
fn skips_a_malformed_message_and_keeps_reading() {
    let (client, helper, _) = connected();

    helper.send_raw("this is not a message");
    helper.send_raw("IDX\tReindex\tC:\\Windows");
    helper.send_raw("IDX\t99");
    helper.send(IndexVerb::Complete, r"C:\");

    eventually(
        || client.is_indexed(r"C:\WINDOWS"),
        "the session should survive malformed lines",
    );
}

#[test]
fn a_dead_helper_clears_the_mirrored_state_and_offers_a_retry() {
    let (client, helper, _) = connected();
    helper.send(IndexVerb::Complete, r"C:\");
    eventually(|| client.any_indexed(), "precondition: the root is indexed");

    helper.close();

    eventually(
        || client.can_retry(),
        "losing the helper should offer a retry",
    );
    // Claiming a volume is indexed when nothing is keeping it current would route searches at a
    // database going stale.
    assert!(!client.any_indexed());
    assert!(!client.is_indexed(r"C:\WINDOWS"));
    assert_eq!("Search index stopped.", client.status_text());
}

#[test]
fn a_fatal_from_the_helper_becomes_the_status_line() {
    let (client, helper, _) = connected();

    helper.send(
        IndexVerb::Fatal,
        "The database schema is newer than this helper.",
    );

    eventually(|| client.can_retry(), "a fatal should offer a retry");
    assert_eq!(
        "The database schema is newer than this helper.",
        client.status_text()
    );
}

/// A half-applied update is the case this exists for.
#[test]
fn refuses_a_helper_speaking_another_protocol_version() {
    let (app_end, helper_end) = DuplexPair::create();
    let client = client(
        Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::started(
            1,
        ))),
        Arc::new(FakeIndexTransportFactory::always(Some(app_end))),
    );
    let helper = ProtocolPeer::new(helper_end);

    client.start().unwrap();
    helper.receive();
    helper.send(
        IndexVerb::Hello,
        &(IndexProtocol::PROTOCOL_VERSION + 1).to_string(),
    );

    eventually(
        || client.can_retry(),
        "a version mismatch should be reported, not mirrored",
    );
    assert!(client.status_text().contains("Search index unavailable"));

    // A helper left over from an older build will not exit on its own while it has an index to
    // keep, and it is holding the endpoint this build needs. Ask it to go.
    assert_eq!(
        IndexVerb::Shutdown,
        helper.receive_one_of(&[IndexVerb::Shutdown]).unwrap().verb
    );
}

#[test]
fn a_declined_prompt_is_retryable_and_says_so() {
    let client = client(
        Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::declined())),
        none_factory(),
    );

    client.start().unwrap();

    eventually(
        || client.can_retry(),
        "a declined prompt should be retryable",
    );
    assert_eq!(
        "Search index off — permission declined.",
        client.status_text()
    );
    assert!(!client.any_indexed());
}

/// A standard user cannot elevate at all, so there is nothing to retry, and nothing was prompted
/// for either.
#[test]
fn a_standard_user_is_never_prompted_and_cannot_retry() {
    let launcher = Arc::new(
        FakeIndexHostLauncher::new(IndexHostLaunchResult::not_administrator()).cannot_elevate(),
    );
    let client = client(launcher.clone(), none_factory());

    client.start().unwrap();

    eventually(
        || !client.status_text().is_empty(),
        "a standard user should be told",
    );
    assert!(!client.can_retry());
    assert_eq!(
        "Search index off — this account is not an administrator.",
        client.status_text()
    );
    assert_eq!(0, launcher.launches());
}

#[test]
fn a_helper_that_never_connects_is_reported_as_unavailable() {
    let client = client(
        Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::started(
            1,
        ))),
        none_factory(),
    );

    client.start().unwrap();

    eventually(
        || client.can_retry(),
        "a helper that never arrives should be retryable",
    );
    assert_eq!("Search index unavailable.", client.status_text());
}

/// The launcher's reason has to reach the status bar. There is no log behind it, and a build that
/// never copied the helper into place looks exactly like a UAC failure without this.
#[test]
fn a_launch_failure_says_why() {
    let client = client(
        Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::failed(
            "the index helper is missing",
        ))),
        none_factory(),
    );

    client.start().unwrap();

    eventually(|| client.can_retry(), "a failed launch should be retryable");
    assert_eq!(
        "Search index unavailable — the index helper is missing.",
        client.status_text()
    );
}

/// A detail quoted from Win32 arrives with its own full stop; one is enough.
#[test]
fn a_launch_failure_does_not_double_its_terminator() {
    let client = client(
        Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::failed(
            "The system cannot find the file specified.",
        ))),
        none_factory(),
    );

    client.start().unwrap();

    eventually(|| client.can_retry(), "a failed launch should be retryable");
    assert_eq!(
        "Search index unavailable — The system cannot find the file specified.",
        client.status_text()
    );
}

/// A launcher with nothing to add must not produce a dangling dash.
#[test]
fn a_launch_failure_with_no_detail_stays_terse() {
    let client = client(
        Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::failed(
            "",
        ))),
        none_factory(),
    );

    client.start().unwrap();

    eventually(|| client.can_retry(), "a failed launch should be retryable");
    assert_eq!("Search index unavailable.", client.status_text());
}

#[test]
fn retry_launches_exactly_once_more() {
    let launcher = Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::declined()));
    let client = client(launcher.clone(), none_factory());

    client.start().unwrap();
    eventually(|| client.can_retry(), "precondition: retryable");

    client.retry();

    eventually(|| launcher.launches() == 2, "retry should launch once more");
    sleep(Duration::from_millis(50));
    assert_eq!(2, launcher.launches());
}

/// Nothing retries on its own: every retry is a UAC prompt.
#[test]
fn nothing_retries_without_being_asked() {
    let launcher = Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::declined()));
    let client = client(launcher.clone(), none_factory());

    client.start().unwrap();
    eventually(|| client.can_retry(), "precondition: retryable");
    sleep(Duration::from_millis(300));

    assert_eq!(1, launcher.launches());
}

#[test]
fn retry_does_nothing_when_there_is_nothing_to_retry() {
    let launcher = Arc::new(
        FakeIndexHostLauncher::new(IndexHostLaunchResult::not_administrator()).cannot_elevate(),
    );
    let client = client(launcher.clone(), none_factory());

    client.start().unwrap();
    eventually(|| !client.status_text().is_empty(), "precondition: settled");

    client.retry();

    sleep(Duration::from_millis(50));
    assert_eq!(0, launcher.launches());
}

#[test]
fn starting_twice_only_launches_once() {
    let (client, helper, launcher) = launched();

    // A second Start must not raise a second prompt.
    client.start().unwrap();
    sleep(Duration::from_millis(50));
    assert_eq!(1, launcher.launches());
    helper.close();
}

#[test]
fn answers_a_ping_so_the_helper_can_tell_the_app_is_alive() {
    let (_client, helper, _) = connected();

    helper.send(IndexVerb::Ping, "");

    assert_eq!(
        IndexVerb::Pong,
        helper.receive_one_of(&[IndexVerb::Pong]).unwrap().verb
    );
}

/// **Closing the app must not end the helper.** This used to send Shutdown, and that line was what
/// cost a fresh elevation prompt and a full rebuild on every launch. Hanging up is all a closing
/// app may do now.
#[test]
fn dispose_lets_the_helper_keep_running() {
    let (client, helper, _) = connected();

    client.dispose();

    // The session ends (the helper reads end-of-stream) and nothing asked it to stop.
    while let Some(message) = helper.receive() {
        assert_ne!(IndexVerb::Shutdown, message.verb);
    }
}

/// Stopping it is a separate, deliberate act: the Settings button, and nothing else.
#[test]
fn stop_asks_the_helper_to_exit() {
    let (client, helper, _) = connected();

    client.stop();

    assert_eq!(
        IndexVerb::Shutdown,
        helper.receive_one_of(&[IndexVerb::Shutdown]).unwrap().verb
    );
}

#[test]
fn start_after_dispose_does_nothing() {
    let client = client(
        Arc::new(FakeIndexHostLauncher::new(IndexHostLaunchResult::declined())),
        none_factory(),
    );

    client.dispose();
    client.start().unwrap();

    sleep(Duration::from_millis(50));
    assert_eq!("", client.status_text());
}
